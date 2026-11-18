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
  // The engine draws the fists dialog again: whose entries count as the
  // opponent's has changed.
  postToWasm('POST', '/api/room/peer', 'id=' + encodeURIComponent(peerId));
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
  clearTimeout(helloTimer);
  link.stop();
  setRoomStore({ inRoom: false, peer: false, link: 'idle', outdated: false });
  // The engine copies the room's timers back to this player's own, and has
  // the turn tracker drawn again to show them.
  postToWasm('POST', '/api/room/disconnect', '');
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
      // The engine applies it, and what follows is the engine's to ask for:
      // the views that show the room are drawn again (kipukas-view.js), and
      // anything to send back comes as ROOM_UPDATES, below.
      console.log('[multiplayer] Received yrs update from peer');
      postToWasmWithCallback('POST', '/api/room/yrs/apply', 'update=' + msg.update, (reply) => {
        if (reply !== 'ok') console.warn('[multiplayer] Could not apply peer update:', reply);
      });
      break;
    }

    default:
      console.log('[multiplayer] Unknown relayed message:', msg);
  }
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

// A change this player makes to the room's shared state through the engine
// (a turn timer advanced, a choice locked in for a round of fists) is queued
// by the engine for the other player, and the engine's worker hands it over
// after the post. Send it on. One that cannot be sent just now is made up by
// the exchange of state when the players are next connected.
if (globalThis.kipukasWorker) {
  globalThis.kipukasWorker.addEventListener('message', (event) => {
    if (!event.data || event.data.type !== 'ROOM_UPDATES') return;
    for (const update of event.data.updates) sendToPeer({ type: 'yrs_update', update });
  });
}

/** Refresh the room status panel. */
function refreshRoomStatus() {
  if (typeof htmx === 'undefined') return;
  setTimeout(() => {
    const status = document.getElementById('room-status');
    if (status) {
      htmx.ajax('GET', '/api/room/status', { target: '#room-status', swap: 'innerHTML' });
    }
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

  /** Check if currently connected to a peer. */
  isConnected() {
    return peerPresent && link.state() === 'open';
  },
};

globalThis.kipukasMultiplayer = kipukasMultiplayer;
console.log('[multiplayer] Kipukas multiplayer module loaded (WebSocket relay)');
