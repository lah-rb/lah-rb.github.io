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

// What the user just asked for, if anything: `{ action: 'create', name }` or
// `{ action: 'join', code, name }`, set only by createRoom() and joinRoom().
// While it is null, a connection is a *resume*: it asks the relay to `rejoin`
// and never re-initialises the engine's room, so shared timers and combat are
// left exactly as they were.
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
  url: relayAddress,
  // Keep retrying for as long as we are in a room. A create/join the user just
  // asked for is different: if the relay cannot be reached, say so and stop.
  shouldRetry: () => pendingAction === null && !!loadSession(),
  onMessage: handleSignalingMessage,
  onState: onLinkState,
  log: (...args) => console.log('[multiplayer]', ...args),
});

/**
 * The relay address for this connection attempt, saying what this player
 * wants: create a room, join one, or rejoin the one they are in.
 *
 * It goes in the address rather than in a first message because the relay
 * hands each connection to its room (one object per room code) before any
 * message is exchanged.
 */
function relayAddress() {
  const url = new URL(globalThis.kipukasRelayUrl());
  const session = loadSession();
  const want = pendingAction ||
    (session ? { action: 'rejoin', code: session.code, name: session.name || '' } : {});
  for (const [key, value] of Object.entries(want)) url.searchParams.set(key, value);
  url.searchParams.set('player', playerId);
  return url.toString();
}

/** Connect to the relay to carry out a create or join the user asked for. */
function connectFor(action) {
  // Stop first: stopping with an action pending reads as "could not connect".
  link.stop();
  pendingAction = action;
  link.start();
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

// This player opened the room in another tab (a card opened from the phone's
// camera app, for example). The relay gives each player one connection, so
// this tab lets go of it. kipukas-api.js reloads the tab the next time it is
// shown, at which point it takes the room back.
globalThis.addEventListener('kipukas-room-moved', goQuiet);

/**
 * This player's connection now belongs to another tab: stop, and do not
 * reconnect. Two tabs that both kept reconnecting would take the connection
 * from each other forever.
 */
function goQuiet() {
  if (!roomCode) return;
  console.log('[multiplayer] Room is now open in another tab; this tab goes quiet');
  peerPresent = false;
  clearTimeout(helloTimer);
  link.stop();
  setRoomStore({ link: 'moved', peer: false });
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
      // Both players are connected. Sent on every (re)connect of either.
      console.log('[multiplayer] Peer joined our room');
      peerPresent = true;
      if (msg.peerId) setPeerId(msg.peerId);
      onPeerConnected();
      break;

    case 'relay':
      // Game message relayed from the other peer through the server
      if (msg.data) {
        handleRelayedMessage(msg.data);
      }
      break;

    case 'peer_away':
      // The other player's connection closed (a page change, a locked
      // phone). They still have their place in the room.
      console.log('[multiplayer] Peer is away');
      markPeerGone();
      break;

    case 'peer_left':
      // The other player left the room for good.
      console.log('[multiplayer] Peer left the room');
      markPeerGone();
      break;

    case 'superseded':
      // This player connected again elsewhere (another tab that the storage
      // event did not reach in time). The relay is closing this socket.
      goQuiet();
      break;

    case 'error': {
      const text = msg.message || 'The relay refused the connection.';
      console.error('[multiplayer] Relay error:', msg.code, text);

      if (pendingAction) {
        // A create/join the user asked for was refused (wrong code or name,
        // room full). Clear the action first so stopping is not reported as
        // a connection failure.
        pendingAction = null;
        link.stop();
        showError(text);
        break;
      }

      // Refused while returning to a room: this player no longer has a place
      // in it (someone joined in their stead while they were away). Leave;
      // shared timers are copied back to this player's own timers by
      // /api/room/disconnect. Skip the status refresh so the explanation
      // below stays on screen.
      leaveRoom({ refreshStatus: false });
      setRoomStore({ link: 'expired' });
      showError(
        msg.code === 'full'
          ? 'Your place in this room was taken while you were away. Create or join a room to keep playing.'
          : text,
      );
      break;
    }
  }
}

/** Record who the opponent is. Only their entries count as the opponent's in combat. */
function setPeerId(id) {
  if (typeof id !== 'string' || !id || id === peerId) return;
  peerId = id;
  saveSession();
  postToWasmWithCallback('POST', '/api/room/peer', 'id=' + encodeURIComponent(peerId), (r) => {
    showFistsPhase(parseReply(r));
  });
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
      setPeerId(msg.playerId);
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

/**
 * The engine just marked damage on these cards (a lost round). Their damage
 * trackers are engine-owned views, so redraw them from the engine.
 */
function refreshDamage(cards) {
  if (!Array.isArray(cards) || !globalThis.kipukasViews) return;
  for (const slug of cards) globalThis.kipukasViews.refresh(slug);
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

    connectFor({ action: 'create', name: roomName });
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

    connectFor({ action: 'join', code, name });
  },

  /** Leave the room for good and clean up. */
  disconnect() {
    // Give up this player's place, so the opponent is told and the place is
    // free for someone else. (Being away keeps the place; leaving does not.)
    link.send({ type: 'leave' });
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
