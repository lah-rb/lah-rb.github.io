/**
 * kipukas-multiplayer.js — WebSocket relay multiplayer manager for Kipukas.
 *
 * Game messages are forwarded between the two players by the relay without
 * inspection — game logic stays 100% client-side in WASM.
 *
 * The socket itself is owned by kipukas-link.js (one socket, one retry timer,
 * never gives up while a room session exists). This module decides what to
 * send when a socket opens and handles what arrives.
 *
 * The room session is persisted so the connection survives page navigation
 * (multi-page Jekyll site). On each page load the module rejoins the relay
 * room if a session exists.
 *
 * Exposed globally as window.kipukasMultiplayer for use by WASM-returned
 * HTML onclick handlers.
 */

import { createLink } from './kipukas-link.js';

let roomCode = '';
let roomName = '';
let isCreator = false;
let peerPresent = false; // Whether the other peer is in the room

// Who this player and the opponent are in the room. This player's id is
// chosen when the room is created or joined; it keys their entries in the
// room document (fists combat). The opponent's id arrives in their hello.
let playerId = '';
let peerId = '';

// Both players' pages must speak the same room-document format. Each says
// hello with its protocol version when the opponent appears; an opponent who
// never does is running an older copy of the site.
const PROTOCOL = 2;
const HELLO_TIMEOUT_MS = 3000;
let helloTimer = null;

// The fists phase (form | waiting | result | outcome) the dialog last showed,
// so it is only redrawn when the phase really changes.
let fistsPhase = null;

// What the user just asked for, if anything: set only by createRoom() and
// joinRoom(). While it is null, a socket that opens is a *resume*: we send
// `rejoin` and never re-initialise the engine's room, so shared timers and
// combat are left exactly as they were.
let pendingAction = null;

// ── Session + UI state ─────────────────────────────────────────────

// Storage is owned by kipukas-api.js (globalThis.kipukasRoomSession), which
// also restores the room in the engine on page load and saves the shared
// document after every change.
const roomSession = globalThis.kipukasRoomSession;

/** Save the current room session. */
function saveSession() {
  if (!roomCode) return;
  roomSession.save({
    code: roomCode,
    name: roomName,
    creator: isCreator,
    playerId,
    peerId,
  });
  setRoomStore({ inRoom: true });
}

/** Load the saved room session (null if none). */
function loadSession() {
  return roomSession.load();
}

/** Update the Alpine store the toolbar tools and status pill read. */
function setRoomStore(patch) {
  const alpine = globalThis.Alpine;
  if (!alpine || typeof alpine.store !== 'function') return;
  const store = alpine.store('room');
  if (store) Object.assign(store, patch);
}

// ── Relay connection ───────────────────────────────────────────────

const link = createLink({
  url: () => globalThis.kipukasRelayUrl(),
  // Keep retrying for as long as we are in a room. A create/join the user just
  // asked for is different: if the relay cannot be reached, say so and stop.
  shouldRetry: () => pendingAction === null && !!loadSession(),
  onOpen: onLinkOpen,
  onMessage: handleSignalingMessage,
  onState: onLinkState,
  log: (...args) => console.log('[multiplayer]', ...args),
});

/** A socket opened: say who we are. */
function onLinkOpen() {
  if (pendingAction) {
    link.send(pendingAction);
    return;
  }
  const session = loadSession();
  if (session && session.code) {
    // Resume. `rejoin` skips name validation and cancels the relay's grace timer.
    link.send({ type: 'rejoin', code: session.code, name: session.name || '' });
  }
}

/** The link changed state (idle / connecting / open / waiting). */
function onLinkState(next, prev) {
  console.log('[multiplayer] Link:', prev, '→', next);
  setRoomStore({ link: next });

  // Socket lost while in a room: the peer cannot reach us until we are back.
  if (prev === 'open' && next !== 'open' && peerPresent) {
    markPeerGone();
  }

  // A create/join the user asked for never reached the relay.
  if (next === 'idle' && pendingAction) {
    pendingAction = null;
    showError('Could not connect to signaling server. Try again.');
  }
}

