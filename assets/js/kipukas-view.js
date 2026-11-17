/**
 * kipukas-view.js — engine-owned views.
 *
 * A view is a part of the page whose contents the game engine renders and
 * owns. The page keeps no copy of its state:
 *
 *   <div data-engine-view="/api/game/damage?card=brox_the_defiant"></div>
 *
 * The attribute is the engine route that renders the view. This script fills
 * the view from that route when it appears, and again whenever its state may
 * have changed.
 *
 * Inside a view, a control names the change a tap asks for:
 *
 *   <button data-engine-post="/api/game/damage"
 *           data-engine-body="card=brox_the_defiant&slot=2">
 *
 * A tap sends that to the engine. The engine applies it, the view is rendered
 * again from its route, and the result is patched into the page: only what
 * differs is touched, so elements keep their identity (focus, transitions,
 * anything else attached to them). Nothing is toggled on the page ahead of
 * the engine, so the page cannot disagree with it.
 *
 * When the engine changes state for another reason (a lost round of fists
 * marks damage, New Game clears it), call kipukasViews.refresh(card).
 *
 * Requests go straight to the engine's worker (set up by kipukas-api.js),
 * not through fetch and the service worker: a tap is answered in the time it
 * takes two messages to cross a thread boundary.
 */
(function () {
  const VIEW = 'data-engine-view';
  const POST = 'data-engine-post';
  const BODY = 'data-engine-body';

  /** Views already filled once, so a view moved in the DOM is not re-mounted. */
  const mounted = new WeakSet();

  /** Ask the engine. Resolves with the response text. */
  function ask(method, route, body) {
    return new Promise((resolve) => {
      const url = new URL(route, location.origin);
      const channel = new MessageChannel();
      channel.port1.onmessage = (msg) => resolve(msg.data && msg.data.html ? msg.data.html : '');
      globalThis.kipukasWorker.postMessage(
        { method, pathname: url.pathname, search: url.search, body: body || '' },
        [channel.port2],
      );
    });
  }

  /** Make the view's contents match `html`, changing only what differs. */
  function patch(view, html) {
    if (globalThis.Idiomorph) {
      globalThis.Idiomorph.morph(view, html, { morphStyle: 'innerHTML' });
    } else {
      view.innerHTML = html; // correct, just not gentle
    }
  }

  /** Render a view from its route. */
  async function render(view) {
    const route = view.getAttribute(VIEW);
    if (!route) return;
    const html = await ask('GET', route, '');
    // The view may have been removed while the engine was answering.
    if (view.isConnected) patch(view, html);
  }

  /** Fill any views at or under `root` that have not been filled yet. */
  function mount(root) {
    const views = [];
    if (root.nodeType === 1 && root.hasAttribute(VIEW)) views.push(root);
    if (root.querySelectorAll) views.push(...root.querySelectorAll('[' + VIEW + ']'));
    for (const view of views) {
      if (mounted.has(view)) continue;
      mounted.add(view);
      render(view);
    }
  }

  /** The card a view shows, from its route (`?card=<slug>`), or null. */
  function cardOf(view) {
    try {
      return new URL(view.getAttribute(VIEW), location.origin).searchParams.get('card');
    } catch (_) {
      return null;
    }
  }

  // A tap on a control inside a view: ask the engine, then redraw that view.
  // The engine handles requests in the order they are sent, so quick taps
  // cannot overtake one another.
  document.addEventListener('click', async (event) => {
    const control = event.target.closest ? event.target.closest('[' + POST + ']') : null;
    if (!control) return;
    const view = control.closest('[' + VIEW + ']');
    if (!view) return;
    event.preventDefault();
    await ask('POST', control.getAttribute(POST), control.getAttribute(BODY));
    await render(view);
  });

  // Views can arrive after load: the hand view injects card panels.
  new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (node.nodeType === 1) mount(node);
      }
    }
  }).observe(document.documentElement, { childList: true, subtree: true });

  globalThis.kipukasViews = {
    /**
     * Redraw from the engine. With a card slug, only that card's views;
     * without one, every view on the page.
     */
    refresh(card) {
      const all = Array.from(document.querySelectorAll('[' + VIEW + ']'));
      const views = card ? all.filter((view) => cardOf(view) === card) : all;
      return Promise.all(views.map(render));
    },

    /** Ask the engine for a change from outside a view. Resolves with its reply. */
    post(route, body) {
      return ask('POST', route, body);
    },
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => mount(document));
  } else {
    mount(document);
  }
})();
