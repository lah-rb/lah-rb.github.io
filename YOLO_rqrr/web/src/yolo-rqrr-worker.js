/**
 * yolo-rqrr-worker.js — Web Worker orchestrating the two-stage QR pipeline.
 *
 * Stage 1: YOLO detection (ONNX Runtime Web) → bounding boxes
 * Stage 2: rqrr decode (Rust/WASM) → QR text
 *
 * Message protocol (compatible with existing qr-camera.js):
 *   IN:  { type: 'QR_FRAME', pixels: ArrayBuffer, width, height }
 *   OUT: { type: 'QR_FOUND', html: string, url: string }
 *   OUT: { type: 'STATUS', status: string, backend: string }
 *
 * The worker lazily initializes both YOLO and rqrr on first frame.
 */

import { initSession, runDetection } from './yolo-inference.js';
import { parseDetections, cropDetection } from './postprocess.js';
import { adaptiveThreshold } from './adaptive-threshold.js';

// ── Configuration ──────────────────────────────────────────────────

const MODEL_URL = '../models/yolo12n-qr.onnx';
const RQRR_WASM_URL = '../rqrr-wasm/pkg/rqrr_decode.js';

// ── State ──────────────────────────────────────────────────────────

let rqrrModule = null;
let initializing = false;
let ready = false;

// ── Initialization ─────────────────────────────────────────────────

async function initialize() {
  if (ready || initializing) return;
  initializing = true;

  try {
    // Initialize both in parallel
    const [backend, rqrr] = await Promise.all([
      initSession(MODEL_URL),
      import(RQRR_WASM_URL).then(async (mod) => {
        // wasm-pack modules need init() called
        if (mod.default && typeof mod.default === 'function') {
          await mod.default();
        }
        return mod;
      }),
    ]);

    rqrrModule = rqrr;
    ready = true;

    self.postMessage({
      type: 'STATUS',
      status: 'ready',
      backend,
    });

    console.log(`[yolo-rqrr-worker] Ready (YOLO: ${backend}, rqrr: wasm)`);
  } catch (err) {
    console.error('[yolo-rqrr-worker] Init failed:', err);
    self.postMessage({
      type: 'STATUS',
      status: 'error',
      error: err.message,
    });
  } finally {
    initializing = false;
  }
}

// ── Frame processing ───────────────────────────────────────────────

async function processFrame(pixels, width, height) {
  if (!ready) {
    await initialize();
    if (!ready) return; // Init failed
  }

  const t0 = performance.now();

  // Stage 1: YOLO detection
  const rgba = new Uint8ClampedArray(pixels);
  const output = await runDetection(rgba, width, height);
  const detections = parseDetections(output, width, height);

  const t1 = performance.now();

  if (detections.length === 0) {
    // No QR code detected in this frame
    return;
  }

  // Stage 2: Try rqrr decode on each detection (highest confidence first)
  // Preprocess with adaptive threshold to boost contrast and handle glare/lighting
  for (const det of detections) {
    const crop = cropDetection(rgba, width, height, det, 0.15);

    // Try adaptive-threshold preprocessed crop first — gives rqrr a clean
    // binary image so it hits strategy 0 (adaptive_thresh) immediately
    const atCrop = adaptiveThreshold(crop.rgba, crop.width, crop.height);
    let result = rqrrModule.decode_qr_crop(
      atCrop,
      crop.width,
      crop.height,
    );

    // Fallback to raw crop if AT preprocessing didn't help
    if (!result || result.length === 0) {
      result = rqrrModule.decode_qr_crop(
        crop.rgba,
        crop.width,
        crop.height,
      );
    }

    if (result && result.length > 0) {
      const t2 = performance.now();

      // Parse result: "strategyIdx|strategyName|decodedText"
      const parts = result.split('|');
      const strategy = parts.length >= 3 ? parts[1] : 'unknown';
      const decodedUrl = parts.length >= 3 ? parts.slice(2).join('|') : result;

      console.log(
        `[yolo-rqrr-worker] Decoded: ${decodedUrl} ` +
        `(YOLO: ${(t1 - t0).toFixed(0)}ms, rqrr[${strategy}]: ${(t2 - t1).toFixed(0)}ms, ` +
        `conf: ${det.confidence.toFixed(2)}, crop: ${crop.width}×${crop.height})`
      );

      // Return in same format as existing QR_FOUND for compatibility
      self.postMessage({
        type: 'QR_FOUND',
        url: decodedUrl,
        html: buildResultHtml(decodedUrl),
        meta: {
          yoloMs: Math.round(t1 - t0),
          rqrrMs: Math.round(t2 - t1),
          confidence: det.confidence,
          strategy,
          cropSize: `${crop.width}×${crop.height}`,
        },
      });
      return;
    }
  }

  // YOLO found QR-like regions but rqrr couldn't decode any
  // This is normal — not every frame will decode successfully
}

/**
 * Build result HTML compatible with the existing Kipukas QR flow.
 * This mimics what handle_request("/api/qr/found") returns.
 */
function buildResultHtml(url) {
  // Validate URL belongs to the Kipukas domain
  const isKipukas = url.startsWith('https://kipukas.com/') ||
                    url.startsWith('https://www.kipukas.com/') ||
                    url.startsWith('/');

  if (isKipukas) {
    return `<div class="qr-result">
      <p>Card found! Redirecting...</p>
      <script>window.location.href = ${JSON.stringify(url)};</script>
    </div>`;
  }

  return `<div class="qr-result">
    <p>QR code detected but URL not recognized:</p>
    <p><code>${url.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></p>
  </div>`;
}

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
  if (event.data?.type === 'QR_FRAME') {
    const { pixels, width, height } = event.data;
    try {
      await processFrame(pixels, width, height);
    } catch (err) {
      console.error('[yolo-rqrr-worker] Frame error:', err);
    }
  }

  if (event.data?.type === 'INIT') {
    await initialize();
  }
};

// Auto-initialize on worker start (lazy — won't block if model not yet available)
// Actual initialization happens on first QR_FRAME or explicit INIT message
console.log('[yolo-rqrr-worker] Worker loaded, waiting for frames...');