/** The peer is no longer reachable (their socket or ours is gone). */
function markPeerGone() {
  peerPresent = false;
  clearTimeout(helloTimer);
  setRoomStore({ peer: false });
  postToWasm('POST', '/api/room/peer_left', '');
  refreshRoomStatus();
}

// Anything that suggests the network is back: retry now rather than waiting
// out the backoff (phone woke up, tab came to the front, page was restored
// from the back/forward cache).
globalThis.addEventListener('online', () => link.kick());
globalThis.addEventListener('pageshow', (event) => {
  if (event.persisted) link.kick();
});
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') link.kick();
});

/** Handle messages from the signaling server. */
function handleSignalingMessage(msg) {
  console.log('[multiplayer] ← signaling msg:', msg.type);
  switch (msg.type) {
    case 'room_created':
      roomCode = msg.code;
      roomName = msg.name || '';
      console.log('[multiplayer] Room created:', roomCode);
      saveSession();
      link.settled();
      // Only a create the user asked for initialises the engine's room.
      if (pendingAction) {
        postToWasm(
          'POST',
          '/api/room/create',
          `code=${roomCode}&name=${encodeURIComponent(roomName)}&player=${playerId}`,
        );
      }
      pendingAction = null;
      refreshRoomStatus();
      break;

    case 'room_joined':
      roomCode = msg.code;
      roomName = msg.name || roomName;
      console.log('[multiplayer] Joined room:', roomCode);
      saveSession();
      link.settled();
      // Only a join the user asked for initialises the engine's room. A resume
      // (page load or reconnect) must not: /api/room/join starts a fresh shared
      // document and re-seeds it, which duplicated timers and wiped combat.
      if (pendingAction) {
        postToWasm(
          'POST',
          '/api/room/join',
          `code=${roomCode}&name=${encodeURIComponent(roomName)}&player=${playerId}`,
        );
      }
      pendingAction = null;
      refreshRoomStatus();
      break;

    case 'peer_joined':
      console.log('[multiplayer] Peer joined our room');
      peerPresent = true;
      onPeerConnected();
      break;

    case 'relay':
      // Game message relayed from the other peer through the server
      if (msg.data) {
        handleRelayedMessage(msg.data);
      }
      break;

    case 'peer_left':
      console.log('[multiplayer] Peer disconnected');
      markPeerGone();
      break;

    case 'error': {
      const text = msg.message || '';
      console.error('[multiplayer] Server error:', text);
      const resuming = pendingAction === null;

      if (resuming && text.includes('full')) {
        // The relay still counts our previous socket (it has not seen it close
        // yet). Not a dead end: try again shortly.
        console.log('[multiplayer] Room full on rejoin, retrying');
        link.bounce();
        break;
      }

      if (resuming && text.includes('not found')) {
        // The relay no longer has this room. Leave it: shared timers are
        // copied back to this player's own timers by /api/room/disconnect.
        // Skip the status refresh so the explanation below stays on screen.
        leaveRoom({ refreshStatus: false });
        setRoomStore({ link: 'expired' });
        showError('This room has expired. Create a new room to keep playing.');
        break;
      }

      // A create/join the user asked for was refused.
      pendingAction = null;
      if (!loadSession()) link.stop();
      showError(text);
      break;
    }
  }
}

/** Leave the room locally: forget the session and return the engine to solo mode. */
function leaveRoom({ refreshStatus = true } = {}) {
  roomSession.clear();
  peerPresent = false;
  pendingAction = null;
  roomCode = '';
  roomName = '';
  playerId = '';
  peerId = '';
  fistsPhase = null;
  clearTimeout(helloTimer);
  link.stop();
  setRoomStore({ inRoom: false, peer: false, link: 'idle', outdated: false });
  // POST disconnect runs export_to_local() in WASM, copying shared CRDT
  // alarms back to the local player document. After it completes, refresh
  // the alarm display in local mode so exported timers appear.
  postToWasmWithCallback('POST', '/api/room/disconnect', '', () => {
    refreshAlarms();
  });
  if (refreshStatus) refreshRoomStatus();
}

