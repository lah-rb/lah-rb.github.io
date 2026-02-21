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

// Signaling server URL (Deno Deploy)
const SIGNAL_URL = 'wss://signal.kipukas.deno.net/ws';

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];

const SESSION_KEY = 'kipukas_room';

let ws = null; // WebSocket to signaling server
let pc = null; // RTCPeerConnection
let dc = null; // RTCDataChannel
let roomCode = '';
let roomName = '';
let isCreator = false;

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

// ── Signaling ──────────────────────────────────────────────────────

/** Connect to signaling server WebSocket. */
function connectSignaling() {
  return new Promise((resolve, reject) => {
    const url = SIGNAL_URL;
    console.log('[multiplayer] Connecting to signaling server:', url);
    ws = new WebSocket(url);

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
      // Set WASM to "in room, waiting for data channel" (connected=false)
      postToWasm(
        'POST',
        '/api/room/create',
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
      refreshRoomStatus();
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
  // Clean up any existing connection first
  cleanupPeer();

  pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });

  pc.onicecandidate = (event) => {
    if (event.candidate && ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'ice_candidate', data: event.candidate }));
    }
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
    pc.createOffer().then((offer) => {
      pc.setLocalDescription(offer);
      ws.send(JSON.stringify({ type: 'sdp_offer', data: offer }));
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
    // NOW we are truly connected — tell WASM and refresh UI
    postToWasm(
      'POST',
      '/api/room/connected',
      `code=${roomCode}&name=${encodeURIComponent(roomName)}`,
    );
    refreshRoomStatus();
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
  if (!pc) return;
  await pc.setRemoteDescription(new RTCSessionDescription(offer));
  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);
  ws.send(JSON.stringify({ type: 'sdp_answer', data: answer }));
}

/** Handle incoming SDP answer from remote peer. */
async function handleSdpAnswer(answer) {
  if (!pc) return;
  await pc.setRemoteDescription(new RTCSessionDescription(answer));
}

/** Handle incoming ICE candidate from remote peer. */
async function handleIceCandidate(candidate) {
  if (!pc) return;
  try {
    await pc.addIceCandidate(new RTCIceCandidate(candidate));
  } catch (err) {
    console.warn('[multiplayer] ICE candidate error:', err);
  }
}

/** Called when WebRTC connection is fully established. */
function onPeerConnected() {
  console.log('[multiplayer] Peer connected via WebRTC!');
  // Note: connected state is set in data channel onopen, not here,
  // to ensure the data channel is actually usable before showing fists form.
}

/** Clean up peer connection. */
function cleanupPeer() {
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
      // Remote peer sent their fists combat choice
      const json = JSON.stringify(msg.data);
      console.log('[multiplayer] Received fists submission:', json);
      // POST to WASM to store remote submission
      postToWasmWithCallback('POST', '/api/room/fists/sync', json, (html) => {
        const container = document.getElementById('fists-container');
        if (container) {
          container.innerHTML = html;
          if (typeof htmx !== 'undefined') htmx.process(container);
          execScripts(container);
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
    cleanupPeer();
    clearSession();
    if (ws) {
      ws.close();
      ws = null;
    }
    roomCode = '';
    roomName = '';
    postToWasm('POST', '/api/room/disconnect', '');
    refreshRoomStatus();
  },

  /** Submit fists combat choice. Called from WASM-rendered HTML button. */
  submitFists(cardSlug) {
    const roleEl = document.querySelector('input[name="fists-role"]:checked');
    const kealEl = document.querySelector('input[name="fists-keal"]:checked');

    if (!roleEl) {
      showError('Please select Attacking or Defending.');
      return;
    }
    if (!kealEl) {
      showError('Please select a Keal Means.');
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

  /** Check if currently connected to a peer. */
  isConnected() {
    return dc && dc.readyState === 'open';
  },
};

globalThis.kipukasMultiplayer = kipukasMultiplayer;
console.log('[multiplayer] Kipukas multiplayer module loaded');
