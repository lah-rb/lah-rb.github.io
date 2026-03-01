/**
 * kipukas-worker.js — Module Web Worker that runs the Kipukas WASM server.
 *
 * Loads the Rust/WASM binary and handles request messages from the page.
 * Each message includes a transferred MessagePort from a MessageChannel
 * that originated in the Service Worker, enabling the full relay:
 *
 *   HTMX fetch → SW intercepts → SW posts to page → page posts to this worker
 *   → WASM processes → responds on port → SW returns Response to HTMX
 *
 * QR detection: YOLO v12n (ONNX Runtime Web) for localization + ZXing (C++/WASM)
 * for decode. YOLO finds QR bounding boxes in the camera frame, then ZXing
 * decodes the cropped region with tryHarder mode for maximum accuracy on
 * camouflaged QR codes with low-res front-facing cameras.
 *
 * This worker runs as { type: 'module' } so it can use ES imports.
 */

import init, { handle_request } from '../js-wasm/kipukas-server-pkg/kipukas_server.js';
import { initSession, runDetection } from './yolo-inference.js';
import { parseDetections, cropDetection } from './postprocess.js';
import { initZXing, decodeQR as zxingDecode } from './zxing-decode.js';

// ── WASM server init ───────────────────────────────────────────────

const wasmReady = init();
let initialized = false;

wasmReady.then(() => {
  initialized = true;
  console.log('[kipukas-worker] WASM server initialized');
}).catch((err) => {
  console.error('[kipukas-worker] WASM init failed:', err);
});

// ── YOLO + ZXing init (for QR detect + decode) ────────────────────

const MODEL_URL = '/assets/js-wasm/yolo12n-qr.onnx';

let qrReady = false;
let qrInitializing = false;
let qrMode = null; // 'yolo+zxing' or 'zxing-only'

async function initQR() {
  if (qrReady || qrInitializing) return;
  qrInitializing = true;

  try {
    const t0 = performance.now();

    // Load YOLO (WebGPU-only) and ZXing in parallel
    const [yoloBackend] = await Promise.all([
      initSession(MODEL_URL),
      initZXing(),
    ]);

    qrReady = true;
    const elapsed = Math.round(performance.now() - t0);

    if (yoloBackend) {
      // WebGPU available → full YOLO+ZXing pipeline
      qrMode = 'yolo+zxing';
      console.log(`[kipukas-worker] YOLO+ZXing ready (${yoloBackend}, ${elapsed}ms)`);
    } else {
      // No WebGPU → ZXing-only mode (full-frame decode, no YOLO localization)
      qrMode = 'zxing-only';
      console.log(`[kipukas-worker] ZXing-only mode (no WebGPU, ${elapsed}ms)`);
    }

    self.postMessage({
      type: 'STATUS',
      status: 'qr-ready',
      backend: yoloBackend || 'zxing-only',
      mode: qrMode,
    });
  } catch (err) {
    console.error('[kipukas-worker] QR init failed:', err);
  } finally {
    qrInitializing = false;
  }
}

// ── YOLO+ZXing QR decode pipeline ─────────────────────────────────

/**
 * Two-stage QR decode: YOLO detects QR bounding boxes, ZXing decodes content.
 * Posts QR_BBOX messages back to main thread for visual overlay debugging.
 *
 * @param {Uint8ClampedArray} rgba - RGBA pixel buffer
 * @param {number} width
 * @param {number} height
 * @returns {Promise<string|null>} Decoded text or null
 */
