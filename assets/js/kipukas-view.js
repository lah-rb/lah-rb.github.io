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
 * A form can instead be sent as it is typed in, a moment after the typing
 * pauses, and is then not cleared:
 *
 *   <form data-engine-post="/api/cards" data-engine-live="300">
 *     <input name="search">
 *   </form>
 *
 * A form need not be inside a view. One that is not has no view of its own
 * to draw again, only those of the route it posts to.
 *
 * Other views of the route that was posted to are drawn again too: a new
 * timer is posted to /api/game/turns from the form, and the list of timers,
 * a view of /api/game/turns, shows it.
 *
 * The engine can ask to be sent a change later, without a tap:
 *
 *   <span hidden data-engine-post="/api/game/turns"
 *         data-engine-body="action=expire" data-engine-after="15000"></span>
 *
 * This is how something happens by the clock, such as a finished timer
 * fading. The engine reads the clock and says how long until its picture
 * next changes; the page only waits that long and asks. Each drawing of the
 * view replaces what was waiting with what the new drawing asks for.
 *
 * Or to be sent one when something scrolls into view, which is how a list
 * grows as it is read:
 *
 *   <div data-engine-post="/api/cards" data-engine-body="shown=12"
 *        data-engine-seen></div>
 *
 * A view's route may ask for something only the page knows, written {name}:
 *
 *   <div data-engine-view="/api/hand?codes={hash}"></div>
 *
 * See PAGE_VALUES. The view is drawn again when that changes.
 *
 * What a view renders may hold other views (a hand holds each card's panel).
 * An inner view is its own: the outer one's later drawings leave it alone.
 * So is any element the engine marks data-engine-keep, for what the page has
 * changed since and the engine cannot know, such as a picture that has faded
 * in once loaded. Kept things are still removed when the outer view no
 * longer renders them, so give what holds them an id if there can be several.
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
  const AFTER = 'data-engine-after';
  const KEEP = 'data-engine-keep';
  const SEEN = 'data-engine-seen';
  const LIVE = 'data-engine-live';

  /** How much of an element must be in view for it to count as seen. */
  const SEEN_SHARE = 0.3;

  /**
   * What the page knows and the engine does not. `{hash}` in a view's route
   * is replaced by the value each time the view is drawn. Each is given the
   * view.
   */
  const PAGE_VALUES = {
    /** What follows the # in the page's address: on the hand page, its cards. */
    hash: () => location.hash.slice(1),

    /**
     * How many columns the view's own grid has just now, if it is laid out
     * as one: the binder's batches are whole rows.
     */
    columns: (view) => {
      if (!view) return '';
      const tracks = getComputedStyle(view).gridTemplateColumns.split(/\s+/);
      return tracks.filter((track) => track.endsWith('px')).length || '';
    },
  };

  /** A route with its {name} places filled in, for this view. */
  function fill(route, view) {
    return (route || '').replace(
      /\{(\w+)\}/g,
      (whole, name) => (PAGE_VALUES[name] ? encodeURIComponent(PAGE_VALUES[name](view)) : whole),
    );
  }

  /** The shortest wait before a post the engine asked for is sent. */
  const SOONEST_MS = 50;

  /** Views already seen, so a view moved in the DOM is not mounted twice. */
  const mounted = new WeakSet();

  /** Views that were displayed when last looked at. */
  const shown = new WeakSet();

  /** For each view, the waits now running for posts the engine asked for. */
  const waiting = new WeakMap();

  /** Elements whose post, sent on being seen, is on its way. */
  const sending = new WeakSet();

  /** For each form sent as it is typed in, the pause now being waited out. */
  const pausing = new WeakMap();

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

  /**
   * Make the view's contents match `html`, changing only what differs. A view
   * inside this one, and anything marked to be kept, is left as it is.
   */
  function patch(view, html) {
    if (globalThis.Idiomorph) {
      globalThis.Idiomorph.morph(view, html, {
        morphStyle: 'innerHTML',
        callbacks: {
          beforeNodeMorphed: (node) =>
            node === view || node.nodeType !== 1 ||
            !(node.hasAttribute(KEEP) || node.hasAttribute(VIEW)),
        },
      });
    } else {
      view.innerHTML = html; // correct, just not gentle
    }
  }

  /**
   * Start the waits a view's contents ask for (data-engine-after), in place
   * of any it had running. When one is up its post is sent as a tap's would
   * be, which draws the view again and so starts the next.
   */
  function schedule(view) {
    for (const timer of waiting.get(view) || []) clearTimeout(timer);
    const timers = [];
    for (const asker of view.querySelectorAll('[' + AFTER + '][' + POST + ']')) {
      const wait = Math.max(SOONEST_MS, Number(asker.getAttribute(AFTER)) || 0);
      timers.push(setTimeout(() => {
        if (asker.isConnected) send(view, asker.getAttribute(POST), asker.getAttribute(BODY));
      }, wait));
    }
    waiting.set(view, timers);
  }

  // Sends the post of an element marked data-engine-seen when enough of it
  // has scrolled into view.
  const lookout = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      const asker = entry.target;
      if (!entry.isIntersecting || sending.has(asker) || !asker.isConnected) continue;
      const view = asker.closest('[' + VIEW + ']');
      if (!view) continue;
      sending.add(asker);
      send(view, asker.getAttribute(POST), asker.getAttribute(BODY))
        .finally(() => sending.delete(asker));
    }
  }, { threshold: SEEN_SHARE });

  /**
   * Look out for the elements in a view that ask to be sent when seen. Each
   * is looked at afresh after every drawing, so one that is still in view
   * (the new batch did not push it out) is sent again.
   */
  function watch(view) {
    for (const asker of view.querySelectorAll('[' + SEEN + '][' + POST + ']')) {
      lookout.unobserve(asker);
      lookout.observe(asker);
    }
  }

  /** Draw a view from its route. */
  async function render(view) {
    const route = view.getAttribute(VIEW);
    if (!route) return;
    const html = await ask('GET', fill(route, view), '');
    // The view may have been removed while the engine was answering.
    if (!view.isConnected) return;
    patch(view, html);
    schedule(view);
    watch(view);
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
      return new URL(fill(view.getAttribute(VIEW), view), location.origin).searchParams.get('card');
    } catch (_) {
      return null;
    }
  }

  /**
   * Ask the engine for a change, then draw again what may show it: the view
   * the change was asked from, if it was asked from one, and every other
   * displayed view of the route that was posted to. The engine handles
   * requests in the order they are sent, so quick taps cannot overtake one
   * another.
   */
  async function send(view, route, body) {
    await ask('POST', route, body);
    const path = pathOf(route);
    const others = allViews().filter((other) =>
      other !== view && isDisplayed(other) && pathOf(other.getAttribute(VIEW)) === path
    );
    await Promise.all((view ? [view, ...others] : others).map(render));
  }

  /** Send a form: its body, then its fields. */
  function sendForm(form) {
    clearTimeout(pausing.get(form));
    const fields = new URLSearchParams(new FormData(form)).toString();
    const body = [form.getAttribute(BODY), fields].filter(Boolean).join('&');
    return send(form.closest('[' + VIEW + ']'), form.getAttribute(POST), body);
  }

  // A tap on a control inside a view.
  document.addEventListener('click', (event) => {
    const control = event.target.closest ? event.target.closest('[' + POST + ']') : null;
    // A form is sent when it is submitted, not when it is tapped.
    if (!control || control.tagName === 'FORM') return;
    const view = control.closest('[' + VIEW + ']');
    if (!view) return;
    event.preventDefault();
    send(view, control.getAttribute(POST), control.getAttribute(BODY));
  });

  // A form submitted by its button or the Enter key. One that is sent as it
  // is typed in is sent at once, and keeps what is typed.
  document.addEventListener('submit', async (event) => {
    const form = event.target;
    if (!form.hasAttribute || !form.hasAttribute(POST)) return;
    event.preventDefault();
    await sendForm(form);
    if (form.isConnected && !form.hasAttribute(LIVE)) form.reset();
  });

  // Typing in a form that is sent as it is typed in: wait for a pause, then
  // send it.
  document.addEventListener('input', (event) => {
    const form = event.target.closest
      ? event.target.closest('form[' + LIVE + '][' + POST + ']')
      : null;
    if (!form) return;
    clearTimeout(pausing.get(form));
    const pause = Number(form.getAttribute(LIVE)) || 0;
    pausing.set(form, setTimeout(() => sendForm(form), pause));
  });

  // Views can arrive after load: the hand view injects card panels. And a
  // view's contents can be put there by other code (the room's timers are, by
  // the multiplayer script): what such contents ask to be sent later is
  // started here, as it would be after a drawing.
  new MutationObserver((records) => {
    const asked = new Set();
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (node.nodeType !== 1) continue;
        mount(node);
        if (node.hasAttribute(AFTER) || node.querySelector('[' + AFTER + ']')) {
          const view = node.closest('[' + VIEW + ']');
          if (view) asked.add(view);
        }
      }
    }
    asked.forEach(schedule);
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

    /**
     * Ask the engine for a change from outside a view, and draw again the
     * views of the route it was posted to.
     */
    send(route, body) {
      return send(null, route, body);
    },
  };

  // The address's codes changed: draw again the views that are given them.
  globalThis.addEventListener('hashchange', () => {
    for (const view of allViews()) {
      if (view.getAttribute(VIEW).includes('{hash}') && isDisplayed(view)) render(view);
    }
  });

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => mount(document));
  } else {
    mount(document);
  }
})();
