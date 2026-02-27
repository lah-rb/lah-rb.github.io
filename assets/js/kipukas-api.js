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

// Spawn the WASM Web Worker (module worker for ES import support)
const wasmWorker = new Worker('/assets/js/kipukas-worker.js', { type: 'module' });

// Expose worker for qr-camera.js and other modules that need direct access
globalThis.kipukasWorker = wasmWorker;

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
  }
});

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
// AUTO-LOAD multiplayer module if room session exists
// ============================================
// kipukas-multiplayer.js is normally lazy-loaded when the user clicks
// the multiplayer button. But if there's a saved room session from a
// previous page, we need to eagerly load it so autoReconnect() fires
// and the WebSocket relay reconnects automatically on page navigation.
(function autoLoadMultiplayer() {
  if (!sessionStorage.getItem('kipukas_room')) return;
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
