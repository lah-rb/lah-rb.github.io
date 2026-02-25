/**
 * kipukas-multiplayer.js — WebSocket relay multiplayer manager for Kipukas.
 *
 * Phase 4b: Replaces WebRTC peer-to-peer with WebSocket message relay
 * through the signaling server. Game messages are forwarded between peers
 * by the server without inspection — game logic stays 100% client-side
 * in WASM.
 *
 * Persists room session in sessionStorage so connections survive page
 * navigation (multi-page Jekyll site). On each page load the module
 * automatically rejoins the signaling server room if a session exists.
 *
 * Exposed globally as window.kipukasMultiplayer for use by WASM-returned
 * HTML onclick handlers.
 */

// Signaling server WebSocket URL (Deno Deploy)
const SIGNAL_WS_URL = 'wss://signal.kipukas.deno.net/ws';

const SESSION_KEY = 'kipukas_room';
const CRDT_STATE_KEY = 'kipukas_crdt_state';

let ws = null; // WebSocket to signaling server (also the message relay)
let roomCode = '';
let roomName = '';
let isCreator = false;
let peerPresent = false; // Whether the other peer is in the room

// ── Reconnection ───────────────────────────────────────────────────

let reconnectAttempts = 0;
const MAX_RECONNECT_ATTEMPTS = 8;
const BASE_RECONNECT_DELAY_MS = 1000;

/** Exponential backoff with jitter for reconnection. */
function reconnectDelay() {
  const exp = Math.min(reconnectAttempts, 6);
  const base = BASE_RECONNECT_DELAY_MS * Math.pow(2, exp);
  const jitter = Math.random() * base * 0.3;
  return base + jitter;
}

// ── Session persistence ────────────────────────────────────────────

/** Save current room session to sessionStorage. */
function saveSession() {
  if (!roomCode) return;
  const data = { code: roomCode, name: roomName, creator: isCreator };
  try {
    sessionStorage.setItem(SESSION_KEY, JSON.stringify(data));
  } catch (_) { /* ignore */ }
}

