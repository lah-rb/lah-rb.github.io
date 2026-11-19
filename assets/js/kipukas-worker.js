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
 * And it holds the two parts of the QR scanner that are modules of their
 * own: the model that finds codes in a camera frame (yolo-inference.js) and
 * the reader that reads them (zxing-decode.js). What is done to a frame
 * around those two is the engine's (kipukas-server/src/scan.rs).
 *
 * This worker runs as { type: 'module' } so it can use ES imports.
 */

import init, {
  handle_request,
  scan_found,
  scan_frame,
  scan_height,
  scan_model_input,
  scan_next,
  scan_width,
} from '../js-wasm/kipukas-server-pkg/kipukas_server.js';
import { engineBuild, hasSimd } from './engine-build.js';
import { initSession, runDetection } from './yolo-inference.js';
import { decodeQR as readCode, initZXing } from './zxing-decode.js';
import { createWire } from './kipukas-wire.js';

// ── WASM server init ───────────────────────────────────────────────

// The build of the engine this browser can run (engine-build.js).
const wasmReady = init(engineBuild());
let initialized = false;

wasmReady.then(() => {
  initialized = true;
  console.log('[kipukas-worker] WASM server initialized');
}).catch((err) => {
  console.error('[kipukas-worker] WASM init failed:', err);
});

// ── The QR scanner's frames ────────────────────────────────────────
//
// A camera frame arrives from qr-camera.js (QR_FRAME) and goes to the
// engine, which has all of what is done to it but two things that are
// modules of their own, held here: the model that finds codes
// (yolo-inference.js) and the reader that reads them (zxing-decode.js).
// The engine names the files of both (routes/qr.rs).

// Whether codes are found by computer vision (the model) before they are
// read: the player's choice, which the engine keeps and tells this worker of.
let cvEnabled = false;
// The addresses of the reader's file, the model's and that of what runs
// the model, as the engine names them.
let readerUrl = null;
let modelUrl = null;
let runnerUrl = null;
// Where the model stands: 'none' before it is asked for, then 'loading',
// and 'ready' or 'failed'. The scanner is told (routes/qr.rs), and says to
// the player what it means for them.
let finder = 'none';
// When the model last failed to load, and how often it was fetched and
// then could not be run.
let finderFailedAt = 0;
let finderCouldNotRun = 0;
// A model that could not be loaded is asked for again when a frame arrives
// this long after (milliseconds), when the browser says the network is
// back, and when the scanner opens or computer vision is turned on. One
// that was fetched and could not be run this many times is left until the
// scanner asks again: more tries would only take more memory.
const FINDER_AGAIN_AFTER = 10000;
const FINDER_TRIES = 3;
// Whether the reader could be loaded, as the scanner was last told.
let readerFailed = false;
// Whether the scanner is to be told what each frame takes: a setting of the
// player's, which the engine keeps and tells this worker of. Only this
// worker can time a frame. What is made of the times is the scanner's.
let statusWanted = false;

/** Tell the scanner something of what this worker holds for it. */
function tellScanner(what, state) {
  if (!initialized) return;
  handle_request('POST', '/api/qr/status', '', `action=${what}&state=${state}`);
  handOver();
}

function finderIs(state) {
  finder = state;
  tellScanner('finder', state);
}

/**
 * Fetch the model and make it ready, unless it is, or is being. `asked`:
 * the scanner has asked for it, or the network is back, so a model that
 * failed is tried again whenever that was.
 */
function loadFinder(asked = false) {
  if (!modelUrl || !runnerUrl || finder === 'loading' || finder === 'ready') return;
  if (asked) finderCouldNotRun = 0;
  if (finder === 'failed' && !asked) {
    if (finderCouldNotRun >= FINDER_TRIES) return;
    if (performance.now() - finderFailedAt < FINDER_AGAIN_AFTER) return;
  }
  // What runs the model needs SIMD, on either of its backends. A browser
  // without it is spared the fetching of some 14 MB it could not use.
  if (!hasSimd()) {
    finderCouldNotRun = FINDER_TRIES;
    if (finder !== 'failed') finderIs('failed');
    return;
  }
  finderIs('loading');
  const t0 = performance.now();
  const failed = () => {
    finderFailedAt = performance.now();
    finderIs('failed');
  };
  initSession(modelUrl, runnerUrl).then((backend) => {
    if (!backend) {
      finderCouldNotRun += 1;
      console.error('[kipukas-worker] The model that finds codes could not be run');
      failed();
      return;
    }
    const elapsed = Math.round(performance.now() - t0);
    console.log(
      `[kipukas-worker] The model that finds codes is ready (${backend}, ${elapsed}ms)`,
    );
    finderIs('ready');
  }, (err) => {
    console.error('[kipukas-worker] The model that finds codes could not be loaded:', err);
    failed();
  });
}

/** Do a piece of work, and add how long it took to `took[part]`. */
function timed(took, part, work) {
  const t0 = performance.now();
  try {
    return work();
  } finally {
    took[part] += performance.now() - t0;
  }
}

