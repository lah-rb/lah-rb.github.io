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
import { adaptiveThreshold } from './adaptive-threshold.js';

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
let cvEnabled = false; // User-controlled CV toggle (persisted in localStorage on main thread)

async function initQR() {
  if (qrReady || qrInitializing) return;
  qrInitializing = true;

  try {
    const t0 = performance.now();

    if (cvEnabled) {
      // CV enabled — load YOLO (WebGPU → WASM fallback) + ZXing in parallel
      const [yoloBackend] = await Promise.all([
        initSession(MODEL_URL),
        initZXing(),
      ]);

      qrReady = true;
      const elapsed = Math.round(performance.now() - t0);

      if (yoloBackend) {
        qrMode = 'yolo+zxing';
        console.log(`[kipukas-worker] YOLO+ZXing ready (${yoloBackend}, ${elapsed}ms)`);
      } else {
        qrMode = 'zxing-only';
        console.log(`[kipukas-worker] CV enabled but YOLO failed — ZXing-only (${elapsed}ms)`);
      }
    } else {
      // CV disabled — ZXing only (lightweight, works everywhere)
      await initZXing();
      qrReady = true;
      qrMode = 'zxing-only';
      const elapsed = Math.round(performance.now() - t0);
      console.log(`[kipukas-worker] ZXing-only mode (CV off, ${elapsed}ms)`);
    }

    self.postMessage({
      type: 'STATUS',
      status: 'qr-ready',
      mode: qrMode,
      cvEnabled,
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
    // Always preprocess with adaptive threshold first — evens out poor
    // lighting and surface shine that ZXing's binarizer struggles with
    try {
      const atFrame = adaptiveThreshold(rgba, width, height);
      const result = zxingDecode(atFrame, width, height);
      if (result && result.text) {
        console.log(`[kipukas-worker] ZXing-only decoded (AT): ${result.text}`);
        return result.text;
      }
    } catch (err) {
      console.warn('[kipukas-worker] ZXing-only AT error:', err.message);
    }
    // Fallback: try raw frame in case threshold hurts this particular image
    try {
      const result = zxingDecode(rgba, width, height);
      if (result && result.text) {
        console.log(`[kipukas-worker] ZXing-only decoded (raw): ${result.text}`);
        return result.text;
      }
    } catch (err) {
      console.warn('[kipukas-worker] ZXing-only raw error:', err.message);
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
  // Try adaptive-threshold preprocessed crop first, then raw crop as fallback
  for (const det of detections) {
    const crop = cropDetection(rgba, width, height, det, 0.15);

    // Attempt 1: Adaptive threshold preprocessed crop (handles glare/lighting)
    try {
      const atCrop = adaptiveThreshold(crop.rgba, crop.width, crop.height);
      const result = zxingDecode(atCrop, crop.width, crop.height);

      if (result && result.text) {
        const t2 = performance.now();

        console.log(
          `[kipukas-worker] QR decoded (AT): ${result.text} ` +
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
      console.warn('[kipukas-worker] ZXing AT decode error:', err.message);
    }

    // Attempt 2: Raw crop fallback
    try {
      const result = zxingDecode(crop.rgba, crop.width, crop.height);

      if (result && result.text) {
        const t2 = performance.now();

        console.log(
          `[kipukas-worker] QR decoded (raw): ${result.text} ` +
          `(YOLO: ${(t1 - t0).toFixed(0)}ms, ZXing: ${(t2 - t1).toFixed(0)}ms, ` +
          `conf: ${det.confidence.toFixed(2)}, crop: ${crop.width}×${crop.height})`,
        );

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
      console.warn('[kipukas-worker] ZXing raw decode error:', err.message);
    }
  }

  // YOLO found QR-like regions but ZXing couldn't decode — normal for some frames
  return null;
}

// ── Frame-drop guard (YOLO is async, unlike ZXing) ─────────────────

let processingFrame = false;

// ── What the engine asks of the page ───────────────────────────────

/**
 * After a post: hand the page what the engine has queued for it (page.rs in
 * the engine). The page does each as it is told:
 *
 * - SAVE: storage keys to write or remove (kipukas-api.js). First, so that
 *   what follows finds storage as the engine left it.
 * - ROOM: the room the player is in, or null (kipukas-api.js).
 * - ROOM_UPDATES: room-document updates for the other player. The page
 *   holds the connection and sends them (kipukas-multiplayer.js).
 * - REDRAW: routes whose views are to be drawn again, because the post
 *   changed what they show (kipukas-view.js).
 * - QUIET: another tab holds the room now (kipukas-multiplayer.js).
 * - RELOAD: load the page again (kipukas-api.js).
 */
function handOver() {
  try {
    const handed = JSON.parse(handle_request('POST', '/api/handover', '', ''));
    if (handed.save) self.postMessage({ type: 'SAVE', writes: handed.save });
    if ('room' in handed) self.postMessage({ type: 'ROOM', room: handed.room });
    if (handed.updates) self.postMessage({ type: 'ROOM_UPDATES', updates: handed.updates });
    if (handed.redraw) self.postMessage({ type: 'REDRAW', routes: handed.redraw });
    if (handed.quiet) self.postMessage({ type: 'QUIET' });
    if (handed.reload) self.postMessage({ type: 'RELOAD' });
  } catch (err) {
    console.error('[kipukas-worker] Could not hand over to the page:', err);
  }
}

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
  // ── The page's first message: everything stored under the engine's keys.
  // The engine puts itself back as it was (store.rs). It is handled before
  // any request, because requests are handled in the order they arrive.
  if (event.data?.type === 'START') {
    if (!initialized) await wasmReady;
    const reply = handle_request('POST', '/api/start', '', JSON.stringify(event.data.stored));
    if (reply !== 'ok') console.warn('[kipukas-worker] Not everything stored could be read:', reply);
    handOver();
    return;
  }

  // ── The page reports something of itself or of storage (store.rs) ──
  if (event.data?.type === 'PAGE') {
    if (!initialized) await wasmReady;
    handle_request('POST', '/api/page', '', JSON.stringify(event.data.report));
    handOver();
    return;
  }

  // ── CV mode toggle (from scanner UI button) ──
  if (event.data?.type === 'SET_CV_MODE') {
    const wasEnabled = cvEnabled;
    cvEnabled = !!event.data.enabled;
    console.log(`[kipukas-worker] CV mode ${cvEnabled ? 'ON' : 'OFF'}`);
    // If switching to CV mode and QR is already initialized as zxing-only,
    // reset so next frame triggers YOLO init
    if (cvEnabled && !wasEnabled && qrReady && qrMode === 'zxing-only') {
      qrReady = false;
      qrMode = null;
    }
    // If switching OFF while YOLO was active, just change mode
    // (YOLO session stays loaded but won't be used)
    if (!cvEnabled && qrMode === 'yolo+zxing') {
      qrMode = 'zxing-only';
    }
    self.postMessage({ type: 'STATUS', status: 'cv-mode-changed', mode: qrMode, cvEnabled });
    return;
  }

  // ── Preload QR stack (triggered 5s after page load by kipukas-api.js) ──
  if (event.data?.type === 'PRELOAD_QR') {
    cvEnabled = !!event.data.cvEnabled;
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

  // No MessagePort: the caller does not need the reply.
  if (!port) {
    try {
      if (!initialized) await wasmReady;
      handle_request(method, pathname, search || '', body || '');
      if (method === 'POST') handOver();
    } catch (err) {
      console.error('[kipukas-worker] Request error:', err);
    }
    return;
  }

  try {
    if (!initialized) {
      await wasmReady;
    }

    const html = handle_request(method, pathname, search || '', body || '');
    port.postMessage({ ok: true, html });
    // A post may have changed the engine's state. What follows from that
    // (what to store, what to draw again) is the engine's to say.
    if (method === 'POST') handOver();
  } catch (err) {
    console.error('[kipukas-worker] WASM request error:', err);
    port.postMessage({
      ok: false,
      html: `<span class="text-kip-red">WASM error: ${err.message || err}</span>`,
    });
  }
};