/** Load room session from sessionStorage (returns null if none). */
function loadSession() {
  try {
    const raw = sessionStorage.getItem(SESSION_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch (_) {
    return null;
  }
}

/** Clear saved room session. */
function clearSession() {
  try {
    sessionStorage.removeItem(SESSION_KEY);
    sessionStorage.removeItem(CRDT_STATE_KEY);
  } catch (_) { /* ignore */ }
}

// ── CRDT Doc persistence (survives page navigation) ────────────────

/** Persist the yrs CRDT Doc state to sessionStorage.
 *  Called after every mutation so the Doc survives page navigation. */
function persistCrdtState() {
  wasmRequest('GET', '/api/room/yrs/state', '', '', (state) => {
    if (state) {
      try {
        sessionStorage.setItem(CRDT_STATE_KEY, state);
      } catch (_) { /* ignore quota errors */ }
    }
  });
}

/** Restore the yrs CRDT Doc from sessionStorage (before sync handshake).
 *  Returns a Promise that resolves when restoration is complete. */
function restoreCrdtState() {
  return new Promise((resolve) => {
    try {
      const state = sessionStorage.getItem(CRDT_STATE_KEY);
      if (!state) {
        resolve();
        return;
      }
      postToWasmWithCallback('POST', '/api/room/yrs/restore', 'state=' + state, () => {
        console.log('[multiplayer] Restored CRDT Doc from sessionStorage');
        resolve();
      });
    } catch (_) {
      resolve();
    }
  });
}

// ── Signaling + Message Relay ──────────────────────────────────────

/** Connect to signaling server WebSocket. */
function connectSignaling() {
  return new Promise((resolve, reject) => {
    console.log('[multiplayer] Connecting to signaling server:', SIGNAL_WS_URL);
    ws = new WebSocket(SIGNAL_WS_URL);

    ws.onopen = () => {
      console.log('[multiplayer] Signaling connected');
      reconnectAttempts = 0;
      resolve();
    };

    ws.onerror = (err) => {
      console.error('[multiplayer] Signaling error:', err);
      reject(err);
    };

    ws.onclose = () => {
      console.log('[multiplayer] Signaling disconnected');
      ws = null;

      // If we were in a room, mark peer as gone and attempt reconnect
      if (roomCode) {
        if (peerPresent) {
          peerPresent = false;
          postToWasm('POST', '/api/room/peer_left', '');
          globalThis.dispatchEvent(new CustomEvent('room-disconnected'));
          refreshRoomStatus();
        }
        scheduleReconnect();
      }
    };

    ws.onmessage = (event) => {
      handleSignalingMessage(JSON.parse(event.data));
    };
  });
}

/** Schedule a reconnection attempt with exponential backoff. */
function scheduleReconnect() {
  if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
    console.warn('[multiplayer] Max reconnect attempts reached, giving up');
    return;
  }
  const delay = reconnectDelay();
  reconnectAttempts++;
  console.log(
    `[multiplayer] Reconnecting in ${
      Math.round(delay)
    }ms (attempt ${reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS})`,
  );
  setTimeout(async () => {
    if (ws && ws.readyState === WebSocket.OPEN) return; // Already reconnected
    const session = loadSession();
    if (!session || !session.code) return; // Session cleared (user disconnected)
    try {
      await connectSignaling();
      ws.send(JSON.stringify({ type: 'rejoin', code: session.code }));
    } catch (err) {
      console.warn('[multiplayer] Reconnect failed:', err);
      scheduleReconnect();
    }
  }, delay);
}

/** Handle messages from the signaling server. */
function handleSignalingMessage(msg) {
  console.log('[multiplayer] ← signaling msg:', msg.type);
  switch (msg.type) {
    case 'room_created':
      roomCode = msg.code;
      roomName = msg.name || '';
      console.log('[multiplayer] Room created:', roomCode);
      saveSession();
      // Update WASM state
      postToWasm(
        'POST',
        '/api/room/create',
        `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
      );
      refreshRoomStatus();
      break;

    case 'room_joined':
      roomCode = msg.code;
      roomName = msg.name || roomName;
      console.log('[multiplayer] Joined room:', roomCode);
      saveSession();
      // Update WASM state so it knows we're in a room
      postToWasm(
        'POST',
        '/api/room/join',
        `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
      );
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
      peerPresent = false;
      postToWasm('POST', '/api/room/peer_left', '');
      globalThis.dispatchEvent(new CustomEvent('room-disconnected'));
      refreshRoomStatus();
      break;

    case 'error':
      console.error('[multiplayer] Server error:', msg.message);
      // If room expired, clear session
      if (msg.message && msg.message.includes('not found')) {
        clearSession();
        roomCode = '';
        roomName = '';
        postToWasm('POST', '/api/room/disconnect', '');
        globalThis.dispatchEvent(new CustomEvent('room-disconnected'));
      }
      showError(msg.message);
      break;
  }
}

/** Called when both peers are in the room and can exchange messages. */
function onPeerConnected() {
  console.log('[multiplayer] Peer connected via WebSocket relay!');
  postToWasm(
    'POST',
    '/api/room/connected',
    `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
  );
  // Notify UI that room is connected
  globalThis.dispatchEvent(new CustomEvent('room-connected'));
  refreshRoomStatus();

  // Switch alarm display to multiplayer mode (reads from CRDT Doc,
  // shows sync buttons). Seeded alarms from local GameState are
  // already in the Doc via seed_from_local() on room create/join.
  refreshAlarms(true);

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
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'relay', data }));
  } else {
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
        persistCrdtState();
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
 *  card page are stale. Uses direct worker communication (bypasses the
 *  service worker relay) to ensure the updated state is read reliably.
 *  Also persists state to localStorage so changes survive page navigation. */
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

  // Fetch updated HTML directly from the WASM worker
  wasmRequest('GET', '/api/game/damage', '?card=' + slug, '', (html) => {
    if (html) {
      tracker.innerHTML = html;
      // The tracker container has hx-trigger="load" — process only the
      // inner children so we don't re-trigger a redundant HTMX load cycle.
      if (typeof htmx !== 'undefined') {
        tracker.querySelectorAll('[hx-get],[hx-post]').forEach((el) => htmx.process(el));
      }
      // Initialize Alpine components on the new DOM tree.
      if (typeof Alpine !== 'undefined') {
        Alpine.initTree(tracker);
      }
      // Dispatch htmx:afterSwap so Alpine's sentinel watcher in
      // keal_damage_tracker.html updates the Final Blows section visibility
      tracker.dispatchEvent(new CustomEvent('htmx:afterSwap', { bubbles: true }));
      console.log('[multiplayer] Keal damage tracker refreshed for:', slug);
    }
    // Persist state to localStorage so damage survives page navigation
    persistState();
  });
}

/** Persist WASM game state to localStorage. */
function persistState() {
  wasmRequest('GET', '/api/game/state', '', '', (json) => {
    if (json) {
      try {
        localStorage.setItem('kipukas_game_state', json);
        console.log('[multiplayer] Game state persisted after damage change');
      } catch (e) {
        console.warn('[multiplayer] Failed to persist game state:', e);
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
 *  @param {boolean} multiplayer — if true, fetch from /api/room/turns (CRDT);
 *                                 if false, fetch from /api/game/turns (local). */
function refreshAlarms(multiplayer) {
  const endpoint = multiplayer
    ? '/api/room/turns?display=alarms'
    : '/api/game/turns?display=alarms';
  wasmRequest('GET', endpoint.split('?')[0], '?' + endpoint.split('?')[1], '', (html) => {
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

// ── Auto-reconnect on page load ────────────────────────────────────

/** Attempt to rejoin a room from a saved session (after page navigation). */
async function autoReconnect() {
  const session = loadSession();
  if (!session || !session.code) return;

  console.log('[multiplayer] Found saved session, auto-rejoining room:', session.code);
  roomCode = session.code;
  roomName = session.name || '';
  isCreator = session.creator || false;

  // Tell WASM we're in a room (waiting for peer) — this calls init_doc()
  postToWasm(
    'POST',
    '/api/room/create',
    `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
  );

  // Restore CRDT Doc from sessionStorage (must happen after init_doc via
  // room/create, before the sync handshake). Uses a small delay to ensure
  // the create POST has been processed by the worker first.
  await new Promise((r) => setTimeout(r, 50));
  await restoreCrdtState();

  try {
    await connectSignaling();
    // Use 'rejoin' — skips name validation, cancels grace timers
    ws.send(JSON.stringify({ type: 'rejoin', code: roomCode }));
  } catch (err) {
    console.warn('[multiplayer] Auto-reconnect failed:', err);
    scheduleReconnect();
  }
}

// Kick off auto-reconnect when the script loads (deferred, so DOM is ready)
autoReconnect();

// ── Public API (exposed on window.kipukasMultiplayer) ───────────────

const kipukasMultiplayer = {
  /** Create a new room. Reads name from #room-name-input. */
  async createRoom() {
    const nameInput = document.getElementById('room-name-input');
    roomName = nameInput ? nameInput.value.trim() : '';
    isCreator = true;

    try {
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        await connectSignaling();
      }
      ws.send(JSON.stringify({ type: 'create', name: roomName }));
    } catch (err) {
      showError('Could not connect to signaling server. Try again.');
      console.error('[multiplayer] Create room failed:', err);
    }
  },

  /** Join an existing room. Reads code and name from inputs. */
  async joinRoom() {
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

    try {
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        await connectSignaling();
      }
      ws.send(JSON.stringify({ type: 'join', code, name }));
    } catch (err) {
      showError('Could not connect to signaling server. Try again.');
      console.error('[multiplayer] Join room failed:', err);
    }
  },

  /** Disconnect from room and clean up. */
  disconnect() {
    clearSession();
    peerPresent = false;
    reconnectAttempts = MAX_RECONNECT_ATTEMPTS; // Prevent auto-reconnect
    if (ws) {
      ws.close();
      ws = null;
    }
    roomCode = '';
    roomName = '';
    // POST disconnect runs export_to_local() in WASM, copying shared
    // CRDT alarms back to local GameState. After it completes, refresh
    // the alarm display in local mode so exported timers appear.
    postToWasmWithCallback('POST', '/api/room/disconnect', '', () => {
      refreshAlarms(false);
    });
    // Notify UI that room is disconnected
    globalThis.dispatchEvent(new CustomEvent('room-disconnected'));
    refreshRoomStatus();
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

    // POST to WASM to store local submission
    const body = `role=${role}&card=${cardSlug}&keal=${keal}`;
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
    const body = `card=${cardSlug}`;
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
          // Swap the alarm list HTML
          const alarms = document.getElementById('turn-alarms');
          if (alarms && result.html) {
            alarms.innerHTML = result.html;
            if (typeof htmx !== 'undefined') htmx.process(alarms);
          }
          // Relay the yrs update to peer
          if (result.update) {
            sendToPeer({ type: 'yrs_update', update: result.update });
          }
          persistCrdtState();
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
        if (alarms && result.html) {
          alarms.innerHTML = result.html;
          if (typeof htmx !== 'undefined') htmx.process(alarms);
        }
        if (result.update) {
          sendToPeer({ type: 'yrs_update', update: result.update });
        }
        persistCrdtState();
      } catch (e) {
        console.warn('[multiplayer] tickTurns parse error:', e);
      }
    });
    console.log('[multiplayer] Ticked turns via yrs CRDT');
  },

  /** Remove a turn timer by index via yrs CRDT. */
  removeTurn(index) {
    postToWasmWithCallback(
      'POST',
      '/api/room/yrs/alarm/remove',
      'index=' + index,
      (response) => {
        try {
          const result = JSON.parse(response);
          const alarms = document.getElementById('turn-alarms');
          if (alarms && result.html) {
            alarms.innerHTML = result.html;
            if (typeof htmx !== 'undefined') htmx.process(alarms);
          }
          if (result.update) {
            sendToPeer({ type: 'yrs_update', update: result.update });
          }
          persistCrdtState();
        } catch (e) {
          console.warn('[multiplayer] removeTurn parse error:', e);
        }
      },
    );
    console.log('[multiplayer] Removed turn via yrs CRDT, index=' + index);
  },

  /** Check if currently connected to a peer. */
  isConnected() {
    return peerPresent && ws && ws.readyState === WebSocket.OPEN;
  },
};

globalThis.kipukasMultiplayer = kipukasMultiplayer;
console.log('[multiplayer] Kipukas multiplayer module loaded (WebSocket relay)');
