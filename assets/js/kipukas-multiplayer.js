/**
 * kipukas-multiplayer.js — WebRTC multiplayer manager for Kipukas.
 *
 * Phase 4: Handles signaling server connection, WebRTC peer setup,
 * data channel messaging, and room state synchronization with WASM.
 *
 * Exposed globally as window.kipukasMultiplayer for use by WASM-returned
 * HTML onclick handlers.
 */

// Signaling server URL (Deno Deploy)
const SIGNAL_URL = 'wss://signal.kipukas.deno.net/ws';

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];

let ws = null; // WebSocket to signaling server
let pc = null; // RTCPeerConnection
let dc = null; // RTCDataChannel
let roomCode = '';
let roomName = '';
let isCreator = false;

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
      // Joiner creates the RTCPeerConnection and sends offer
      setupPeerConnection(true);
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
      postToWasm('POST', '/api/room/disconnect', '');
      refreshRoomStatus();
      break;

    case 'error':
      console.error('[multiplayer] Server error:', msg.message);
      showError(msg.message);
      break;
  }
}

// ── WebRTC ─────────────────────────────────────────────────────────

/** Set up RTCPeerConnection and data channel. */
function setupPeerConnection(initiator) {
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
      postToWasm('POST', '/api/room/disconnect', '');
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
  postToWasm('POST', '/api/room/connected', `name=${encodeURIComponent(roomName)}`);
  refreshRoomStatus();
}

/** Clean up peer connection. */
function cleanupPeer() {
  if (dc) {
    dc.close();
    dc = null;
  }
  if (pc) {
    pc.close();
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

/** Refresh the room status panel in the UI. */
function refreshRoomStatus() {
  const target = document.getElementById('room-status');
  if (target && typeof htmx !== 'undefined') {
    htmx.ajax('GET', '/api/room/status', { target: '#room-status', swap: 'innerHTML' });
  }
}

/** Show an error message in the room status panel. */
function showError(message) {
  const target = document.getElementById('room-status');
  if (target) {
    target.innerHTML = '<div class="p-4"><span class="text-kip-red text-sm">' + message +
      '</span></div>';
  }
}

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

  /** Join an existing room. Reads code from #room-code-input. */
  async joinRoom() {
    const codeInput = document.getElementById('room-code-input');
    const code = codeInput ? codeInput.value.trim().toUpperCase() : '';
    if (code.length !== 4) {
      showError('Please enter a 4-character room code.');
      return;
    }
    isCreator = false;

    try {
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        await connectSignaling();
      }
      ws.send(JSON.stringify({ type: 'join', code }));
    } catch (err) {
      showError('Could not connect to signaling server. Try again.');
      console.error('[multiplayer] Join room failed:', err);
    }
  },

  /** Disconnect from room and clean up. */
  disconnect() {
    cleanupPeer();
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
