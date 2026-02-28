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
 * QR detection: YOLO v12n (ONNX Runtime Web) for localization + rqrr (Rust/WASM)
 * for decode. Replaces the previous ZXing path for improved accuracy on
 * camouflaged QR codes with low-res front-facing cameras.
 *
 * This worker runs as { type: 'module' } so it can use ES imports.
 */

import init, { handle_request } from '../js-wasm/kipukas-server-pkg/kipukas_server.js';
import { initSession, runDetection } from './yolo-inference.js';
import { parseDetections, cropDetection } from './postprocess.js';

// ── WASM server init ───────────────────────────────────────────────

const wasmReady = init();
let initialized = false;

wasmReady.then(() => {
  initialized = true;
  console.log('[kipukas-worker] WASM server initialized');
}).catch((err) => {
  console.error('[kipukas-worker] WASM init failed:', err);
});

// ── YOLO + rqrr init (for QR detect + decode) ─────────────────────

const MODEL_URL = '/assets/js-wasm/yolo12n-qr.onnx';
const RQRR_WASM_JS = '/assets/js-wasm/rqrr-decode-pkg/rqrr_decode.js';

let rqrrModule = null;
let qrReady = false;
let qrInitializing = false;

async function initQR() {
  if (qrReady || qrInitializing) return;
  qrInitializing = true;

  try {
    const t0 = performance.now();
    const [backend, rqrr] = await Promise.all([
      initSession(MODEL_URL),
      import(RQRR_WASM_JS).then(async (mod) => {
        if (mod.default && typeof mod.default === 'function') {
          await mod.default();
        }
        return mod;
      }),
    ]);

    rqrrModule = rqrr;
    qrReady = true;
    const elapsed = Math.round(performance.now() - t0);
    console.log(`[kipukas-worker] YOLO+rqrr ready (${backend} backend, ${elapsed}ms)`);

    self.postMessage({
      type: 'STATUS',
      status: 'qr-ready',
      backend,
    });
  } catch (err) {
    console.error('[kipukas-worker] YOLO+rqrr init failed:', err);
  } finally {
    qrInitializing = false;
  }
}

// ── YOLO+rqrr QR decode pipeline ──────────────────────────────────

/**
 * Two-stage QR decode: YOLO detects QR bounding boxes, rqrr decodes content.
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

  const t0 = performance.now();

  // Stage 1: YOLO detection
  const output = await runDetection(rgba, width, height);
  const detections = parseDetections(output, width, height);

  const t1 = performance.now();

  if (detections.length === 0) return null;

  // Stage 2: rqrr decode on each detection (highest confidence first)
  for (const det of detections) {
    const crop = cropDetection(rgba, width, height, det, 0.15);
    const result = rqrrModule.decode_qr_crop(
      crop.rgba,
      crop.width,
      crop.height,
    );

    if (result && result.length > 0) {
      const t2 = performance.now();
      // Parse: "strategyIdx|strategyName|decodedText"
      const parts = result.split('|');
      const strategy = parts.length >= 3 ? parts[1] : 'unknown';
      const decodedUrl = parts.length >= 3 ? parts.slice(2).join('|') : result;

      console.log(
        `[kipukas-worker] QR decoded: ${decodedUrl} ` +
        `(YOLO: ${(t1 - t0).toFixed(0)}ms, rqrr[${strategy}]: ${(t2 - t1).toFixed(0)}ms, ` +
        `conf: ${det.confidence.toFixed(2)}, crop: ${crop.width}×${crop.height})`
      );

      return decodedUrl;
    }
  }

  // YOLO found QR-like regions but rqrr couldn't decode — normal for some frames
  return null;
}

// ── Frame-drop guard (YOLO is async, unlike ZXing) ─────────────────

let processingFrame = false;

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
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
