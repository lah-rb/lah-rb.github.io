/**
 * kipukas-api.js — starts the game engine for this page and does for it what
 * only a page can.
 *
 * The engine (Rust compiled to WASM) runs in a worker of its own
 * (kipukas-worker.js), exposed as window.kipukasWorker. The page's views are
 * drawn from it (kipukas-view.js), and it holds the multiplayer connection.
 * It cannot reach the page or the browser's storage, so after anything
 * changes it says what it wants done, and this file does it:
 *
 *   SAVE     write or remove storage keys
 *   ROOM     whether the player is in a room, for the toolbar
 *   GO       go to another page of this site (a scanned QR code)
 *   RELOAD   load this page again
 *   DOWNLOAD hand the player a file to keep (a backup of their data)
 *
 * It also starts the pool of workers that decode card art (JPEG XL), which
 * the service worker hands images to through this page.
 *
 * Load this script with defer, ahead of the Alpine bundle.
 */

// ============================================
// THE ENGINE AND THE BROWSER'S STORAGE
// ============================================
// Every page load starts a fresh game engine, in a worker of its own. What
// must outlive a page is kept in localStorage, which the engine cannot reach
// and this page can. The engine decides what is stored and when
// (kipukas-server/src/store.rs); this file only fetches and carries:
//
//   - at load it hands the engine everything stored under the engine's keys
//     (START, below), from which the engine puts itself back as it was;
//   - it writes and removes the keys the engine names after a change (SAVE);
//   - it tells the engine when another tab changes one of them, and when
//     this page comes back into view (PAGE). What follows from that, such as
//     loading the page again because its engine is out of date, is the
//     engine's to ask for (RELOAD).
const ENGINE_KEYS = ['kipukas_player_doc', 'kipukas_room', 'kipukas_room_doc', 'kipukas_room_tab'];

// Handed to the engine at load with the rest, but not watched for changes:
//   - a relay address to use in place of the engine's own, for local work,
//     written by nobody but a developer. Run the relay locally (`deno task
//     dev` in signaling-server/) and point at it with:
//       localStorage.setItem('kipukas_relay_url', 'ws://localhost:8787/ws')
//   - the settings page's switches (kipukas-server/src/settings.rs): that
//     the camera privacy notice has been accepted, and whether the QR
//     scanner shows what its frames take;
//   - what the QR scanner keeps as it was left: whether codes are found by
//     computer vision, whether the screen light is on, and which camera
//     the player chose (kipukas-server/src/routes/qr.rs).
const READ_AT_START = [
  'kipukas_relay_url',
  'qr-privacy-accepted',
  'kipukas-scanner-status',
  'kipukas-cv-enabled',
  'kipukas-scanner-light',
  'kipukas-scanner-camera',
];

/** What is stored under one of the engine's keys, or null. */
function stored(key) {
  try {
    return localStorage.getItem(key);
  } catch (_) {
    return null; // storage unavailable
  }
}

/** Everything stored for the engine. */
function allStored() {
  const all = {};
  for (const key of [...ENGINE_KEYS, ...READ_AT_START]) {
    const value = stored(key);
    if (value !== null) all[key] = value;
  }
  return all;
}

// Whether the player is in a room, as the engine last said. Toolbar tools
// read it from an Alpine store when they initialise, so they are right from
// the first render. Until the engine has started and said, a stored room is
// taken to mean yes.
//
// The listener must be registered before Alpine starts, which is why this
// script is loaded ahead of the Alpine bundle.
let roomSaid = null;

document.addEventListener('alpine:init', () => {
  globalThis.Alpine.store('room', {
    // This player is in a room (created or joined), opponent present or not.
    inRoom: roomSaid === null ? stored('kipukas_room') !== null : roomSaid,
  });
});

// Start the engine's worker (a module worker, so it can import)
const wasmWorker = new Worker('/assets/js/kipukas-worker.js', { type: 'module' });

// For kipukas-view.js, which asks the engine for what it draws, and
// qr-camera.js, which hands it camera frames.
globalThis.kipukasWorker = wasmWorker;

