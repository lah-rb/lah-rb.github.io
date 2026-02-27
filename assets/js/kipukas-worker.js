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
 * Phase 2 addition: Also loads ZXing WASM for QR code decoding.
 * QR frame messages (type: 'QR_FRAME') are decoded by ZXing in JS,
 * then the result is formatted by the Rust WASM server.
 *
 * This worker runs as { type: 'module' } so it can use ES imports.
 */

import init, { handle_request } from '../js-wasm/kipukas-server-pkg/kipukas_server.js';

// ── WASM server init ───────────────────────────────────────────────

const wasmReady = init();
let initialized = false;

wasmReady.then(() => {
  initialized = true;
  console.log('[kipukas-worker] WASM server initialized');
}).catch((err) => {
  console.error('[kipukas-worker] WASM init failed:', err);
});

// ── ZXing init (for QR decode) ─────────────────────────────────────

let zxing = null;
let zxingReady = false;

// ZXing is a classic (non-ES-module) script. Module workers don't support
// importScripts(), so we fetch the script text and use indirect eval to
// execute it in the worker's global scope, defining the ZXing factory.
(async () => {
  try {
    const resp = await fetch('/assets/js-wasm/zxing_reader.js');
    const text = await resp.text();
    (0, eval)(text); // indirect eval — runs in global scope, defines ZXing on globalThis
    if (typeof ZXing === 'function') {
      zxing = await ZXing({ locateFile: (file) => `/assets/js-wasm/${file}` });
      zxingReady = true;
      console.log('[kipukas-worker] ZXing WASM initialized');
    }
  } catch (err) {
    // ZXing QR decode will be unavailable; camera scanning won't work
    // but the rest of the app continues fine.
    console.warn('[kipukas-worker] Could not load ZXing:', err.message);
  }
})();

// ── ZXing decode helper ────────────────────────────────────────────

/**
 * Decode a QR code from raw RGBA pixel data using ZXing.
 * @param {Uint8ClampedArray} pixels - RGBA pixel buffer
 * @param {number} width
 * @param {number} height
 * @returns {string|null} Decoded text or null
 */
function decodeQR(pixels, width, height) {
  if (!zxingReady || !zxing) return null;

  const buffer = zxing._malloc(pixels.byteLength);
  zxing.HEAPU8.set(pixels, buffer);
  const result = zxing.readBarcodeFromPixmap(buffer, width, height, false, 'QRCode');
  zxing._free(buffer);

  return result.text || null;
}

// ── Message handler ────────────────────────────────────────────────

self.onmessage = async (event) => {
  // ── QR frame decode (direct from qr-camera.js, no MessagePort) ──
  if (event.data?.type === 'QR_FRAME') {
    const { pixels, width, height } = event.data;
    const decoded = decodeQR(new Uint8ClampedArray(pixels), width, height);
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
        (pathname.startsWith('/api/game/') || pathname.startsWith('/api/room/'))
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

    // Auto-persist: after any POST to /api/game/* or /api/room/*, notify
    // main thread so it can fetch PLAYER_DOC base64 and save to localStorage.
    // Room routes are included because multiplayer alarm mutations
    // (yrs/alarm/add, yrs/alarm/tick, yrs/alarm/remove) call
    // export_to_local() which updates PLAYER_DOC, and room lifecycle
    // routes (create, join, disconnect) call seed_from_local() or
    // export_to_local() which also modify PLAYER_DOC.
    if (
      method === 'POST' &&
      (pathname.startsWith('/api/game/') || pathname.startsWith('/api/room/'))
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