async function decodeQR(rgba, width, height) {
  if (!qrReady) {
    await initQR();
    if (!qrReady) return null;
  }

  // ── ZXing-only mode (no WebGPU) — scan full frame directly ──
  if (qrMode === 'zxing-only') {
    try {
      const result = zxingDecode(rgba, width, height);
      if (result && result.text) {
        console.log(`[kipukas-worker] ZXing-only decoded: ${result.text}`);
        return result.text;
      }
    } catch (err) {
      console.warn('[kipukas-worker] ZXing-only error:', err.message);
    }
    return null;
  }

  // ── YOLO+ZXing mode (WebGPU available) ──
  const t0 = performance.now();

  // Stage 1: YOLO detection
  const output = await runDetection(rgba, width, height);
  const detections = parseDetections(output, width, height);

  const t1 = performance.now();

  // Always post bbox data to main thread for visual overlay
  // (even when no detections — clears old boxes)
  self.postMessage({
    type: 'QR_BBOX',
    detections: detections.map((d) => ({
      x: d.x,
      y: d.y,
      w: d.w,
      h: d.h,
      confidence: d.confidence,
    })),
    yoloMs: Math.round(t1 - t0),
    frameW: width,
    frameH: height,
    decoded: false,
  });

  if (detections.length === 0) return null;

  // Stage 2: ZXing decode on each detection (highest confidence first)
  for (const det of detections) {
    const crop = cropDetection(rgba, width, height, det, 0.15);

    try {
      const result = zxingDecode(crop.rgba, crop.width, crop.height);

      if (result && result.text) {
        const t2 = performance.now();

        console.log(
          `[kipukas-worker] QR decoded: ${result.text} ` +
          `(YOLO: ${(t1 - t0).toFixed(0)}ms, ZXing: ${(t2 - t1).toFixed(0)}ms, ` +
          `conf: ${det.confidence.toFixed(2)}, crop: ${crop.width}×${crop.height})`,
        );

        // Update bbox overlay to show successful decode
        self.postMessage({
          type: 'QR_BBOX',
          detections: detections.map((d) => ({
            x: d.x,
            y: d.y,
            w: d.w,
            h: d.h,
            confidence: d.confidence,
          })),
          yoloMs: Math.round(t1 - t0),
          frameW: width,
          frameH: height,
          decoded: true,
        });

        return result.text;
      }
    } catch (err) {
      console.warn('[kipukas-worker] ZXing decode error:', err.message);
    }
  }

  // YOLO found QR-like regions but ZXing couldn't decode — normal for some frames
  return null;
}

// ── Frame-drop guard (YOLO is async, unlike ZXing) ─────────────────

let processingFrame = false;

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
  // ── Preload QR stack (triggered 5s after page load by kipukas-api.js) ──
  if (event.data?.type === 'PRELOAD_QR') {
    initQR();
    return;
  }

  // ── QR frame decode (direct from qr-camera.js, no MessagePort) ──
  if (event.data?.type === 'QR_FRAME') {
    // Drop frame if previous inference is still running
    if (processingFrame) return;
    processingFrame = true;

    const { pixels, width, height } = event.data;
    let decoded;
    try {
      decoded = await decodeQR(new Uint8ClampedArray(pixels), width, height);
    } finally {
      processingFrame = false;
    }
    if (decoded) {
      // Format result via WASM, then post back to main thread
      if (!initialized) await wasmReady;
      const html = handle_request(
        'GET',
        '/api/qr/found',
        `?url=${encodeURIComponent(decoded)}`,
        '',
      );
      self.postMessage({ type: 'QR_FOUND', html, url: decoded });
    }
    return;
  }

  // ── Standard WASM request (from SW relay or direct fallback) ─────
  const { method, pathname, search, body } = event.data;
  const port = event.ports[0];

  // Fire-and-forget: no MessagePort means caller doesn't need a response.
  // Process the request, trigger PERSIST_STATE for localStorage sync, and return.
  // Used by Alpine damage tracker buttons (kipukasWorker.postMessage with no port).
  if (!port) {
    try {
      if (!initialized) await wasmReady;
      handle_request(method, pathname, search || '', body || '');
      if (
        method === 'POST' &&
        (pathname.startsWith('/api/game/') ||
          pathname.startsWith('/api/room/') ||
          pathname.startsWith('/api/player/'))
      ) {
        self.postMessage({ type: 'PERSIST_STATE' });
      }
    } catch (err) {
      console.error('[kipukas-worker] Fire-and-forget error:', err);
    }
    return;
  }

  try {
    if (!initialized) {
      await wasmReady;
    }

    const html = handle_request(method, pathname, search || '', body || '');
    port.postMessage({ ok: true, html });

    // Auto-persist: after any POST to /api/game/*, /api/room/*, or
    // /api/player/*, notify main thread so it can fetch PLAYER_DOC base64
    // and save to localStorage.
    // Room routes: multiplayer alarm mutations and room lifecycle routes
    // call export_to_local()/seed_from_local() which modify PLAYER_DOC.
    // Player routes: affinity declarations modify PLAYER_DOC directly.
    if (
      method === 'POST' &&
      (pathname.startsWith('/api/game/') ||
        pathname.startsWith('/api/room/') ||
        pathname.startsWith('/api/player/'))
    ) {
      self.postMessage({ type: 'PERSIST_STATE' });
    }
  } catch (err) {
    console.error('[kipukas-worker] WASM request error:', err);
    port.postMessage({
      ok: false,
      html: `<span class="text-kip-red">WASM error: ${err.message || err}</span>`,
    });
  }
};