// The engine's first message, ahead of any request: what is stored. The
// worker handles messages in the order they are posted, so the engine is
// back as it was (in its room, with the room's document, and reconnecting)
// before anything is drawn.
wasmWorker.postMessage({ type: 'START', stored: allStored() });

/** Report something of this page or of storage to the engine. */
function report(what) {
  wasmWorker.postMessage({ type: 'PAGE', report: what });
}

wasmWorker.addEventListener('message', (event) => {
  const data = event.data;
  if (!data) return;

  if (data.type === 'SAVE') {
    // Storage keys to write (a string) or remove (null).
    for (const [key, value] of Object.entries(data.writes)) {
      try {
        if (value === null) localStorage.removeItem(key);
        else localStorage.setItem(key, value);
      } catch (_) { /* quota or storage unavailable */ }
    }
  } else if (data.type === 'ROOM') {
    // Whether the player is in a room, for the toolbar.
    roomSaid = data.inRoom;
    const store = globalThis.Alpine?.store('room');
    if (store) store.inRoom = roomSaid;
  } else if (data.type === 'GO') {
    // A path on this site, which the engine has checked (a scanned QR code).
    location.assign(data.path);
  } else if (data.type === 'RELOAD') {
    location.reload();
  } else if (data.type === 'DOWNLOAD') {
    // A file the engine has made for the player to keep. The browser saves
    // it where it saves downloads.
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([data.text], { type: 'application/json' }));
    link.download = data.name;
    link.click();
    URL.revokeObjectURL(link.href);
  }
});

// Another tab wrote or removed one of the engine's keys. What it holds now
// is read afresh: two tabs loading at once both write, and the last write is
// the one that counts.
globalThis.addEventListener('storage', (event) => {
  if (ENGINE_KEYS.includes(event.key)) {
    report({ event: 'storage', key: event.key, value: stored(event.key) });
  }
});

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') report({ event: 'shown' });
});

// A page restored from the back/forward cache saw nothing of storage while
// it was frozen: the engine is handed what is stored now.
globalThis.addEventListener('pageshow', (event) => {
  if (event.persisted) report({ event: 'restored', stored: allStored() });
});

// ============================================
// JXL DECODE WORKER POOL — parallel, off the engine's worker
// ============================================
// Grid thumbnails (and full card art) are JXL, decoded by WASM. A small pool of
// dedicated decode workers lets tiles decode in parallel instead of queueing
// behind the engine's requests in the single worker above. Round-robin dispatch.
const JXL_POOL_SIZE = Math.min(navigator.hardwareConcurrency || 2, 4);
const jxlPool = Array.from(
  { length: JXL_POOL_SIZE },
  () => new Worker('/assets/js/jxl-decode-worker.js', { type: 'module' }),
);
let jxlPoolNext = 0;
// Timing instrumentation: workers report per-decode ms; log for measurement.
for (const w of jxlPool) {
  w.addEventListener('message', (e) => {
    if (e.data?.type === 'JXL_TIMING') {
      console.log(
        `[jxl-pool] ${e.data.w}x${e.data.h} d=${e.data.maxDim} ${e.data.ms.toFixed(1)}ms`,
      );
    }
  });
}

// The service worker has fetched a card's art (.jxl) and needs it decoded.
// Hand it to the next pool worker in turn, with the service worker's port
// to answer on; the bytes and the port are transferred, not copied.
if (navigator.serviceWorker) {
  navigator.serviceWorker.addEventListener('message', (event) => {
    if (event.data?.type === 'WASM_DECODE_JXL') {
      const worker = jxlPool[jxlPoolNext];
      jxlPoolNext = (jxlPoolNext + 1) % jxlPool.length;
      worker.postMessage(
        { bytes: event.data.bytes, maxDim: event.data.maxDim || 0 },
        [event.data.bytes, ...event.ports],
      );
    }
  });
}

// Log worker errors for debugging
wasmWorker.onerror = (err) => {
  console.error('[kipukas-api] WASM Worker error:', err);
};
