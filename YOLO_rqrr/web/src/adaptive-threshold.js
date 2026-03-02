/**
 * adaptive-threshold.js — Adaptive threshold preprocessing for QR decode.
 *
 * Applies mean-based adaptive thresholding to RGBA pixel data using an integral
 * image, matching rqrr's "at_coarse" strategy (block=21, c=10). This evens out
 * poor lighting and surface shine that decoders struggle with.
 *
 * The algorithm is identical to rqrr-wasm/src/lib.rs `adaptive_threshold()` and
 * the training augmentation in train/augment.py `adaptive_thresh(block=21, c=10)`,
 * keeping the entire stack consistent.
 *
 * Usage (from a module worker):
 *   import { adaptiveThreshold } from './adaptive-threshold.js';
 *   const processed = adaptiveThreshold(rgbaPixels, width, height);
 *   // processed: Uint8ClampedArray (RGBA) ready for decode
 */

/**
 * Apply adaptive threshold to RGBA pixel data.
 *
 * Converts to greyscale internally, computes the integral image, applies
 * mean-based thresholding, then writes the binary result back as RGBA
 * (R=G=B=0|255, A=255).
 *
 * @param {Uint8ClampedArray|Uint8Array} rgba - Source RGBA pixels
 * @param {number} w - Image width
 * @param {number} h - Image height
 * @param {number} [block=21] - Half-size of the local neighborhood
 * @param {number} [c=10] - Constant subtracted from the local mean
 * @returns {Uint8ClampedArray} Thresholded RGBA pixels
 */
export function adaptiveThreshold(rgba, w, h, block = 21, c = 10) {
  const len = w * h;

  // ── Step 1: RGBA → greyscale (BT.601 weights, matching rqrr) ──
  const grey = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    const base = i * 4;
    grey[i] = (77 * rgba[base] + 150 * rgba[base + 1] + 29 * rgba[base + 2]) >> 8;
  }

  // ── Step 2: Build integral image ──
  // integral[(y+1) * iw + (x+1)] = sum of grey[0..y][0..x]
  const iw = w + 1;
  const integral = new Int32Array(iw * (h + 1)); // row 0 and col 0 are zero-padding

  for (let y = 0; y < h; y++) {
    let rowSum = 0;
    for (let x = 0; x < w; x++) {
      rowSum += grey[y * w + x];
      integral[(y + 1) * iw + (x + 1)] = rowSum + integral[y * iw + (x + 1)];
    }
  }

  // ── Step 3: Adaptive threshold ──
  const out = new Uint8ClampedArray(len * 4);

  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      // Local neighborhood bounds (clamped to image)
      const y0 = y - block > 0 ? y - block : 0;
      const x0 = x - block > 0 ? x - block : 0;
      const y1 = y + block + 1 < h ? y + block + 1 : h;
      const x1 = x + block + 1 < w ? x + block + 1 : w;

      const area = (y1 - y0) * (x1 - x0);
      const sum = integral[y1 * iw + x1] - integral[y0 * iw + x1]
                - integral[y1 * iw + x0] + integral[y0 * iw + x0];

      // Threshold: local mean minus constant c
      const thresh = (sum / area) - c;
      const val = grey[y * w + x] < thresh ? 0 : 255;

      const idx = (y * w + x) * 4;
      out[idx] = val;      // R
      out[idx + 1] = val;  // G
      out[idx + 2] = val;  // B
      out[idx + 3] = 255;  // A
    }
  }

  return out;
}
