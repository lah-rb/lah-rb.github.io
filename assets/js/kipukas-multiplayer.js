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
  roomSession.save({ code: roomCode, name: roomName, creator: isCreator });
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
          `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
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
          `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
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
  link.stop();
  setRoomStore({ inRoom: false, peer: false, link: 'idle' });
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

  // Switch alarm display to multiplayer mode (reads from CRDT Doc,
  // shows sync buttons). Seeded alarms from local GameState are
  // already in the Doc via seed_from_local() on room create/join.
  refreshAlarms();

  // Initiate yrs CRDT sync handshake — exchange state vectors so both
  // peers converge on the same Doc state after (re)connect.
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
    case 'fists_submission': {
      // Remote peer sent their fists combat choice.
      const json = JSON.stringify(msg.data);
      console.log('[multiplayer] Received fists submission:', json);

      // Store in WASM
      postToWasmWithCallback('POST', '/api/room/fists/sync', json, (html) => {
        // Check if the response contains a result page (regular or Final Blows)
        if (html && html.includes('reportOutcome')) {
          const container = document.getElementById('fists-container');
          if (container) {
            container.innerHTML = html;
            if (typeof htmx !== 'undefined') htmx.process(container);
            execScripts(container);
          }
        } else {
          // Local hasn't submitted yet — just show a message
          showFistsMessage('\u2713 Opponent has locked in their choice!', 'text-emerald-600');
        }
      });
      break;
    }

    case 'fists_reset': {
      console.log('[multiplayer] Remote peer reset fists');
      postToWasmWithCallback('POST', '/api/room/fists/reset', '', (html) => {
        const container = document.getElementById('fists-container');
        if (container) {
          container.innerHTML = html;
          if (typeof htmx !== 'undefined') htmx.process(container);
        }
        refreshRoomStatus();
      });
      break;
    }

    case 'fists_outcome': {
      const attackerWon = msg.attacker_won;
      console.log('[multiplayer] Received fists outcome: attacker_won=' + attackerWon);
      postToWasmWithCallback(
        'POST',
        '/api/room/fists/outcome',
        'attacker_won=' + (attackerWon ? 'true' : 'false'),
        (html) => {
          const container = document.getElementById('fists-container');
          if (container) {
            container.innerHTML = html;
            if (typeof htmx !== 'undefined') htmx.process(container);
            execScripts(container);
          }
          setTimeout(refreshKealTracker, 150);
        },
      );
      break;
    }

    case 'final_blows_submission': {
      const json = JSON.stringify(msg.data);
      console.log('[multiplayer] Received Final Blows submission:', json);

      postToWasmWithCallback('POST', '/api/room/fists/final/sync', json, (html) => {
        if (html && html.includes('reportOutcome')) {
          const container = document.getElementById('fists-container');
          if (container) {
            container.innerHTML = html;
            if (typeof htmx !== 'undefined') htmx.process(container);
            execScripts(container);
          }
        } else {
          showFistsMessage('\u2713 Opponent stated Final Blows!', 'text-emerald-600');
        }
      });
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
      // Peer sent a yrs binary update — apply it to our local Doc.
      console.log('[multiplayer] Received yrs update from peer');
      postToWasmWithCallback('POST', '/api/room/yrs/apply', 'update=' + msg.update, (html) => {
        // html is the refreshed alarm list from the yrs Doc
        const alarms = document.getElementById('turn-alarms');
        if (alarms && html) {
          alarms.innerHTML = html;
          if (typeof htmx !== 'undefined') htmx.process(alarms);
        }
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

/** Refresh the keal damage tracker on the card page behind the modal.
 *  After auto-mark damage changes WASM state, the checkboxes on the
 *  card page are stale.
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
function refreshKealTracker() {
  // The keal damage tracker has id="keal-damage-{slug}"
  const tracker = document.querySelector('[id^="keal-damage-"]');
  if (!tracker) {
    console.log('[multiplayer] No keal damage tracker in DOM, skipping refresh');
    // Still persist even if tracker isn't in the DOM (e.g. on a different page)
    persistState();
    return;
  }
  const slug = tracker.id.replace('keal-damage-', '');
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

/** Re-execute inline scripts after innerHTML swap. */
function execScripts(el) {
  el.querySelectorAll('script').forEach((old) => {
    const s = document.createElement('script');
    s.textContent = old.textContent;
    old.parentNode.replaceChild(s, old);
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
    const fists = document.getElementById('fists-container');
    if (fists) {
      const url = fists.getAttribute('hx-get') || '/api/room/fists';
      htmx.ajax('GET', url, { target: '#fists-container', swap: 'innerHTML' });
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

    // POST to WASM to store local submission (include today for loyalty tracking)
    const today = new Date().toISOString().slice(0, 10);
    const body = `role=${role}&card=${cardSlug}&keal=${keal}&today=${today}`;
    postToWasmWithCallback('POST', '/api/room/fists', body, (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
        execScripts(container);
      }
    });
  },

  /** Send fists data to peer. Called by inline script from WASM response. */
  sendFists(submissionData) {
    sendToPeer({ type: 'fists_submission', data: submissionData });
    console.log('[multiplayer] Sent fists submission to peer');
  },

  /** Submit Final Blows for a card with exhausted keal means. */
  submitFinalBlows(cardSlug) {
    const today = new Date().toISOString().slice(0, 10);
    const body = `card=${cardSlug}&today=${today}`;
    postToWasmWithCallback('POST', '/api/room/fists/final', body, (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
        execScripts(container);
      }
    });
  },

  /** Send Final Blows data to peer. Called by inline script from WASM response. */
  sendFinalBlows(submissionData) {
    sendToPeer({ type: 'final_blows_submission', data: submissionData });
    console.log('[multiplayer] Sent Final Blows submission to peer');
  },

  /** Reset fists on both local and remote sides. */
  resetFists() {
    postToWasmWithCallback('POST', '/api/room/fists/reset', '', (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
      }
      refreshRoomStatus();
    });

    // Notify remote peer to reset as well
    sendToPeer({ type: 'fists_reset' });
    console.log('[multiplayer] Sent fists reset to peer');
  },

  /** Report combat outcome ("Did you win?" answer). */
  reportOutcome(won) {
    // Determine attacker_won from local role + answer
    postToWasmWithCallback('POST', '/api/room/fists/outcome', 'won=' + won, (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
        execScripts(container);
      }
      setTimeout(refreshKealTracker, 150);
    });

    // Derive attacker_won for the peer
    postToWasmWithCallback('GET', '/api/room/state', '', (json) => {
      try {
        const state = JSON.parse(json);
        if (!state || !state.fists) return;
        let localRole;
        if (state.fists.local) {
          localRole = state.fists.local.role;
        } else if (state.fists.local_final_blows) {
          localRole = 'Defending';
        } else {
          return;
        }
        let attackerWon;
        if (localRole === 'Attacking') {
          attackerWon = won === 'yes';
        } else {
          attackerWon = won === 'no';
        }
        sendToPeer({ type: 'fists_outcome', attacker_won: attackerWon });
        console.log('[multiplayer] Sent fists outcome to peer: attacker_won=' + attackerWon);
      } catch (e) {
        console.warn('[multiplayer] Could not parse room state for outcome sync:', e);
      }
    });
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
