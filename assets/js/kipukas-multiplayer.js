/**
 * kipukas-multiplayer.js — WebRTC multiplayer manager for Kipukas.
 *
 * Phase 4: Handles signaling server connection, WebRTC peer setup,
 * data channel messaging, and room state synchronization with WASM.
 *
 * Persists room session in sessionStorage so connections survive page
 * navigation (multi-page Jekyll site). On each page load the module
 * automatically rejoins the signaling server room if a session exists.
 *
 * Exposed globally as window.kipukasMultiplayer for use by WASM-returned
 * HTML onclick handlers.
 */

// Signaling server base URL (Deno Deploy)
const SIGNAL_BASE = 'https://signal.kipukas.deno.net';
const SIGNAL_WS_URL = 'wss://signal.kipukas.deno.net/ws';

// Baseline ICE servers (STUN only — Cloudflare primary, Google fallback).
// TURN servers are fetched dynamically from the signaling server.
const STUN_SERVERS = [
  { urls: 'stun:stun.cloudflare.com:3478' },
  { urls: 'stun:stun.l.google.com:19302' },
  { urls: 'stun:stun1.l.google.com:19302' },
];

const SESSION_KEY = 'kipukas_room';

let ws = null; // WebSocket to signaling server
let pc = null; // RTCPeerConnection
let dc = null; // RTCDataChannel
let roomCode = '';
let roomName = '';
let isCreator = false;
let peerConnectedCalled = false; // Guard against double onPeerConnected calls
let turnServers = []; // Populated from signaling server

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
  } catch (_) { /* ignore */ }
}

// ── TURN credential fetching ───────────────────────────────────────

/** Fetch TURN credentials from the signaling server (proxies Cloudflare API). */
async function fetchTurnCredentials() {
  try {
    const resp = await fetch(`${SIGNAL_BASE}/turn-credentials`);
    if (!resp.ok) {
      console.warn('[multiplayer] TURN credential fetch failed:', resp.status);
      return;
    }
    const data = await resp.json();
    if (data.iceServers && Array.isArray(data.iceServers)) {
      turnServers = data.iceServers;
      console.log('[multiplayer] TURN credentials loaded:', turnServers.length, 'servers');
    } else {
      console.log('[multiplayer] No TURN servers available (STUN only)');
    }
  } catch (err) {
    console.warn('[multiplayer] Could not fetch TURN credentials:', err);
  }
}

/** Build the full ICE server list (STUN + TURN). */
function getIceServers() {
  return [...STUN_SERVERS, ...turnServers];
}

// ── Signaling ──────────────────────────────────────────────────────

/** Connect to signaling server WebSocket. */
function connectSignaling() {
  return new Promise((resolve, reject) => {
    console.log('[multiplayer] Connecting to signaling server:', SIGNAL_WS_URL);
    ws = new WebSocket(SIGNAL_WS_URL);

    ws.onopen = () => {
      console.log('[multiplayer] Signaling connected');
      resolve();
    };

    ws.onerror = (err) => {
      console.error('[multiplayer] Signaling error:', err);
      reject(err);
    };

    ws.onclose = () => {
      console.log('[multiplayer] Signaling disconnected');
      ws = null;
    };

    ws.onmessage = (event) => {
      handleSignalingMessage(JSON.parse(event.data));
    };
  });
}

