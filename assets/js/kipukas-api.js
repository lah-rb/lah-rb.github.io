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
// ROOM SESSION — how a multiplayer room is remembered between page loads
// ============================================
// Every page load starts a fresh game engine and a fresh socket, so the room
// has to be put back each time. It is kept in localStorage, which every tab
// of this browser shares, so a card opened in a new tab (a QR code scanned
// with the phone's camera app, for instance) is still in the room:
//   kipukas_room      { v, code, name, creator, playerId, peerId, ts } — which
//                     room this player is in, who they are in it, and who
//                     the opponent is (once known). Forgotten after 12 hours
//                     without use.
//   kipukas_room_doc  the shared room document (base64): timers and fists
//                     combat, saved after every change
//   kipukas_room_tab  the id of the tab that holds the room connection
// This file reads them on load (see "RESUME ROOM" below); kipukas-multiplayer.js
// writes the session when a room is created, joined or left.
const ROOM_KEY = 'kipukas_room';
const ROOM_DOC_KEY = 'kipukas_room_doc';
const ROOM_TAB_KEY = 'kipukas_room_tab';
const ROOM_TTL_MS = 12 * 60 * 60 * 1000;

/** Random hex string of `bytes` bytes. */
function randomHex(bytes) {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return Array.from(buf, (b) => b.toString(16).padStart(2, '0')).join('');
}

// Identifies this tab (this page load) among the tabs sharing the room.
const TAB_ID = randomHex(8);

globalThis.kipukasRoomSession = {
  /** The saved room, or null when this player is not in one. */
  load() {
    try {
      const raw = localStorage.getItem(ROOM_KEY);
      const session = raw ? JSON.parse(raw) : null;
      if (!session || !session.code) return null;
      if (!session.ts || Date.now() - session.ts > ROOM_TTL_MS) {
        this.clear(); // left over from an earlier day
        return null;
      }
      return session;
    } catch (_) {
      return null;
    }
  },
  /** Save the room, and make this tab the one that holds its connection. */
  save(session) {
    try {
      localStorage.setItem(ROOM_KEY, JSON.stringify({ ...session, v: 2, ts: Date.now() }));
    } catch (_) { /* storage unavailable */ }
    this.claim();
  },
  /** Forget the room and its shared document. */
  clear() {
    try {
      localStorage.removeItem(ROOM_KEY);
      localStorage.removeItem(ROOM_DOC_KEY);
      localStorage.removeItem(ROOM_TAB_KEY);
    } catch (_) { /* storage unavailable */ }
  },
  loadDoc() {
    try {
      return localStorage.getItem(ROOM_DOC_KEY) || '';
    } catch (_) {
      return '';
    }
  },
  saveDoc(state) {
    try {
      localStorage.setItem(ROOM_DOC_KEY, state);
    } catch (_) { /* quota or storage unavailable */ }
  },
  /** A fresh random id for this player in a room (24 hex characters). */
  newPlayerId() {
    return randomHex(12);
  },
  /**
   * Make this tab the one that holds the room connection. Other tabs see the
   * change (a `storage` event) and go quiet: the relay gives each player one
   * slot, so two tabs of the same player must not both connect.
   */
  claim() {
    try {
      localStorage.setItem(ROOM_TAB_KEY, TAB_ID);
    } catch (_) { /* storage unavailable */ }
  },
  /** Whether this tab holds the room connection (true if no tab has claimed it). */
  isOwner() {
    try {
      const owner = localStorage.getItem(ROOM_TAB_KEY);
      return !owner || owner === TAB_ID;
    } catch (_) {
      return true;
    }
  },
};

// ============================================
// ONE TAB AT A TIME
// ============================================
// Each tab runs its own game engine, loaded from storage when the page
// loaded. If another tab changes what is in storage (takes over the room,
// marks damage, changes a timer), this tab's engine is out of date. Rather
// than let it write stale state back, it reloads the next time it is shown.
let tabStale = false;

globalThis.addEventListener('storage', (event) => {
  if (event.key === ROOM_TAB_KEY) {
    // Read the current owner rather than trusting the event: two tabs loading
    // at once both write the key, and the last write is the one that counts.
    if (!globalThis.kipukasRoomSession.isOwner()) {
      tabStale = true;
      globalThis.dispatchEvent(new CustomEvent('kipukas-room-moved'));
    }
    return;
  }
  if (event.key === 'kipukas_player_doc' || event.key === ROOM_KEY || event.key === ROOM_DOC_KEY) {
    tabStale = true;
  }
});

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible' && tabStale) location.reload();
});

// A page restored from the back/forward cache received no storage events
// while it was frozen.
globalThis.addEventListener('pageshow', (event) => {
  if (event.persisted && (tabStale || !globalThis.kipukasRoomSession.isOwner())) {
    location.reload();
  }
});

// Sessions used to live in sessionStorage (one per tab). Tidy up the old keys.
try {
  sessionStorage.removeItem('kipukas_room');
  sessionStorage.removeItem('kipukas_crdt_state');
} catch (_) { /* storage unavailable */ }

