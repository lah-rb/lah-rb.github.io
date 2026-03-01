/**
 * zxing-decode.js — ESM wrapper around the existing Emscripten ZXing WASM build.
 *
 * The local zxing_reader.js is a classic IIFE (not ESM). Module workers can't
 * use importScripts(), so we fetch the script text and evaluate it to get the
 * ZXing factory function.
 *
 * Usage (from a module worker):
 *   import { initZXing, decodeQR } from './zxing-decode.js';
 *   await initZXing();
 *   const result = decodeQR(rgbaUint8, width, height);
 *   // result: { text, position } or null
 */

let zxing = null;

/**
 * Initialize the ZXing WASM module.
 * Fetches the Emscripten glue JS, evaluates it, then instantiates WASM.
 *
 * @returns {Promise<void>}
 */
export async function initZXing() {
  if (zxing) return;

  const SCRIPT_URL = '/assets/js-wasm/zxing_reader.js';

  // Fetch the non-module Emscripten glue script as text
  const resp = await fetch(SCRIPT_URL);
  const scriptText = await resp.text();

  // Evaluate to extract the ZXing factory (IIFE that returns a function)
  // The script sets `var ZXing = (...)();` — we capture it via Function()
  const ZXingFactory = new Function(scriptText + '\nreturn ZXing;')();

  // Instantiate with locateFile so it finds the .wasm sibling
  zxing = await ZXingFactory({
    locateFile: (file) => `/assets/js-wasm/${file}`,
  });

  console.log('[zxing-decode] ZXing WASM initialized');
}

/**
 * Decode a QR code from RGBA pixel data.
 *
 * @param {Uint8ClampedArray|Uint8Array} rgba - RGBA pixel buffer
 * @param {number} width - Image width
 * @param {number} height - Image height
 * @returns {{ text: string, position: Object }|null} Decoded result or null
 */
export function decodeQR(rgba, width, height) {
  if (!zxing) throw new Error('[zxing-decode] Not initialized — call initZXing() first');

  const buf = zxing._malloc(rgba.byteLength);
  try {
    zxing.HEAPU8.set(rgba, buf);
    const result = zxing.readBarcodeFromPixmap(buf, width, height, true, 'QRCode');

    if (result.format) {
      return {
        text: result.text,
        position: result.position,
      };
    }
    return null;
  } finally {
    zxing._free(buf);
  }
}

/**
 * Check if ZXing is ready.
 * @returns {boolean}
 */
export function isReady() {
  return zxing !== null;
}