/** Handle messages from the signaling server. */
function handleSignalingMessage(msg) {
  console.log('[multiplayer] ← signaling msg:', msg.type, msg);
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
      // Update WASM state so it knows we're in a room (even before WebRTC connects)
      postToWasm(
        'POST',
        '/api/room/join',
        `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
      );
      // Joiner creates the RTCPeerConnection and sends offer
      setupPeerConnection(true);
      refreshRoomStatus();
      break;

    case 'peer_joined':
      console.log('[multiplayer] Peer joined our room');
      // Creator sets up peer connection, waits for offer
      setupPeerConnection(false);
      break;

    case 'sdp_offer':
      handleSdpOffer(msg.data);
      break;

    case 'sdp_answer':
      handleSdpAnswer(msg.data);
      break;

    case 'ice_candidate':
      handleIceCandidate(msg.data);
      break;

    case 'peer_left':
      console.log('[multiplayer] Peer disconnected');
      cleanupPeer();
      // Don't clear session — peer may come back (grace period on server)
      postToWasm('POST', '/api/room/peer_left', '');
      refreshRoomStatus();
      break;

    case 'error':
      console.error('[multiplayer] Server error:', msg.message);
      // If room expired, clear session
      if (msg.message && msg.message.includes('not found')) {
        clearSession();
        postToWasm('POST', '/api/room/disconnect', '');
      }
      showError(msg.message);
      break;
  }
}

// ── WebRTC ─────────────────────────────────────────────────────────

/** Set up RTCPeerConnection and data channel. */
function setupPeerConnection(initiator) {
  console.log('[multiplayer] setupPeerConnection(initiator=' + initiator + ')');
  // Clean up any existing connection first
  cleanupPeer();

  const iceServers = getIceServers();
  console.log(
    '[multiplayer] Using ICE servers:',
    iceServers.length,
    '(STUN:',
    STUN_SERVERS.length,
    '+ TURN:',
    turnServers.length,
    ')',
  );
  pc = new RTCPeerConnection({ iceServers });

  pc.onicecandidate = (event) => {
    if (event.candidate && ws && ws.readyState === WebSocket.OPEN) {
      console.log(
        '[multiplayer] → sending ICE candidate:',
        event.candidate.candidate?.substring(0, 60),
      );
      ws.send(JSON.stringify({ type: 'ice_candidate', data: event.candidate }));
    } else if (!event.candidate) {
      console.log('[multiplayer] ICE gathering complete (null candidate)');
    }
  };

  pc.onicegatheringstatechange = () => {
    console.log('[multiplayer] ICE gathering state:', pc.iceGatheringState);
  };

  pc.oniceconnectionstatechange = () => {
    console.log('[multiplayer] ICE connection state:', pc.iceConnectionState);
  };

  pc.onsignalingstatechange = () => {
    console.log('[multiplayer] Signaling state:', pc.signalingState);
  };

  pc.onconnectionstatechange = () => {
    console.log('[multiplayer] Connection state:', pc.connectionState);
    if (pc.connectionState === 'connected') {
      onPeerConnected();
    } else if (pc.connectionState === 'failed' || pc.connectionState === 'disconnected') {
      cleanupPeer();
      postToWasm('POST', '/api/room/peer_left', '');
      refreshRoomStatus();
    }
  };

  if (initiator) {
    // Create data channel and send SDP offer
    dc = pc.createDataChannel('kipukas', { ordered: true });
    setupDataChannel(dc);
    console.log('[multiplayer] Creating SDP offer...');
    pc.createOffer().then(async (offer) => {
      console.log('[multiplayer] SDP offer created, setting local description...');
      await pc.setLocalDescription(offer);
      console.log('[multiplayer] Local description set, sending offer to signaling server');
      // Log whether candidates are embedded in the SDP
      const candidateLines = (pc.localDescription?.sdp || '').split('\n').filter((l) =>
        l.startsWith('a=candidate')
      );
      console.log(
        '[multiplayer] Candidates embedded in offer SDP:',
        candidateLines.length,
        candidateLines,
      );
      ws.send(JSON.stringify({ type: 'sdp_offer', data: pc.localDescription }));
    }).catch((err) => {
      console.error('[multiplayer] Failed to create/send offer:', err);
    });
  } else {
    // Wait for data channel from the initiator
    pc.ondatachannel = (event) => {
      dc = event.channel;
      setupDataChannel(dc);
    };
  }
}

/** Configure data channel event handlers. */
function setupDataChannel(channel) {
  channel.onopen = () => {
    console.log('[multiplayer] Data channel open');
    // Data channel open is the most reliable signal that we can
    // exchange messages. Use it as the primary "connected" trigger.
    onPeerConnected();
  };

  channel.onclose = () => {
    console.log('[multiplayer] Data channel closed');
  };

  channel.onmessage = (event) => {
    handleDataChannelMessage(JSON.parse(event.data));
  };
}

/** Handle incoming SDP offer from remote peer. */
async function handleSdpOffer(offer) {
  console.log('[multiplayer] handleSdpOffer called, pc exists:', !!pc);
  if (!pc) {
    console.error('[multiplayer] No peer connection! Cannot handle SDP offer.');
    return;
  }
  try {
    console.log('[multiplayer] Setting remote description (offer)...');
    await pc.setRemoteDescription(new RTCSessionDescription(offer));
    console.log('[multiplayer] Remote description set, creating answer...');
    const answer = await pc.createAnswer();
    console.log('[multiplayer] Answer created, setting local description...');
    await pc.setLocalDescription(answer);
    console.log('[multiplayer] Local description set, sending answer to signaling server');
    // Log whether candidates are embedded in the SDP
    const candidateLines = (pc.localDescription?.sdp || '').split('\n').filter((l) =>
      l.startsWith('a=candidate')
    );
    console.log(
      '[multiplayer] Candidates embedded in answer SDP:',
      candidateLines.length,
      candidateLines,
    );
    ws.send(JSON.stringify({ type: 'sdp_answer', data: pc.localDescription }));
  } catch (err) {
    console.error('[multiplayer] Error handling SDP offer:', err);
  }
}

/** Handle incoming SDP answer from remote peer. */
async function handleSdpAnswer(answer) {
  console.log('[multiplayer] handleSdpAnswer called, pc exists:', !!pc);
  if (!pc) {
    console.error('[multiplayer] No peer connection! Cannot handle SDP answer.');
    return;
  }
  try {
    console.log('[multiplayer] Setting remote description (answer)...');
    await pc.setRemoteDescription(new RTCSessionDescription(answer));
    console.log('[multiplayer] Remote description (answer) set successfully');
  } catch (err) {
    console.error('[multiplayer] Error handling SDP answer:', err);
  }
}

/** Handle incoming ICE candidate from remote peer. */
async function handleIceCandidate(candidate) {
  console.log(
    '[multiplayer] handleIceCandidate called, pc exists:',
    !!pc,
    'signalingState:',
    pc?.signalingState,
  );
  if (!pc) {
    console.error('[multiplayer] No peer connection! Cannot add ICE candidate.');
    return;
  }
  try {
    await pc.addIceCandidate(new RTCIceCandidate(candidate));
    console.log('[multiplayer] ICE candidate added successfully');
  } catch (err) {
    console.warn('[multiplayer] ICE candidate error:', err);
  }
}

/** Called when WebRTC connection is fully established.
 *  May be called from both connectionstatechange and data channel onopen;
 *  the guard ensures we only process it once per connection. */
function onPeerConnected() {
  if (peerConnectedCalled) return;
  peerConnectedCalled = true;
  console.log('[multiplayer] Peer connected via WebRTC!');
  postToWasm(
    'POST',
    '/api/room/connected',
    `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
  );
  // Notify UI that room is connected
  window.dispatchEvent(new CustomEvent('room-connected'));
  refreshRoomStatus();
}