/** Called when both peers are in the room and can exchange messages. */
function onPeerConnected() {
  console.log('[multiplayer] Peer connected via WebSocket relay!');
  postToWasm(
    'POST',
    '/api/room/connected',
    `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
  );
  setRoomStore({ peer: true });
  refreshRoomStatus();

  // Say who we are and which room-document format we speak. An opponent on
  // an older copy of the site never answers; tell this player after a moment.
  sendToPeer({ type: 'hello', proto: PROTOCOL, playerId });
  clearTimeout(helloTimer);
  helloTimer = setTimeout(() => {
    console.warn('[multiplayer] No hello from opponent: they need to update');
    setRoomStore({ outdated: true });
  }, HELLO_TIMEOUT_MS);

  refreshAlarms();

  // Exchange state vectors so both room documents converge after every
  // (re)connect. This is what repairs anything missed while either player
  // was away: timers and fists combat both live in the room document.
  initiateYrsSync();
}

/** Yrs CRDT sync handshake: send our state vector to the peer. */
function initiateYrsSync() {
  wasmRequest('GET', '/api/room/yrs/sv', '', '', (sv) => {
    if (sv) {
      sendToPeer({ type: 'yrs_sv', sv });
      console.log('[multiplayer] Sent yrs state vector to peer');
    }
  });
}

/** Send a game message to the peer via the WebSocket relay. */
function sendToPeer(data) {
  if (!link.send({ type: 'relay', data })) {
    console.warn('[multiplayer] WebSocket not open, cannot relay message');
  }
}

// ── Relayed Message Protocol ───────────────────────────────────────

/** Handle game messages received via the WebSocket relay from the peer. */
function handleRelayedMessage(msg) {
  switch (msg.type) {
    case 'hello': {
      // The opponent's id and protocol version.
      clearTimeout(helloTimer);
      const compatible = msg.proto === PROTOCOL;
      setRoomStore({ outdated: !compatible });
      if (!compatible) {
        console.warn('[multiplayer] Opponent speaks protocol', msg.proto, 'we speak', PROTOCOL);
      }
      if (typeof msg.playerId === 'string' && msg.playerId && msg.playerId !== peerId) {
        peerId = msg.playerId;
        saveSession();
        postToWasmWithCallback(
          'POST',
          '/api/room/peer',
          'id=' + encodeURIComponent(peerId),
          (r) => {
            showFistsPhase(parseReply(r));
          },
        );
      }
      break;
    }

    case 'yrs_sv': {
      // Peer sent their state vector — compute our diff and send it back,
      // then send our own state vector so they can compute their diff for us.
      console.log('[multiplayer] Received yrs state vector from peer');
      postToWasmWithCallback('POST', '/api/room/yrs/diff', 'sv=' + msg.sv, (diff) => {
        if (diff && !diff.startsWith('{')) {
          sendToPeer({ type: 'yrs_update', update: diff });
          console.log('[multiplayer] Sent yrs diff to peer');
        }
      });
      // Send our state vector back so peer can compute their diff for us
      wasmRequest('GET', '/api/room/yrs/sv', '', '', (sv) => {
        if (sv) {
          sendToPeer({ type: 'yrs_sv_reply', sv });
        }
      });
      break;
    }

    case 'yrs_sv_reply': {
      // Peer replied with their state vector (response to our initial sv).
      // Compute and send our diff.
      console.log('[multiplayer] Received yrs state vector reply from peer');
      postToWasmWithCallback('POST', '/api/room/yrs/diff', 'sv=' + msg.sv, (diff) => {
        if (diff && !diff.startsWith('{')) {
          sendToPeer({ type: 'yrs_update', update: diff });
          console.log('[multiplayer] Sent yrs diff reply to peer');
        }
      });
      break;
    }

    case 'yrs_update': {
      // Peer sent a room-document update (timers, fists combat, or both).
      console.log('[multiplayer] Received yrs update from peer');
      postToWasmWithCallback('POST', '/api/room/yrs/apply', 'update=' + msg.update, (response) => {
        const reply = parseReply(response);
        if (!reply || reply.error) {
          console.warn('[multiplayer] Could not apply peer update:', reply && reply.error);
          return;
        }
        const alarms = document.getElementById('turn-alarms');
        if (alarms && reply.alarms != null) {
          alarms.innerHTML = reply.alarms;
          if (typeof htmx !== 'undefined') htmx.process(alarms);
        }
        // Applying a finished round records that it was applied: send that back.
        if (reply.update) sendToPeer({ type: 'yrs_update', update: reply.update });
        refreshDamage(reply.damage);
        showFistsPhase(reply);
      });
      break;
    }

    default:
      console.log('[multiplayer] Unknown relayed message:', msg);
  }
}

/** Show a message in the dedicated fists message area. */
function showFistsMessage(text, colorClass) {
  const msgEl = document.getElementById('fists-message');
  if (msgEl) {
    msgEl.innerHTML = `<p class="text-sm ${colorClass || 'text-kip-drk-sienna'}">${text}</p>`;
  }
}

/** Parse a JSON reply from the engine (null if it is not JSON). */
function parseReply(text) {
  try {
    return JSON.parse(text);
  } catch (_) {
    return null;
  }
}

/** Re-fetch the fists dialog contents for this page's card. */
function refreshFists() {
  const fists = document.getElementById('fists-container');
  if (!fists || typeof htmx === 'undefined') return;
  const url = fists.getAttribute('hx-get') || '/api/room/fists';
  htmx.ajax('GET', url, { target: '#fists-container', swap: 'innerHTML' });
}

/**
 * Put a fists reply's HTML into the dialog, relay its room-document update
 * to the opponent, and refresh any damage tracker it changed.
 */
function applyFistsReply(response, { render = true } = {}) {
  const reply = parseReply(response);
  if (!reply) return null;
  if (render && reply.html != null) {
    const container = document.getElementById('fists-container');
    if (container) {
      container.innerHTML = reply.html;
      if (typeof htmx !== 'undefined') htmx.process(container);
    }
  }
  if (reply.update) sendToPeer({ type: 'yrs_update', update: reply.update });
  refreshDamage(reply.damage);
  if (reply.phase) fistsPhase = reply.phase;
  return reply;
}

/**
 * The room document changed under us (the opponent did something). Redraw
 * the fists dialog only if this player's phase changed: redrawing the form
 * while they are choosing would wipe their selection.
 */
function showFistsPhase(reply) {
  if (!reply || !reply.phase) return;
  const changed = reply.phase !== fistsPhase;
  // First news after a page load: the dialog already drew itself from the
  // restored document, so only redraw if it is past the form.
  const firstNews = fistsPhase === null;
  fistsPhase = reply.phase;
  if (changed && !(firstNews && reply.phase === 'form')) {
    refreshFists();
  } else if (reply.phase === 'form' && reply.remote_ready) {
    showFistsMessage('\u2713 Opponent has locked in their choice!', 'text-emerald-600');
  }
}

/** Refresh the damage trackers of the cards the engine just marked. */
function refreshDamage(cards) {
  if (!Array.isArray(cards) || cards.length === 0) return;
  setTimeout(() => cards.forEach((slug) => refreshKealTracker(slug)), 150);
}

// ── WASM helpers ───────────────────────────────────────────────────

/** POST to WASM route via the worker (fire-and-forget for state updates). */
function postToWasm(method, path, body) {
  if (!globalThis.kipukasWorker) return;
  const channel = new MessageChannel();
  channel.port1.onmessage = () => {}; // discard response
  globalThis.kipukasWorker.postMessage(
    { method, pathname: path, search: '', body },
    [channel.port2],
  );
}

/** POST to WASM route and call back with the HTML response. */
function postToWasmWithCallback(method, path, body, callback) {
  if (!globalThis.kipukasWorker) return;
  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => {
    if (callback) callback(msg.data.html);
  };
  globalThis.kipukasWorker.postMessage(
    { method, pathname: path, search: '', body },
    [channel.port2],
  );
}

/** Send a request to WASM worker with query string support and call back with the HTML response. */
function wasmRequest(method, path, search, body, callback) {
  if (!globalThis.kipukasWorker) return;
  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => {
    if (callback) callback(msg.data.html);
  };
  globalThis.kipukasWorker.postMessage(
    { method, pathname: path, search: search || '', body: body || '' },
    [channel.port2],
  );
}

/** Refresh the keal damage tracker of one card, if it is on this page.
 *  After the engine marks damage (a lost round), the checkboxes showing
 *  that card are stale. A page can show several trackers (the hand view),
 *  so the card is named rather than taking the first tracker found.
 *
 *  Strategy: update the existing Alpine scope's reactive properties
 *  directly via Alpine.$data() instead of replacing innerHTML and
 *  re-initializing Alpine. This avoids a cross-browser bug where
 *  Alpine.initTree() fails to discover new x-data scopes after
 *  innerHTML replacement on Firefox, Trivalent, DuckDuckGo, and
 *  Cromite (Chrome derivatives). Alpine reactive assignments trigger
 *  all :class bindings automatically — no DOM replacement needed.
 *
 *  Falls back to innerHTML replacement only if the Alpine scope is
 *  not yet initialized (e.g., HTMX initial load hasn't completed). */
function refreshKealTracker(slug) {
  // The keal damage tracker has id="keal-damage-{slug}"
  const tracker = slug ? document.getElementById('keal-damage-' + slug) : null;
  if (!tracker) {
    console.log('[multiplayer] No keal damage tracker on this page for:', slug);
    // Still persist: the card may be open on another page later.
    persistState();
    return;
  }
  console.log('[multiplayer] Refreshing keal damage tracker for:', slug);

  // Try to find the existing Alpine scope (the x-data div inside the tracker)
  const alpineEl = tracker.querySelector('[x-data]');
  if (alpineEl && typeof Alpine !== 'undefined') {
    // Preferred path: fetch JSON state and update Alpine scope directly.
    // No innerHTML swap, no initTree — works identically on all browsers.
    wasmRequest('GET', '/api/game/damage/state', '?card=' + slug, '', (json) => {
      try {
        const state = JSON.parse(json);
        if (state.slots) {
          const scope = Alpine.$data(alpineEl);
          // Update each slot — Alpine reactively updates :class bindings
          for (const [k, v] of Object.entries(state.slots)) {
            scope.slots[parseInt(k, 10)] = v;
          }
          scope.wasted = !!state.wasted;
          console.log('[multiplayer] Keal tracker updated via Alpine scope for:', slug);
        }
      } catch (e) {
        console.warn('[multiplayer] Alpine scope update failed, falling back to innerHTML:', e);
        refreshKealTrackerFallback(tracker, slug);
      }
      persistState();
    });
  } else {
    // Fallback: Alpine scope not yet initialized (initial HTMX load
    // hasn't completed). Use innerHTML replacement.
    console.log('[multiplayer] No Alpine scope found, using innerHTML fallback for:', slug);
    refreshKealTrackerFallback(tracker, slug);
    persistState();
  }
}

/** Fallback: replace tracker innerHTML and re-initialize Alpine.
 *  Used only when the Alpine scope hasn't been created yet. */
function refreshKealTrackerFallback(tracker, slug) {
  wasmRequest('GET', '/api/game/damage', '?card=' + slug, '', (html) => {
    if (html) {
      tracker.innerHTML = html;
      if (typeof htmx !== 'undefined') {
        tracker.querySelectorAll('[hx-get],[hx-post]').forEach((el) => htmx.process(el));
      }
      if (typeof Alpine !== 'undefined') {
        Alpine.initTree(tracker);
      }
      console.log('[multiplayer] Keal damage tracker refreshed (fallback) for:', slug);
    }
  });
}

/** Persist WASM game state to localStorage (PLAYER_DOC base64). */
function persistState() {
  wasmRequest('GET', '/api/player/state', '', '', (b64) => {
    if (b64) {
      try {
        localStorage.setItem('kipukas_player_doc', b64);
        console.log('[multiplayer] Player doc persisted after damage change');
      } catch (e) {
        console.warn('[multiplayer] Failed to persist player doc:', e);
      }
    }
  });
}

/** Refresh the alarm display in #turn-alarms.
 *  Rust auto-detects multiplayer via is_peer_connected() and reads from
 *  CRDT Doc (multiplayer) or PLAYER_DOC (local) accordingly. */
function refreshAlarms() {
  wasmRequest('GET', '/api/game/turns', '?display=alarms', '', (html) => {
    const alarms = document.getElementById('turn-alarms');
    if (alarms && html != null) {
      alarms.innerHTML = html;
      if (typeof htmx !== 'undefined') htmx.process(alarms);
    }
  });
}

/** Refresh the room status panel and fists section in the UI. */
function refreshRoomStatus() {
  if (typeof htmx === 'undefined') return;
  setTimeout(() => {
    const status = document.getElementById('room-status');
    if (status) {
      htmx.ajax('GET', '/api/room/status', { target: '#room-status', swap: 'innerHTML' });
    }
    refreshFists();
  }, 100);
}

/** Show an error message in the room status panel. */
function showError(message) {
  const target = document.getElementById('room-status');
  if (target) {
    target.innerHTML = '<div class="p-4"><span class="text-kip-red text-sm">' + message +
      '</span></div>';
  }
}

// ── Resume on page load ────────────────────────────────────────────

/** Rejoin the relay room from a saved session (after page navigation). */
function resume() {
  const session = loadSession();
  if (!session) return;

  console.log('[multiplayer] Found saved session, rejoining room:', session.code);
  roomCode = session.code;
  roomName = session.name || '';
  isCreator = session.creator || false;
  playerId = session.playerId || '';
  peerId = session.peerId || '';

  // The engine was already put back in this room, with the shared document
  // restored, by kipukas-api.js (POST /api/room/resume) before this module
  // loaded. All that is left is the socket.
  link.start();
}

// Kick off the resume when the script loads (deferred, so DOM is ready)
resume();

// ── Public API (exposed on window.kipukasMultiplayer) ───────────────

const kipukasMultiplayer = {
  /** Create a new room. Reads name from #room-name-input. */
  createRoom() {
    const nameInput = document.getElementById('room-name-input');
    roomName = nameInput ? nameInput.value.trim() : '';
    isCreator = true;
    playerId = roomSession.newPlayerId();
    peerId = '';

    pendingAction = { type: 'create', name: roomName };
    if (link.state() === 'open') link.send(pendingAction);
    else link.start();
  },

  /** Join an existing room. Reads code and name from inputs. */
  joinRoom() {
    const codeInput = document.getElementById('room-code-input');
    const nameInput = document.getElementById('room-name-join-input');
    const code = codeInput ? codeInput.value.trim().toUpperCase() : '';
    const name = nameInput ? nameInput.value.trim() : '';
    if (code.length !== 4) {
      showError('Please enter a 4-character room code.');
      return;
    }
    isCreator = false;
    roomName = name;
    playerId = roomSession.newPlayerId();
    peerId = '';

    pendingAction = { type: 'join', code, name };
    if (link.state() === 'open') link.send(pendingAction);
    else link.start();
  },

  /** Disconnect from room and clean up. */
  disconnect() {
    leaveRoom();
  },

  /** Submit fists combat choice. Called from WASM-rendered HTML button. */
  submitFists(cardSlug) {
    const roleEl = document.querySelector('input[name="fists-role"]:checked');
    const kealEl = document.querySelector('input[name="fists-keal"]:checked');

    if (!roleEl) {
      showFistsMessage('Please select Attacking or Defending.', 'text-kip-red');
      return;
    }
    if (!kealEl) {
      showFistsMessage('Please select a Keal Means.', 'text-kip-red');
      return;
    }

    const role = roleEl.value;
    const keal = kealEl.value;

    // The engine writes the choice into the room document and returns the
    // update to relay (include today for loyalty tracking).
    const today = new Date().toISOString().slice(0, 10);
    const body = `role=${role}&card=${cardSlug}&keal=${keal}&today=${today}`;
    postToWasmWithCallback('POST', '/api/room/fists', body, applyFistsReply);
  },

  /** Submit Final Blows for a card with exhausted keal means. */
  submitFinalBlows(cardSlug) {
    const today = new Date().toISOString().slice(0, 10);
    const body = `card=${cardSlug}&today=${today}`;
    postToWasmWithCallback('POST', '/api/room/fists/final', body, applyFistsReply);
  },

  /** Start a new round for both players (New Round, Try Again, Cancel round). */
  resetFists() {
    postToWasmWithCallback('POST', '/api/room/fists/reset', '', (response) => {
      applyFistsReply(response);
      refreshFists();
    });
  },

  /**
   * The fists dialog was closed. This does NOT cancel a round in progress:
   * the round stays in the room document and reopening the dialog shows
   * where it stands. Only a finished round gives way to a new one.
   */
  closeFists() {
    postToWasmWithCallback('POST', '/api/room/fists/close', '', (response) => {
      applyFistsReply(response, { render: false });
    });
  },

  /** Report combat outcome ("Did you win?" answer). */
  reportOutcome(won) {
    // The engine records the answer in the room document (that is how the
    // opponent learns of it) and marks this player's damage if they lost.
    postToWasmWithCallback('POST', '/api/room/fists/outcome', 'won=' + won, applyFistsReply);
  },

  // ── Turn timer sync (yrs CRDT) ─────────────────────────────────────

  /** Add a turn timer via yrs CRDT and sync update to peer. */
  addTurn(turns, name, colorSet) {
    const t = parseInt(turns, 10) || 1;
    const clamped = Math.max(1, Math.min(99, t));
    const n = (name || '').trim();
    const c = colorSet || 'red';

    // Mutate the yrs Doc — returns JSON { update, html }
    postToWasmWithCallback(
      'POST',
      '/api/room/yrs/alarm/add',
      'turns=' + clamped + '&name=' + encodeURIComponent(n) + '&color_set=' + c,
      (response) => {
        try {
          const result = JSON.parse(response);
          // Swap the alarm list HTML (use != null so empty string still clears DOM)
          const alarms = document.getElementById('turn-alarms');
          if (alarms && result.html != null) {
            alarms.innerHTML = result.html;
            if (typeof htmx !== 'undefined') htmx.process(alarms);
          }
          // Relay the yrs update to peer
          if (result.update) {
            sendToPeer({ type: 'yrs_update', update: result.update });
          }
        } catch (e) {
          console.warn('[multiplayer] addTurn parse error:', e);
        }
      },
    );
    console.log('[multiplayer] Added turn via yrs CRDT: ' + clamped + ' cycles');
  },

  /** Advance all turn timers by one diel cycle via yrs CRDT. */
  tickTurns() {
    postToWasmWithCallback('POST', '/api/room/yrs/alarm/tick', '', (response) => {
      try {
        const result = JSON.parse(response);
        const alarms = document.getElementById('turn-alarms');
        // Use != null so empty string (all alarms removed) still clears DOM
        if (alarms && result.html != null) {
          alarms.innerHTML = result.html;
          if (typeof htmx !== 'undefined') htmx.process(alarms);
        }
        if (result.update) {
          sendToPeer({ type: 'yrs_update', update: result.update });
        }
      } catch (e) {
        console.warn('[multiplayer] tickTurns parse error:', e);
      }
    });
    console.log('[multiplayer] Ticked turns via yrs CRDT');
  },

  /** Remove a shared turn timer by its id via yrs CRDT. */
  removeTurn(id) {
    postToWasmWithCallback(
      'POST',
      '/api/room/yrs/alarm/remove',
      'id=' + encodeURIComponent(id),
      (response) => {
        try {
          const result = JSON.parse(response);
          const alarms = document.getElementById('turn-alarms');
          // Use != null so empty string (all alarms removed) still clears DOM
          if (alarms && result.html != null) {
            alarms.innerHTML = result.html;
            if (typeof htmx !== 'undefined') htmx.process(alarms);
          }
          if (result.update) {
            sendToPeer({ type: 'yrs_update', update: result.update });
          }
        } catch (e) {
          console.warn('[multiplayer] removeTurn parse error:', e);
        }
      },
    );
    console.log('[multiplayer] Removed turn via yrs CRDT, id=' + id);
  },

  /** Check if currently connected to a peer. */
  isConnected() {
    return peerPresent && link.state() === 'open';
  },
};

globalThis.kipukasMultiplayer = kipukasMultiplayer;
console.log('[multiplayer] Kipukas multiplayer module loaded (WebSocket relay)');
