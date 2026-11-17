/**
 * hand-view.js — the /hand/ multi-card "hand" view.
 *
 * Reads a hand from the URL hash: concatenated 4-char card codes, e.g.
 * #KVphRrDC (two cards). Looks each up in /assets/data/hand-index.json, then
 * composes one full-bleed pane per card by cloning the #hand-pane-tpl template.
 *
 * This script only builds the panes and their art. Each pane's detail panel
 * is an engine-owned view (assets/js/kipukas-view.js): the game engine renders
 * the card, including its damage tracker, and redraws it when its state
 * changes. The same renderer serves every card type, and nothing has to be
 * fetched or initialised here.
 *
 * Layout is pure CSS scroll-snap (see .hand-pane in input.css): a multi-card
 * spread on desktop, a one-card swipe carousel on mobile.
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
    // The detail panel is the engine's to render. kipukas-view.js fills it as
    // soon as the pane is in the page.
    var slot = section.querySelector('[data-pane-content]');
    if (slot) {
      slot.setAttribute('data-engine-view', '/api/card/panel?card=' + encodeURIComponent(card.slug));
    }
    return section;
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
        spread.appendChild(buildPane(card, tpl));
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
