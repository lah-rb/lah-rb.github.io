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
// Phase 3b: PERSIST_STATE listener — auto-save after every game mutation
// ============================================
// The WASM worker sends { type: 'PERSIST_STATE', json } after every
// POST to /api/game/*. We write directly to localStorage here on the
// main thread — no async round-trips, no unreliable beforeunload.
wasmWorker.addEventListener('message', (event) => {
  if (event.data?.type === 'PERSIST_STATE') {
    localStorage.setItem('kipukas_game_state', event.data.json);
  }
});

// ============================================
// Phase 3b: MIGRATE Alpine $persist data to WASM game state
// ============================================
// On first load after Phase 3b update, reads existing localStorage keys
// from Alpine's $persist plugin ({cardSlug}_damage, alarms, etc.) and
// imports them into the WASM game state via /api/game/import.
// After successful import, sets a flag to skip on subsequent loads.
(function migrateAlpineState() {
  if (localStorage.getItem('kipukas_state_migrated')) return;

  // Wait a tick for the DOM to settle, then attempt migration
  setTimeout(() => {
    const gameState = { cards: {}, alarms: [], show_alarms: true };
    let foundData = false;

    // Scan for Alpine $persist damage keys: _x_{cardSlug}_damage
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key) continue;

      // Match damage keys: _x_{slug}_damage
      const damageMatch = key.match(/^_x_(.+)_damage$/);
      if (damageMatch) {
        const slug = damageMatch[1];
        // Skip the global clearDamage token
        if (slug === 'clear') continue;

        try {
          const val = JSON.parse(localStorage.getItem(key));
          if (val && typeof val === 'object') {
            const slots = {};
            const wasted = !!val.wasted;
            // Convert numeric keys to slot map
            for (const [k, v] of Object.entries(val)) {
              if (k !== 'wasted') {
                const num = parseInt(k, 10);
                if (!isNaN(num)) {
                  slots[num] = !!v;
                }
              }
            }
            // Only import if there's actual damage state
            if (Object.values(slots).some((v) => v) || wasted) {
              gameState.cards[slug] = { slots, wasted };
              foundData = true;
            }
          }
        } catch (_e) {
          // Skip unparseable entries
        }
      }

      // Match alarm key: _x_alarms
      if (key === '_x_alarms') {
        try {
          const alarms = JSON.parse(localStorage.getItem(key));
          if (Array.isArray(alarms)) {
            gameState.alarms = alarms
              .filter((a) => typeof a === 'number' && a >= 0)
              .map((remaining) => ({ remaining }));
            if (gameState.alarms.length > 0) foundData = true;
          }
        } catch (_e) {
          // Skip
        }
      }

      // Match show alarms key: _x_showAlarms
      if (key === '_x_showAlarms') {
        try {
          const val = JSON.parse(localStorage.getItem(key));
          if (typeof val === 'boolean') {
            gameState.show_alarms = val;
          }
        } catch (_e) {
          // Skip
        }
      }
    }

    if (foundData) {
      console.log('[kipukas-api] Migrating Alpine $persist state to WASM:', gameState);
      const json = JSON.stringify(gameState);
      const channel = new MessageChannel();
      channel.port1.onmessage = (msg) => {
        if (msg.data.ok) {
          console.log('[kipukas-api] State migration complete');
          localStorage.setItem('kipukas_state_migrated', 'true');
        } else {
          console.warn('[kipukas-api] State migration failed:', msg.data.html);
        }
      };
      wasmWorker.postMessage(
        {
          method: 'POST',
          pathname: '/api/game/import',
          search: '',
          body: json,
        },
        [channel.port2],
      );
    } else {
      // No old data to migrate — mark as done
      localStorage.setItem('kipukas_state_migrated', 'true');
    }
  }, 500); // Small delay to let WASM worker initialize
})();

// ============================================
// Phase 3b: RESTORE game state from localStorage on load
// ============================================
// Send the import message immediately — the worker queues messages and
// gates on `await wasmReady`, so this will be processed before any
// HTMX `hx-trigger="load"` requests that arrive later.
(function restorePersistedState() {
  const saved = localStorage.getItem('kipukas_game_state');
  if (!saved) return;

  console.log('[kipukas-api] Restoring persisted game state from localStorage');
  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => {
    if (msg.data.ok) {
      console.log('[kipukas-api] Game state restored from localStorage');
    }
  };
  wasmWorker.postMessage(
    {
      method: 'POST',
      pathname: '/api/game/import',
      search: '',
      body: saved,
    },
    [channel.port2],
  );
})();