// Room state for the UI, as an Alpine store. Toolbar tools read it when they
// initialise, so they are right from the first render instead of waiting for
// an event. kipukas-multiplayer.js keeps it current.
//
// This listener must be registered before Alpine starts, which is why this
// script is loaded ahead of the Alpine bundle.
document.addEventListener('alpine:init', () => {
  const inRoom = !!globalThis.kipukasRoomSession.load();
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
// PERSIST_STATE listener — auto-save PLAYER_DOC after every game mutation
// ============================================
// The WASM worker sends { type: 'PERSIST_STATE' } after every POST to /api/game/*.
// We fetch the full PLAYER_DOC as base64 from WASM and save to localStorage.
wasmWorker.addEventListener('message', (event) => {
  if (event.data?.type === 'PERSIST_STATE') {
    const ch = new MessageChannel();
    ch.port1.onmessage = (msg) => {
      if (msg.data.html) {
        localStorage.setItem('kipukas_player_doc', msg.data.html);
        // Phase E: broadcast to sync peer if a sync session is active
        if (globalThis.kipukasSync && kipukasSync.isConnected()) {
          kipukasSync.broadcastUpdate();
        }
      }
    };
    wasmWorker.postMessage(
      { method: 'GET', pathname: '/api/player/state', search: '', body: '' },
      [ch.port2],
    );
    persistRoomDoc();
  }
});

/** Save the shared room document, if this player is in a room. */
function persistRoomDoc() {
  const session = globalThis.kipukasRoomSession.load();
  if (!session) return;
  // Another tab holds the room now; its document is the current one.
  if (!globalThis.kipukasRoomSession.isOwner()) return;
  const ch = new MessageChannel();
  ch.port1.onmessage = (msg) => {
    const state = msg.data.html;
    // The room may have been left (or swapped) while this request was in
    // flight; a stale document must not be saved against another room.
    const current = globalThis.kipukasRoomSession.load();
    if (!state || !current || current.code !== session.code) return;
    globalThis.kipukasRoomSession.saveDoc(state);
  };
  wasmWorker.postMessage(
    { method: 'GET', pathname: '/api/room/yrs/state', search: '', body: '' },
    [ch.port2],
  );
}

// ============================================
// RESTORE PLAYER_DOC from localStorage on load
// ============================================
// Restore kipukas_player_doc (base64 yrs binary) if it exists.
// Messages are queued by the worker and gated on `await wasmReady`,
// so they are processed before any HTMX `hx-trigger="load"` requests.
(function restorePersistedState() {
  const playerDoc = localStorage.getItem('kipukas_player_doc');
  if (!playerDoc) return;

  console.log('[kipukas-api] Restoring PLAYER_DOC from localStorage');
  const ch = new MessageChannel();
  ch.port1.onmessage = (msg) => {
    if (msg.data.html === 'ok') {
      console.log('[kipukas-api] PLAYER_DOC restored from localStorage');
    } else {
      console.warn('[kipukas-api] PLAYER_DOC restore issue:', msg.data.html);
    }
  };
  wasmWorker.postMessage(
    { method: 'POST', pathname: '/api/player/restore', search: '', body: playerDoc },
    [ch.port2],
  );
})();

// ============================================
// RESUME ROOM — put the engine back in the room before the first render
// ============================================
// Posted straight after the player doc restore and before HTMX issues any
// `hx-trigger="load"` request (the worker handles requests in the order they
// are posted). The engine is therefore in room mode, with the shared document
// restored, by the time anything renders: timers and tools show shared state
// from the start, and edits made while the peer is still reconnecting go to
// the shared document rather than to a solo copy.
(function resumeRoom() {
  const session = globalThis.kipukasRoomSession.load();
  if (!session) return;

  // A session saved before player ids existed gets one now.
  if (!session.playerId) {
    session.playerId = globalThis.kipukasRoomSession.newPlayerId();
  }
  // Saving refreshes the session's age and makes this tab the one that holds
  // the room: any other tab of this player goes quiet.
  globalThis.kipukasRoomSession.save(session);

  console.log('[kipukas-api] Resuming room', session.code);
  const body = 'code=' + encodeURIComponent(session.code) +
    '&name=' + encodeURIComponent(session.name || '') +
    '&player=' + encodeURIComponent(session.playerId) +
    '&peer=' + encodeURIComponent(session.peerId || '') +
    '&state=' + globalThis.kipukasRoomSession.loadDoc();
  const ch = new MessageChannel();
  ch.port1.onmessage = (msg) => {
    if (msg.data.html !== 'ok') {
      console.warn('[kipukas-api] Room resume issue:', msg.data.html);
    }
  };
  wasmWorker.postMessage(
    { method: 'POST', pathname: '/api/room/resume', search: '', body },
    [ch.port2],
  );
})();

// ============================================
// AUTO-LOAD multiplayer module if room session exists
// ============================================
// kipukas-multiplayer.js is normally lazy-loaded when the user clicks
// the multiplayer button. But if there's a saved room session from a
// previous page, we need to eagerly load it so autoReconnect() fires
// and the WebSocket relay reconnects automatically on page navigation.
(function autoLoadMultiplayer() {
  if (!globalThis.kipukasRoomSession.load()) return;
  if (globalThis.kipukasMultiplayerLoaded) return;

  console.log('[kipukas-api] Saved room session found, auto-loading multiplayer module');
  import('/assets/js/kipukas-multiplayer.js')
    .then(function () {
      globalThis.kipukasMultiplayerLoaded = true;
      console.log('[kipukas-api] Multiplayer module auto-loaded for reconnect');
    })
    .catch(function (err) {
      console.warn('[kipukas-api] Failed to auto-load multiplayer:', err);
    });
})();

// ============================================
// QR detection stack is loaded on demand
// ============================================
// Previously preloaded 5s into every visit (wasteful for the majority who never
// scan). It's now warmed on the FIRST QR-button click — see window.kipukasLoadQR
// in _layouts/default.html — so it preps while the user reads the privacy modal.
