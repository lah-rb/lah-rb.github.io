/**
 * yolo-inference.js — ONNX Runtime Web session management for YOLOv12n.
 *
 * Handles model loading, backend selection (WebGPU → WASM fallback),
 * and image preprocessing for YOLO inference.
 *
 * Usage (from Web Worker):
 *   import { initSession, runDetection } from './yolo-inference.js';
 *   await initSession('/models/yolo12n-qr.onnx');
 *   const detections = await runDetection(rgbaPixels, 640, 480);
 */

let session = null;
let backend = null;
const INPUT_SIZE = 640; // YOLO input resolution

/**
 * Initialize the ONNX Runtime session.
 * Tries WebGPU first for GPU acceleration, falls back to WASM.
 *
 * @param {string} modelUrl - URL to the .onnx model file
 * @returns {Promise<string>} The backend that was selected ('webgpu' or 'wasm')
 */
export async function initSession(modelUrl) {
  if (session) return backend;

  // Import onnxruntime-web — in a worker context, use importScripts or dynamic import
  const ort = await import('https://cdn.jsdelivr.net/npm/onnxruntime-web@1.21.0/dist/ort.all.min.mjs');

  // Try WebGPU first (3-8× faster on mobile)
  const backends = ['webgpu', 'wasm'];

  for (const b of backends) {
    try {
      ort.env.wasm.numThreads = 1; // Single thread in worker
      session = await ort.InferenceSession.create(modelUrl, {
        executionProviders: [b],
        graphOptimizationLevel: 'all',
      });
      backend = b;
      console.log(`[yolo-inference] Session created with ${b} backend`);
      return backend;
    } catch (err) {
      console.warn(`[yolo-inference] ${b} backend failed:`, err.message);
      session = null;
    }
  }

  throw new Error('[yolo-inference] No suitable backend available');
}

/**
 * Run YOLO detection on an RGBA frame.
 *
 * @param {ArrayBuffer|Uint8ClampedArray} rgba - RGBA pixel data
 * @param {number} srcWidth - Source image width
 * @param {number} srcHeight - Source image height
 * @returns {Promise<Float32Array>} Raw model output tensor
 */
export async function runDetection(rgba, srcWidth, srcHeight) {
  if (!session) throw new Error('[yolo-inference] Session not initialized');

  const ort = await import('https://cdn.jsdelivr.net/npm/onnxruntime-web@1.21.0/dist/ort.all.min.mjs');

  // Preprocess: RGBA → RGB float32 [1, 3, 640, 640], normalized to [0, 1]
  const pixels = rgba instanceof Uint8ClampedArray ? rgba : new Uint8ClampedArray(rgba);
  const inputData = preprocessFrame(pixels, srcWidth, srcHeight);

  // Create input tensor [1, 3, INPUT_SIZE, INPUT_SIZE]
  const inputTensor = new ort.Tensor('float32', inputData, [1, 3, INPUT_SIZE, INPUT_SIZE]);

  // Run inference
  const feeds = {};
  const inputName = session.inputNames[0];
  feeds[inputName] = inputTensor;

  const results = await session.run(feeds);
  const outputName = session.outputNames[0];

  return {
    data: results[outputName].data,
    dims: results[outputName].dims,
    scaleX: srcWidth / INPUT_SIZE,
    scaleY: srcHeight / INPUT_SIZE,
  };
}

/**
 * Preprocess RGBA frame to YOLO input format.
 * Resizes to INPUT_SIZE×INPUT_SIZE, converts to RGB float32, normalizes to [0,1].
 *
 * @param {Uint8ClampedArray} rgba - Source RGBA pixels
 * @param {number} srcW - Source width
 * @param {number} srcH - Source height
 * @returns {Float32Array} [1, 3, INPUT_SIZE, INPUT_SIZE] tensor data
 */
function preprocessFrame(rgba, srcW, srcH) {
  const size = INPUT_SIZE;
  const channelSize = size * size;
  const data = new Float32Array(3 * channelSize);

  // Letterbox resize: maintain aspect ratio, pad with grey (114/255)
  const scale = Math.min(size / srcW, size / srcH);
  const newW = Math.round(srcW * scale);
  const newH = Math.round(srcH * scale);
  const padX = Math.round((size - newW) / 2);
  const padY = Math.round((size - newH) / 2);

  // Fill with letterbox padding value (114/255 ≈ 0.447)
  const padValue = 114 / 255;
  data.fill(padValue);

  // Nearest-neighbor resize and RGB extraction
  for (let y = 0; y < newH; y++) {
    const srcY = Math.min(Math.floor(y / scale), srcH - 1);
    for (let x = 0; x < newW; x++) {
      const srcX = Math.min(Math.floor(x / scale), srcW - 1);
      const srcIdx = (srcY * srcW + srcX) * 4;
      const dstX = x + padX;
      const dstY = y + padY;
      const dstIdx = dstY * size + dstX;

      // CHW format: [R plane][G plane][B plane]
      data[dstIdx] = rgba[srcIdx] / 255;                     // R
      data[channelSize + dstIdx] = rgba[srcIdx + 1] / 255;   // G
      data[2 * channelSize + dstIdx] = rgba[srcIdx + 2] / 255; // B
    }
  }

  return data;
}

/**
 * Check if the session is ready.
 * @returns {boolean}
 */
export function isReady() {
  return session !== null;
}

/**
 * Get the current backend name.
 * @returns {string|null}
 */
export function getBackend() {
  return backend;
}
