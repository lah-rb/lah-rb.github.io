/**
 * kipukas-api.js — Page-level bridge between Service Worker and WASM Web Worker.
 *
 * Architecture (Option C — SW + Web Worker sidecar):
 *
 *   1. HTMX makes a fetch("/api/...")
 *   2. Service Worker intercepts it, creates a MessageChannel
 *   3. SW sends { type: 'WASM_REQUEST', ... } + port to this page
 *   4. This script relays the message + port to the WASM Web Worker
 *   5. Web Worker processes via WASM, responds on the port
 *   6. SW receives the response on its end of the channel
 *   7. SW returns a real Response to HTMX
 *
 * The MessageChannel port is transferred directly from SW → page → worker,
 * so the worker responds straight back to the SW with no extra hops.
 *
 * Phase 2 addition: Exposes the worker on window.kipukasWorker so that
 * qr-camera.js can send QR_FRAME messages directly (bypassing the SW
 * relay for the performance-critical scan loop).
 *
 * Load this script with defer in _layouts/default.html.
 */

// ============================================
// RELAY ADDRESS — single source of truth
// ============================================
// The multiplayer / device-sync relay address lives here because every page
// that can hold a room loads this file: the Jekyll layout and the standalone
// rules book. Moving the relay to another host is a change to this one line
// (and to the matching route in signaling-server/wrangler.jsonc).
//
// For local work, run the relay locally (`deno task dev` in signaling-server/)
// and point at it without editing code:
//   localStorage.setItem('kipukas_relay_url', 'ws://localhost:8787/ws')
const KIPUKAS_RELAY_URL = 'wss://signal.kipukas.us/ws';

globalThis.kipukasRelayUrl = function () {
  try {
    const override = localStorage.getItem('kipukas_relay_url');
    if (override && /^wss?:\/\//.test(override)) return override;
  } catch (_) { /* storage unavailable — use the default */ }
  return KIPUKAS_RELAY_URL;
};

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

/** What is stored under one of the engine's keys, or null. */
function stored(key) {
  try {
    return localStorage.getItem(key);
  } catch (_) {
    return null; // storage unavailable
  }
}

/** Everything stored under the engine's keys. */
function allStored() {
  const all = {};
  for (const key of ENGINE_KEYS) {
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
  const inRoom = roomSaid === null ? stored('kipukas_room') !== null : roomSaid;
  globalThis.Alpine.store('room', {
    inRoom, // this player is in a room (created or joined), peer present or not
    // idle | connecting | open | waiting | expired | moved (to another tab)
    link: inRoom ? 'connecting' : 'idle',
    peer: false, // the other player is present
    outdated: false, // the other player is on an older copy of the site
  });
});

// Spawn the WASM Web Worker (module worker for ES import support)
const wasmWorker = new Worker('/assets/js/kipukas-worker.js', { type: 'module' });

// Expose worker for qr-camera.js and other modules that need direct access
globalThis.kipukasWorker = wasmWorker;

// The engine's first message, ahead of any request: what is stored. The
// worker handles messages in the order they are posted, so the engine is
// back as it was (in its room, with the room's document) before anything is
// drawn.
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
    // A device paired for syncing is sent the player's new state.
    if ('kipukas_player_doc' in data.writes && globalThis.kipukasSync?.isConnected()) {
      globalThis.kipukasSync.broadcastUpdate();
    }
  } else if (data.type === 'ROOM') {
    // The room the player is in (null: none). The connection script reads
    // it when it loads, which it does as soon as there is a room to return
    // to: the engine is already back in it, and only the socket is missing.
    globalThis.kipukasRoom = data.room;
    roomSaid = data.room !== null;
    const store = globalThis.Alpine?.store('room');
    if (store) store.inRoom = roomSaid;
    if (roomSaid) {
      import('/assets/js/kipukas-multiplayer.js').catch((err) => {
        console.warn('[kipukas-api] Failed to load the connection script:', err);
      });
    }
  } else if (data.type === 'QUIET') {
    globalThis.dispatchEvent(new CustomEvent('kipukas-room-moved'));
  } else if (data.type === 'RELOAD') {
    location.reload();
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
// JXL DECODE WORKER POOL — parallel, off the /api worker
// ============================================
// Grid thumbnails (and full card art) are JXL, decoded by WASM. A small pool of
// dedicated decode workers lets tiles decode in parallel instead of serializing
// behind /api calls in the single worker above. Round-robin dispatch.
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
      console.log(`[jxl-pool] ${e.data.w}x${e.data.h} d=${e.data.maxDim} ${e.data.ms.toFixed(1)}ms`);
    }
  });
}