/** Tell the scanner what a frame took, and when it was begun (see lookForCode). */
function tellScannerOfFrame(took) {
  const model = took.model === null ? '' : `&model=${took.model}`;
  handle_request(
    'POST',
    '/api/qr/status',
    '',
    `action=frame&engine=${took.engine}&reader=${took.reader}${model}&at=${took.at}`,
  );
  handOver();
}

/**
 * Look for a code in a camera frame, and give what it says, or null.
 *
 * The engine takes the frame and offers the reader pictures of it one after
 * another until one is read (scan.rs). With computer vision on and the
 * model ready, the model is run on the frame first, and the pictures are
 * the boxes it found. Until the model is ready, and when it cannot be
 * loaded (no connection, and never fetched before), the frame is read
 * whole, as it is with computer vision off.
 *
 * What the model found is posted to the page, which draws it over the
 * camera's picture (QR_BBOX, qr-camera.js).
 *
 * What the frame takes is written into `took`, in milliseconds, by whose
 * work it is: the engine's, the reader's, and the model's if it is run
 * (null if not). `looked` says the frame got as far as being looked at.
 * `at` is when the frame arrived, by this worker's clock.
 *
 * @param {ArrayBuffer} pixels - the frame, four bytes a pixel
 * @param {number} width
 * @param {number} height
 * @param {{looked: boolean, at: number, engine: number, reader: number, model: number|null}} took
 * @returns {Promise<string|null>}
 */
async function lookForCode(pixels, width, height, took) {
  if (!readerUrl) return null;
  if (!initialized) await wasmReady;
  try {
    await initZXing(readerUrl);
  } catch (err) {
    // Said once, not at every frame it goes on for.
    if (!readerFailed) {
      console.error('[kipukas-worker] The QR reader could not be loaded:', err);
      tellScanner('reader', 'failed');
    }
    readerFailed = true;
    return null;
  }
  if (readerFailed) tellScanner('reader', 'ready');
  readerFailed = false;
  const engine = (work) => timed(took, 'engine', work);
  if (!engine(() => scan_frame(new Uint8Array(pixels), width, height))) return null;
  took.looked = true;

  let found = null;
  if (cvEnabled) {
    loadFinder();
    if (finder === 'ready') {
      const t0 = performance.now();
      const input = engine(scan_model_input);
      const t1 = performance.now();
      const output = await runDetection(input);
      took.model = performance.now() - t1;
      const boxes = engine(() => scan_found(output.data, output.places));
      found = {
        type: 'QR_BBOX',
        boxes,
        yoloMs: Math.round(performance.now() - t0),
        frameW: width,
        frameH: height,
      };
      // Posted even when there are none, which clears the boxes drawn before.
      self.postMessage({ ...found, decoded: false });
    }
  }

  for (let picture = engine(scan_next); picture; picture = engine(scan_next)) {
    let result = null;
    try {
      result = timed(took, 'reader', () => readCode(picture, scan_width(), scan_height()));
    } catch (err) {
      console.warn('[kipukas-worker] The QR reader failed on a picture:', err.message);
    }
    if (result && result.text) {
      // The boxes again, drawn as read.
      if (found) self.postMessage({ ...found, decoded: true });
      return result.text;
    }
  }
  return null;
}

// A frame is let go if the one before it is still being looked at. The page
// takes the next only when told this one is done with (QR_DONE), so that
// happens only if it gave up waiting to be told.
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
 * found (`cv`: by computer vision first) or whether the scanner shows what
 * its frames take (`status`). `reader`, `model` and `runner` are the
 * addresses of the reader's file, the model's and that of what runs the
 * model. What is wanted is fetched now, while the camera starts.
 */
function wantQR({ cv, reader, model, runner, status }) {
  cvEnabled = !!cv;
  statusWanted = !!status;
  if (reader) readerUrl = reader;
  if (model) modelUrl = model;
  if (runner) runnerUrl = runner;
  // A reader that cannot be fetched now is asked for again with each frame.
  if (readerUrl) initZXing(readerUrl).catch(() => {});
  if (cvEnabled) loadFinder(true);
}

const wire = createWire({
  request: (method, path, body) => handle_request(method, path, '', body),
  toPage: (message) => self.postMessage(message),
  also: (handed) => {
    if (handed.qr) wantQR(handed.qr);
  },
});

const handOver = wire.handOver;

// The browser says the network is back: a good moment to check a connection,
// and to fetch a model that could not be.
self.addEventListener('online', () => {
  if (initialized) wire.reportPage({ event: 'online' });
  if (cvEnabled) loadFinder(true);
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
    if (processingFrame) return;
    processingFrame = true;

    const { pixels, width, height } = event.data;
    const took = { looked: false, at: performance.now(), engine: 0, reader: 0, model: null };
    let decoded;
    try {
      decoded = await lookForCode(pixels, width, height, took);
    } catch (err) {
      console.error('[kipukas-worker] A frame could not be looked at:', err);
      took.looked = false;
    } finally {
      processingFrame = false;
    }
    // The page takes its next frame when it hears this (qr-camera.js).
    self.postMessage({ type: 'QR_DONE' });
    if (statusWanted && took.looked) tellScannerOfFrame(took);
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
