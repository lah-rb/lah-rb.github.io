/**
 * hand-view.js — the /hand/ multi-card "hand" view.
 *
 * Reads a hand from the URL hash: concatenated 4-char card codes, e.g.
 * #KVphRrDC (two cards). Looks each up in /assets/data/hand-index.json, then
 * composes one full-bleed pane per card by cloning the #hand-pane-tpl template
 * and injecting the card's pre-rendered detail fragment (/card_fragments/<slug>.html).
 *
 * Layout is pure CSS scroll-snap (see .hand-pane in input.css): a multi-card
 * spread on desktop, a one-card swipe carousel on mobile. The same fragment the
 * single card page renders is reused here, so all 7 card types just work.
 */
(function () {
  'use strict';

  var INDEX_URL = '/assets/data/hand-index.json';
  var indexPromise = null;

  function loadIndex() {
    if (!indexPromise) {
      indexPromise = fetch(INDEX_URL).then(function (r) {
        if (!r.ok) throw new Error('hand-index ' + r.status);
        return r.json();
      });
    }
    return indexPromise;
  }

  // "#KVphRrDC" (optional commas) → ["KVph", "RrDC"]. Fixed 4-char chunks.
  function parseHand(hash) {
    var raw = (hash || '').replace(/^#/, '').replace(/,/g, '').trim();
    var codes = [];
    for (var i = 0; i + 4 <= raw.length; i += 4) codes.push(raw.slice(i, i + 4));
    return codes;
  }

  function buildPane(card, tpl) {
    var section = tpl.content.cloneNode(true).querySelector('.hand-pane');
    var lqip = section.querySelector('[data-art-lqip]');
    var full = section.querySelector('[data-art-full]');
    lqip.src = '/assets/images/' + card.img_name + '?d=512';
    full.src = '/assets/images/' + card.img_name;
    return section;
  }

  // Activate injected Alpine (x-data) + HTMX (the keal tracker's hx-get) markup.
  function activate(el) {
    if (window.htmx) {
      try {
        window.htmx.process(el);
      } catch (e) { /* htmx not ready yet — harmless */ }
    }
    if (window.Alpine && window.Alpine.initTree) {
      try {
        window.Alpine.initTree(el);
      } catch (e) { /* Alpine guards double-init */ }
    }
  }

  function showEmpty(empty, spread) {
    empty.hidden = false;
    spread.hidden = true;
  }

  function render() {
    var spread = document.getElementById('hand-spread');
    var empty = document.getElementById('hand-empty');
    var tpl = document.getElementById('hand-pane-tpl');
    if (!spread || !empty || !tpl) return;

    var codes = parseHand(location.hash);
    spread.innerHTML = '';

    if (codes.length === 0) {
      showEmpty(empty, spread);
      return;
    }

    loadIndex().then(function (index) {
      var found = 0;
      codes.forEach(function (code) {
        var card = index[code];
        if (!card) return; // unknown code — skip
        found++;
        var pane = buildPane(card, tpl);
        spread.appendChild(pane);
        fetch('/card_fragments/' + card.slug + '.html')
          .then(function (r) { return r.ok ? r.text() : ''; })
          .then(function (html) {
            var slot = pane.querySelector('[data-pane-content]');
            if (slot && html) {
              slot.innerHTML = html;
              activate(slot);
            }
          })
          .catch(function () { /* leave the pane art-only on fetch failure */ });
      });

      if (found === 0) showEmpty(empty, spread);
      else {
        empty.hidden = true;
        spread.hidden = false;
      }
    }).catch(function () {
      showEmpty(empty, spread);
    });
  }

  function start() {
    render();
    // Lets later phases mutate the hand (add/remove) without a reload.
    window.addEventListener('hashchange', render);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start);
  } else {
    start();
  }
})();