/** Clean up peer connection. */
function cleanupPeer() {
  peerConnectedCalled = false;
  if (dc) {
    try {
      dc.close();
    } catch (_) { /* ignore */ }
    dc = null;
  }
  if (pc) {
    try {
      pc.close();
    } catch (_) { /* ignore */ }
    pc = null;
  }
}

// ── Data Channel Protocol ──────────────────────────────────────────

/** Handle messages received via the WebRTC data channel. */
function handleDataChannelMessage(msg) {
  switch (msg.type) {
    case 'fists_submission': {
      // Remote peer sent their fists combat choice.
      // Store it in WASM via fire-and-forget, then show a message
      // WITHOUT wiping the local submission form.
      const json = JSON.stringify(msg.data);
      console.log('[multiplayer] Received fists submission:', json);

      // Store in WASM
      postToWasmWithCallback('POST', '/api/room/fists/sync', json, (html) => {
        // Check if the response contains a result page (regular or Final Blows)
        // by looking for the reportOutcome function which only appears in results.
        if (html && html.includes('reportOutcome')) {
          const container = document.getElementById('fists-container');
          if (container) {
            container.innerHTML = html;
            if (typeof htmx !== 'undefined') htmx.process(container);
            execScripts(container);
          }
        } else {
          // Local hasn't submitted yet — just show a message in the
          // dedicated message area, leaving the form intact.
          showFistsMessage('✓ Opponent has locked in their choice!', 'text-emerald-600');
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
        // Refresh to re-fetch the fists selection form (same as local resetFists)
        refreshRoomStatus();
      });
      break;
    }

    case 'fists_outcome': {
      // Remote peer reported the combat outcome.
      // attacker_won is already resolved by the sender.
      const attackerWon = msg.attacker_won;
      console.log('[multiplayer] Received fists outcome: attacker_won=' + attackerWon);
      postToWasmWithCallback('POST', '/api/room/fists/outcome',
        'attacker_won=' + (attackerWon ? 'true' : 'false'), (html) => {
        const container = document.getElementById('fists-container');
        if (container) {
          container.innerHTML = html;
          if (typeof htmx !== 'undefined') htmx.process(container);
          execScripts(container);
        }
        // Refresh the keal damage tracker so checkboxes reflect auto-marked damage
        setTimeout(refreshKealTracker, 150);
      });
      break;
    }

    case 'final_blows_submission': {
      // Remote peer sent their Final Blows submission (card with exhausted keal means).
      const json = JSON.stringify(msg.data);
      console.log('[multiplayer] Received Final Blows submission:', json);

      // Store in WASM
      postToWasmWithCallback('POST', '/api/room/fists/final/sync', json, (html) => {
        // Check if the response contains a result page by looking for the
        // reportOutcome function which only appears in actual results.
        if (html && html.includes('reportOutcome')) {
          const container = document.getElementById('fists-container');
          if (container) {
            container.innerHTML = html;
            if (typeof htmx !== 'undefined') htmx.process(container);
            execScripts(container);
          }
        } else {
          // Local hasn't submitted yet — just show a message in the
          // dedicated message area, leaving the form intact.
          showFistsMessage('✓ Opponent stated Final Blows!', 'text-emerald-600');
        }
      });
      break;
    }

    default:
      console.log('[multiplayer] Unknown data channel message:', msg);
  }
}

/** Send a fists submission to the remote peer via data channel. */
function sendFists(submissionData) {
  if (dc && dc.readyState === 'open') {
    dc.send(JSON.stringify({ type: 'fists_submission', data: submissionData }));
    console.log('[multiplayer] Sent fists submission to peer');
  } else {
    console.warn('[multiplayer] Data channel not open, cannot send fists');
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
        tracker.querySelectorAll('[hx-get],[hx-post]').forEach(el => htmx.process(el));
      }
      // Initialize Alpine components on the new DOM tree. Raw innerHTML
      // swaps bypass htmx's swap pipeline, so Alpine's MutationObserver
      // doesn't reliably detect new x-data elements on all browsers.
      // Alpine.initTree() explicitly scans and initializes them.
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

/** Persist WASM game state to localStorage.
 *  Fetches the current state JSON directly from the WASM worker and writes
 *  it to the same localStorage key used by the /api/game/persist endpoint.
 *  Avoids detached <script> tags which don't execute without a browser refresh. */
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

/** Refresh the room status panel and fists section in the UI.
 *  Small delay ensures WASM state updates (via postToWasm) are
 *  processed before the HTMX GET requests arrive at the worker. */
function refreshRoomStatus() {
  if (typeof htmx === 'undefined') return;
  setTimeout(() => {
    const status = document.getElementById('room-status');
    if (status) {
      htmx.ajax('GET', '/api/room/status', { target: '#room-status', swap: 'innerHTML' });
    }
    // Also refresh the fists section so it reflects the new connection state.
    // Re-use the hx-get URL already on the element (includes card slug if any).
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

  // Tell WASM we're in a room (waiting for peer)
  postToWasm(
    'POST',
    '/api/room/create',
    `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
  );

  // Fetch TURN credentials before connecting
  await fetchTurnCredentials();

  try {
    await connectSignaling();
    // Use 'rejoin' — skips name validation, cancels grace timers
    ws.send(JSON.stringify({ type: 'rejoin', code: roomCode }));
  } catch (err) {
    console.warn('[multiplayer] Auto-reconnect failed:', err);
    // Don't clear session — user can try manually
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

    // Fetch TURN credentials before creating the room
    await fetchTurnCredentials();

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

    // Fetch TURN credentials before joining
    await fetchTurnCredentials();

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
    cleanupPeer();
    clearSession();
    if (ws) {
      ws.close();
      ws = null;
    }
    roomCode = '';
    roomName = '';
    postToWasm('POST', '/api/room/disconnect', '');
    // Notify UI that room is disconnected
    window.dispatchEvent(new CustomEvent('room-disconnected'));
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
    sendFists(submissionData);
  },

  /** Submit Final Blows for a card with exhausted keal means. */
  submitFinalBlows(cardSlug) {
    // POST to WASM to store local Final Blows submission
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
    if (dc && dc.readyState === 'open') {
      dc.send(JSON.stringify({ type: 'final_blows_submission', data: submissionData }));
      console.log('[multiplayer] Sent Final Blows submission to peer');
    } else {
      console.warn('[multiplayer] Data channel not open, cannot send Final Blows');
    }
  },

  /** Reset fists on both local and remote sides. Called from "Try Again" button. */
  resetFists() {
    // Reset local WASM state and refresh UI
    postToWasmWithCallback('POST', '/api/room/fists/reset', '', (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
      }
      // After local reset, refresh the fists section to show the form again
      refreshRoomStatus();
    });

    // Notify remote peer to reset as well
    if (dc && dc.readyState === 'open') {
      dc.send(JSON.stringify({ type: 'fists_reset' }));
      console.log('[multiplayer] Sent fists reset to peer');
    }
  },

  /** Report combat outcome ("Did you win?" answer). Called from WASM-rendered buttons. */
  reportOutcome(won) {
    // Determine attacker_won from local role + answer
    postToWasmWithCallback('POST', '/api/room/fists/outcome', 'won=' + won, (html) => {
      const container = document.getElementById('fists-container');
      if (container) {
        container.innerHTML = html;
        if (typeof htmx !== 'undefined') htmx.process(container);
        execScripts(container);
      }
      // Refresh the keal damage tracker so checkboxes reflect auto-marked damage
      setTimeout(refreshKealTracker, 150);
    });

    // Derive attacker_won for the peer: we need to know our local role.
    // For Final Blows (local_final_blows exists, local is null) treat as Defending.
    postToWasmWithCallback('GET', '/api/room/state', '', (json) => {
      try {
        const state = JSON.parse(json);
        if (!state || !state.fists) return;
        let localRole;
        if (state.fists.local) {
          localRole = state.fists.local.role;
        } else if (state.fists.local_final_blows) {
          // Final Blows card is always the defender
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
        // Send to peer
        if (dc && dc.readyState === 'open') {
          dc.send(JSON.stringify({ type: 'fists_outcome', attacker_won: attackerWon }));
          console.log('[multiplayer] Sent fists outcome to peer: attacker_won=' + attackerWon);
        }
      } catch (e) {
        console.warn('[multiplayer] Could not parse room state for outcome sync:', e);
      }
    });
  },

  /** Check if currently connected to a peer. */
  isConnected() {
    return dc && dc.readyState === 'open';
  },
};

globalThis.kipukasMultiplayer = kipukasMultiplayer;
console.log('[multiplayer] Kipukas multiplayer module loaded');
