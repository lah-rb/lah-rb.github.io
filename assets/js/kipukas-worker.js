/**
 * kipukas-worker.js — the game engine's worker.
 *
 * Loads the engine (Rust compiled to WASM) and answers requests from the
 * page with it. Each request arrives as a message with a MessagePort to
 * answer on.
 *
 * It is also the engine's hands. The engine cannot open a socket, wait for
 * anything or reach the page, so after every post this worker takes what the
 * engine has queued and sees it done (kipukas-wire.js): the room's sockets
 * and waits here, storage and drawing passed on to the page.
 *
 * QR detection: YOLO26n (ONNX Runtime Web) for localization + ZXing (C++/WASM)
 * for decode. YOLO finds QR bounding boxes in the camera frame, then ZXing
 * decodes the cropped region with tryHarder mode for maximum accuracy on
 * camouflaged QR codes with low-res front-facing cameras. Which files the
 * model and the reader are, the engine says (kipukas-server/src/routes/qr.rs).
 *
 * This worker runs as { type: 'module' } so it can use ES imports.
 */

import init, { handle_request } from '../js-wasm/kipukas-server-pkg/kipukas_server.js';
import { initSession, runDetection } from './yolo-inference.js';
import { cropDetection, parseDetections } from './postprocess.js';
import { decodeQR as zxingDecode, initZXing } from './zxing-decode.js';
import { adaptiveThreshold } from './adaptive-threshold.js';
import { createWire } from './kipukas-wire.js';

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

// The margin left around a box the model finds when it is cut out for the
// reader, as a share of the box. The model was trained on boxes that already
// hold a margin about the code (see CONTRIBUTING.md, The model that finds
// codes), so little is added.
const MARGIN = 0.05;

let qrReady = false;
let qrInitializing = false;
let qrMode = null; // 'yolo+zxing' or 'zxing-only'
// Whether codes are found by computer vision (YOLO) before they are read:
// the player's choice, which the engine keeps and tells this worker of.
let cvEnabled = false;
// The addresses of the QR reader's file and the model's, as the engine names
// them.
let readerUrl = null;
let modelUrl = null;

async function initQR() {
  if (qrReady || qrInitializing || !readerUrl) return;
  qrInitializing = true;

  try {
    const t0 = performance.now();

    if (cvEnabled) {
      // CV enabled — load YOLO (WebGPU → WASM fallback) + ZXing in parallel
      const [yoloBackend] = await Promise.all([
        initSession(modelUrl),
        initZXing(readerUrl),
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
      await initZXing(readerUrl);
      qrReady = true;
      qrMode = 'zxing-only';
      const elapsed = Math.round(performance.now() - t0);
      console.log(`[kipukas-worker] ZXing-only mode (CV off, ${elapsed}ms)`);
    }
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
 * A frame arrives from qr-camera.js as a QR_FRAME message.
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
    const crop = cropDetection(rgba, width, height, det, MARGIN);

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

// ── What the engine asks for ───────────────────────────────────────
//
// The engine only answers requests: it cannot reach the page, open a socket
// or wait for anything. What it wants done is queued, and taken after every
// post (the handover). kipukas-wire.js does this worker's part of it, the
// sockets and waits of the room's connection, and passes the page the rest:
//
// - SAVE: storage keys to write or remove (kipukas-api.js)
// - ROOM: whether the player is in a room (kipukas-api.js)
// - REDRAW: routes whose views are to be drawn again (kipukas-view.js)
// - REFRESH: draw every view again (kipukas-view.js)
// - GO: a page of this site to go to (kipukas-api.js)
// - RELOAD: load the page again (kipukas-api.js)
//
// One part is this worker's own: the QR decoder, below.

/**
 * The engine's scanner has opened, or the player has changed how codes are
 * found (`cv`: by computer vision first). Get the decoder ready for it,
 * while the camera starts. `reader` and `model` are the addresses of the
 * reader's file and the model's.
 */
function wantQR({ cv, reader, model }) {
  const wasEnabled = cvEnabled;
  cvEnabled = !!cv;
  if (reader) readerUrl = reader;
  if (model) modelUrl = model;
  // Turned on with the decoder already set up without YOLO: set it up again.
  if (cvEnabled && !wasEnabled && qrReady && qrMode === 'zxing-only') {
    qrReady = false;
    qrMode = null;
  }
  // Turned off: the YOLO session stays loaded but is not used.
  if (!cvEnabled && qrMode === 'yolo+zxing') {
    qrMode = 'zxing-only';
  }
  initQR();
}

const wire = createWire({
  request: (method, path, body) => handle_request(method, path, '', body),
  toPage: (message) => self.postMessage(message),
  also: (handed) => {
    if (handed.qr) wantQR(handed.qr);
  },
});

const handOver = wire.handOver;

// The browser says the network is back: a good moment to check a connection.
self.addEventListener('online', () => {
  if (initialized) wire.reportPage({ event: 'online' });
});

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
  // ── The page's first message: everything stored under the engine's keys.
  // The engine puts itself back as it was (store.rs), and if that is in a
  // room, asks for a socket to return to it. It is handled before any
  // request, because requests are handled in the order they arrive.
  if (event.data?.type === 'START') {
    if (!initialized) await wasmReady;
    const reply = handle_request('POST', '/api/start', '', JSON.stringify(event.data.stored));
    if (reply !== 'ok') console.warn('[kipukas-worker] Could not read all that is stored:', reply);
    handOver();
    return;
  }

  // ── The page reports something of itself or of storage (store.rs) ──
  if (event.data?.type === 'PAGE') {
    if (!initialized) await wasmReady;
    wire.reportPage(event.data.report);
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
      // What the code says is the engine's to judge: it sends the page on
      // to where the code leads, or says in the scanner why it does not
      // (routes/qr.rs).
      if (!initialized) await wasmReady;
      handle_request(
        'POST',
        '/api/qr/status',
        '',
        `action=found&url=${encodeURIComponent(decoded)}`,
      );
      handOver();
    }
    return;
  }

  // ── A request from the page: a view to draw, or a tap or form of one
  // (kipukas-view.js) ──
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
