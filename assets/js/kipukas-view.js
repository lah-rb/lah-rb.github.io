/**
 * kipukas-view.js — engine-owned views.
 *
 * A view is a part of the page whose contents the game engine renders and
 * owns. The page keeps no copy of its state:
 *
 *   <div data-engine-view="/api/game/damage?card=brox_the_defiant"></div>
 *
 * The attribute is the engine route that renders the view. This script draws
 * the view from that route when it is shown, and again whenever its state may
 * have changed.
 *
 * A view that is not displayed (a closed dialog) is not drawn. It is drawn
 * each time it comes to be displayed, so a dialog opens on what is true now,
 * and nothing is asked of the engine for a tool nobody opens.
 *
 * Inside a view, a control names the change a tap asks for:
 *
 *   <button type="button" data-engine-post="/api/game/damage"
 *           data-engine-body="card=brox_the_defiant&slot=2">
 *
 * A tap sends that to the engine. A form does the same when it is submitted,
 * and sends its fields after its body:
 *
 *   <form data-engine-post="/api/game/turns" data-engine-body="action=add">
 *     <input name="turns" type="number"> <button>New Timer</button>
 *   </form>
 *
 * The engine applies the change, and the view is drawn again from its route
 * and patched into the page: only what differs is touched, so elements keep
 * their identity (focus, transitions, anything else attached to them).
 * Nothing is toggled on the page ahead of the engine, so the page cannot
 * disagree with it. A submitted form is cleared.
 *
 * Other views of the route that was posted to are drawn again too: a new
 * timer is posted to /api/game/turns from the form, and the list of timers,
 * a view of /api/game/turns, shows it.
 *
 * A route or a body may ask for something only the page knows, written
 * {name}. See PAGE_VALUES.
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

  /**
   * What the page knows and the engine does not. `{today}` in a view's route
   * or in a control's route or body is replaced by the value when the request
   * is made.
   */
  const PAGE_VALUES = {
    /** Today's date, YYYY-MM-DD (UTC). */
    today: () => new Date().toISOString().slice(0, 10),
  };

  /** A route or body with its {name} places filled in. */
  function fill(text) {
    return (text || '').replace(
      /\{(\w+)\}/g,
      (whole, name) => (PAGE_VALUES[name] ? encodeURIComponent(PAGE_VALUES[name]()) : whole),
    );
  }

  /** Views already seen, so a view moved in the DOM is not mounted twice. */
  const mounted = new WeakSet();

  /** Views that were displayed when last looked at. */
  const shown = new WeakSet();

  /** Whether an element is displayed: it, and everything it is in, has a box. */
  function isDisplayed(element) {
    return element.isConnected && element.getClientRects().length > 0;
  }

  function allViews() {
    return Array.from(document.querySelectorAll('[' + VIEW + ']'));
  }

  /** The path of a route, without its query. */
  function pathOf(route) {
    try {
      return new URL(fill(route), location.origin).pathname;
    } catch (_) {
      return '';
    }
  }

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

  /** Draw a view from its route. */
  async function render(view) {
    const route = view.getAttribute(VIEW);
    if (!route) return;
    const html = await ask('GET', fill(route), '');
    // The view may have been removed while the engine was answering.
    if (view.isConnected) patch(view, html);
  }

  // Draws a view when it comes to be displayed. The margin is far larger than
  // any page, so this is about being displayed at all (a dialog opening), not
  // about being scrolled into the window.
  const watcher = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      const view = entry.target;
      if (!entry.isIntersecting) {
        shown.delete(view);
      } else if (!shown.has(view)) {
        shown.add(view);
        render(view);
      }
    }
  }, { rootMargin: '100000px' });

  /** Start looking after any views at or under `root` not seen before. */
  function mount(root) {
    const views = [];
    if (root.nodeType === 1 && root.hasAttribute(VIEW)) views.push(root);
    if (root.querySelectorAll) views.push(...root.querySelectorAll('[' + VIEW + ']'));
    for (const view of views) {
      if (mounted.has(view)) continue;
      mounted.add(view);
      // Displayed already: draw now, not a frame later when the watcher
      // first reports. Otherwise the watcher draws it when it is shown.
      if (isDisplayed(view)) {
        shown.add(view);
        render(view);
      }
      watcher.observe(view);
    }
  }

  /** The card a view shows, from its route (`?card=<slug>`), or null. */
  function cardOf(view) {
    try {
      return new URL(fill(view.getAttribute(VIEW)), location.origin).searchParams.get('card');
    } catch (_) {
      return null;
    }
  }

  /**
   * Ask the engine for a change from inside `view`, then draw again what may
   * show it: the view itself, and every other displayed view of the route
   * that was posted to. The engine handles requests in the order they are
   * sent, so quick taps cannot overtake one another.
   */
  async function send(view, route, body) {
    route = fill(route);
    await ask('POST', route, body);
    const path = pathOf(route);
    const others = allViews().filter((other) =>
      other !== view && isDisplayed(other) && pathOf(other.getAttribute(VIEW)) === path
    );
    await Promise.all([view, ...others].map(render));
  }

  // A tap on a control inside a view.
  document.addEventListener('click', (event) => {
    const control = event.target.closest ? event.target.closest('[' + POST + ']') : null;
    // A form is sent when it is submitted, not when it is tapped.
    if (!control || control.tagName === 'FORM') return;
    const view = control.closest('[' + VIEW + ']');
    if (!view) return;
    event.preventDefault();
    send(view, control.getAttribute(POST), fill(control.getAttribute(BODY)));
  });

  // A form inside a view, submitted by its button or the Enter key: its body,
  // then its fields.
  document.addEventListener('submit', async (event) => {
    const form = event.target;
    if (!form.hasAttribute || !form.hasAttribute(POST)) return;
    const view = form.closest('[' + VIEW + ']');
    if (!view) return;
    event.preventDefault();
    const fields = new URLSearchParams(new FormData(form)).toString();
    const body = [fill(form.getAttribute(BODY)), fields].filter(Boolean).join('&');
    await send(view, form.getAttribute(POST), body);
    if (form.isConnected) form.reset();
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
     * Draw again from the engine. With a card slug, only that card's views;
     * without one, every view on the page. Views that are not displayed are
     * left for when they are shown.
     */
    refresh(card) {
      const views = allViews().filter((view) =>
        isDisplayed(view) && (!card || cardOf(view) === card)
      );
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
