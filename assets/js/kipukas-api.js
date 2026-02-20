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
window.kipukasWorker = wasmWorker;

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
  // Prefer query params already in the URL (htmx.ajax puts them there).
  // Fall back to serialized hx-include form parameters for attribute-driven requests.
  // Note: url.search includes '?' prefix, so strip it — the send line adds it back.
  const params = evt.detail.requestConfig.parameters;
  const hasFormParams = params && typeof params === 'object' && Object.keys(params).length > 0;
  const qs = url.search.slice(1) || (hasFormParams ? new URLSearchParams(params).toString() : '');

  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => {
    // Find the HTMX swap target: check evt.detail.target first (set by htmx.ajax),
    // then fall back to hx-target attribute lookup for attribute-driven requests.
    const targetEl = evt.detail.target
      || (() => {
        const sel = evt.detail.elt?.getAttribute('hx-target')
          || evt.detail.elt?.closest('[hx-target]')?.getAttribute('hx-target');
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
      method: evt.detail.requestConfig.verb?.toUpperCase() || 'GET',
      pathname: url.pathname,
      search: qs ? '?' + qs : '',
      body: '',
    },
    [channel.port2],
  );
});

// Log worker errors for debugging
wasmWorker.onerror = (err) => {
  console.error('[kipukas-api] WASM Worker error:', err);
};