// Listen for messages from the controlling Service Worker
if (navigator.serviceWorker) {
  navigator.serviceWorker.addEventListener('message', (event) => {
    if (event.data?.type === 'WASM_REQUEST') {
      // Transfer the MessageChannel port directly to the Web Worker.
      // The worker will respond on this port, which the SW is listening on.
      wasmWorker.postMessage(
        {
          method: event.data.method,
          pathname: event.data.pathname,
          search: event.data.search,
          body: event.data.body || '',
        },
        event.ports, // Transfer all ports (the SW sent one)
      );
    } else if (event.data?.type === 'WASM_DECODE_JXL') {
      // SW fetched the whole .jxl and needs it decoded. Dispatch to the next
      // pool worker (round-robin); forward the buffer + the SW's response port,
      // transferring both so the bytes travel zero-copy.
      const worker = jxlPool[jxlPoolNext];
      jxlPoolNext = (jxlPoolNext + 1) % jxlPool.length;
      worker.postMessage(
        { bytes: event.data.bytes, maxDim: event.data.maxDim || 0 },
        [event.data.bytes, ...event.ports],
      );
    }
  });
}

// ============================================
// FALLBACK: Direct main-thread routing when no SW is active
// ============================================
// During development (jekyll serve) or on first page load before the SW
// claims the page, /api/* fetches would 404 on the network.
// This intercepts HTMX requests and routes them directly through the
// Web Worker, bypassing the SW relay entirely.
document.addEventListener('htmx:beforeRequest', (evt) => {
  const path = evt.detail.requestConfig.path;
  if (!path.startsWith('/api/')) return; // Not an API route — let HTMX fetch normally

  // If the SW is controlling this page, let the full relay chain handle it
  if (navigator.serviceWorker?.controller) return;

  // No SW — handle it directly via the Web Worker
  evt.preventDefault();
  console.log('[kipukas-api] No SW controller, routing directly:', path);

  const url = new URL(path, location.origin);
  const verb = (evt.detail.requestConfig.verb || 'GET').toUpperCase();
  const params = evt.detail.requestConfig.parameters;
  const hasFormParams = params && typeof params === 'object' && Object.keys(params).length > 0;

  // Phase 3b: For POST requests, serialize params as form body instead of query string.
  // For GET requests, prefer query params already in the URL (htmx.ajax puts them there),
  // fall back to serialized hx-include form parameters for attribute-driven requests.
  let qs = '';
  let body = '';
  if (verb === 'POST' || verb === 'PUT' || verb === 'PATCH') {
    // POST body: serialize params as URL-encoded form data
    body = hasFormParams ? new URLSearchParams(params).toString() : '';
    qs = url.search.slice(1);
  } else {
    qs = url.search.slice(1) || (hasFormParams ? new URLSearchParams(params).toString() : '');
  }

  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => {
    // Find the HTMX swap target: check evt.detail.target first (set by htmx.ajax),
    // then fall back to hx-target attribute lookup for attribute-driven requests.
    const targetEl = evt.detail.target ||
      (() => {
        const sel = evt.detail.elt?.getAttribute('hx-target') ||
          evt.detail.elt?.closest('[hx-target]')?.getAttribute('hx-target');
        return sel ? document.querySelector(sel) : null;
      })();
    if (targetEl) {
      targetEl.innerHTML = msg.data.html;
      // Let HTMX know we've settled this element (processes hx-* attributes)
      if (typeof htmx !== 'undefined') htmx.process(targetEl);
      // Execute inline <script> tags (innerHTML doesn't run them automatically)
      targetEl.querySelectorAll('script').forEach((old) => {
        const s = document.createElement('script');
        s.textContent = old.textContent;
        old.parentNode.replaceChild(s, old);
      });
    }
  };

  wasmWorker.postMessage(
    {
      method: verb,
      pathname: url.pathname,
      search: qs ? '?' + qs : '',
      body: body,
    },
    [channel.port2],
  );
});

// Log worker errors for debugging
wasmWorker.onerror = (err) => {
  console.error('[kipukas-api] WASM Worker error:', err);
};

// ============================================
// QR detection stack is loaded on demand
// ============================================
// Previously preloaded 5s into every visit (wasteful for the majority who never
// scan). It's now warmed on the FIRST QR-button click — see window.kipukasLoadQR
// in _layouts/default.html — so it preps while the user reads the privacy modal.
