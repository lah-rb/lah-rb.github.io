/**
 * zxing-decode.js — the scanner's QR reader: zxing-cpp, compiled to
 * WebAssembly.
 *
 * The reader is one .wasm file with nothing of JavaScript's beside it,
 * built by zxing-reader/build.sh from a release of zxing-cpp, for QR codes
 * alone. It is fetched and started here, and offers three functions
 * (zxing-reader/reader.cpp): make room for a picture, read the code in it,
 * and where the code's text is. Which file it is, the engine says
 * (READER in kipukas-server/src/routes/qr.rs).
 *
 * Usage (from a module worker):
 *   import { decodeQR, initZXing } from './zxing-decode.js';
 *   await initZXing('/assets/js-wasm/zxing-3.2.0-rc1.wasm');
 *   const result = decodeQR(rgbaUint8, width, height);
 *   // result: { text } or null
 */

// The reader looks at each picture twice: as it is, and after closing small
// gaps in its dark parts (zxing-cpp's tryDenoise). That reads pictures out
// of focus or speckled that are otherwise not read, and takes more time
// over a picture with no code in it.
const DENOISE = 1;

// What the started module offers, and the address it was fetched from.
let reader = null;
let address = null;
// The module being fetched and started: its address, and a promise of it.
let starting = null;

const utf8 = new TextDecoder();

/**
 * Fetch and start the reader at `wasmUrl`, unless it is the one in use. A
 * reader that could not be fetched is asked for again at the next call.
 *
 * @param {string} wasmUrl - the address of the reader's .wasm file
 * @returns {Promise<void>}
 */
export async function initZXing(wasmUrl) {
  if (reader && address === wasmUrl) return;
  if (!starting || starting.address !== wasmUrl) {
    const promise = start(wasmUrl);
    starting = { address: wasmUrl, promise };
    promise.catch(() => {
      if (starting && starting.promise === promise) starting = null;
    });
  }
  const mine = starting;
  const started = await mine.promise;
  // Another reader was asked for meanwhile: this one does not take its place.
  if (starting !== mine) return;
  reader = started;
  address = wasmUrl;
}

/**
 * Read the QR code in a picture.
 *
 * @param {Uint8ClampedArray|Uint8Array} rgba - the picture, four bytes a pixel
 * @param {number} width
 * @param {number} height
 * @returns {{ text: string }|null} what the code says, or null if none is read
 */
export function decodeQR(rgba, width, height) {
  if (!reader) throw new Error('[zxing-decode] Not initialized — call initZXing() first');
  if (rgba.length !== width * height * 4) return null;
  const picture = reader.qr_picture(width, height);
  if (!picture) return null;
  // The memory is looked up at each call: it is another object once it has
  // grown.
  new Uint8Array(reader.memory.buffer, picture, rgba.length).set(rgba);
  const length = reader.qr_read(DENOISE);
  if (length < 0) throw new Error('the reader failed');
  if (length === 0) return null;
  return { text: utf8.decode(new Uint8Array(reader.memory.buffer, reader.qr_text(), length)) };
}

/**
 * Whether a reader has been started.
 * @returns {boolean}
 */
export function isReady() {
  return reader !== null;
}

/** Fetch the module at `wasmUrl` and start it. Gives what it offers. */
async function start(wasmUrl) {
  let memory = null;
  // The little the module asks of what it runs in. It was compiled for a
  // system with files and an environment, and here there is neither.
  const imports = {
    env: {
      // The module's memory grew. Nothing here holds on to it between calls.
      emscripten_notify_memory_growth() {},
    },
    wasi_snapshot_preview1: {
      fd_close: () => 0,
      // No environment variables: none counted, none written.
      environ_sizes_get(count, size) {
        const view = new DataView(memory.buffer);
        view.setUint32(count, 0, true);
        view.setUint32(size, 0, true);
        return 0;
      },
      environ_get: () => 0,
    },
  };

  const resp = await fetch(wasmUrl);
  if (!resp.ok) throw new Error(`[zxing-decode] ${wasmUrl}: ${resp.status}`);
  const { instance } = await WebAssembly.instantiate(await resp.arrayBuffer(), imports);
  const started = instance.exports;
  memory = started.memory;
  // Run what the module sets up before its first call.
  started._initialize();
  return started;
}
