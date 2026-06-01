/**
 * jxl-decode-worker.js — lean WASM worker that only decodes JPEG XL.
 *
 * One of a small pool spawned by kipukas-api.js so grid thumbnails decode in
 * parallel and off the /api worker. It loads the same kipukas-server pkg (one
 * binary) but uses only the `decode_jxl` export.
 *
 * Message in:  { bytes: ArrayBuffer, maxDim: number } + [responsePort]
 *   - bytes:  the whole .jxl file
 *   - maxDim: longest output side in px (0 = full resolution)
 *   - port:   the Service Worker's MessageChannel port to reply on
 * Reply out (on port): { ok: true, bytes: ArrayBuffer } | { ok: false, error }
 * Also posts { type: 'JXL_TIMING', ms, w, h } to the page for measurement.
 */

import init, { decode_jxl } from '../js-wasm/kipukas-server-pkg/kipukas_server.js';

const wasmReady = init();

self.onmessage = async (event) => {
  const { bytes, maxDim } = event.data;
  const port = event.ports[0];
  try {
    await wasmReady;
    const t0 = performance.now();
    const bmp = decode_jxl(new Uint8Array(bytes), maxDim || 0);
    const ms = performance.now() - t0;
    // BMP header carries width/height (LE int32 at offsets 18/22) — cheap to read.
    const dv = new DataView(bmp.buffer);
    const w = dv.getInt32(18, true);
    const h = dv.getInt32(22, true);
    self.postMessage({ type: 'JXL_TIMING', ms, w, h: Math.abs(h), maxDim: maxDim || 0 });
    port?.postMessage({ ok: true, bytes: bmp.buffer }, [bmp.buffer]);
  } catch (err) {
    console.error('[jxl-decode-worker] decode error:', err);
    port?.postMessage({ ok: false, error: String(err?.message || err) });
  }
};
