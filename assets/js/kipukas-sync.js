/**
 * kipukas-sync.js — Cross-device PLAYER_DOC sync via WebSocket relay.
 *
 * Phase E: Syncs the yrs CRDT PLAYER_DOC between two devices using the
 * same signaling server and relay protocol proven in multiplayer. Device
 * pairing uses room codes; mutual HMAC-SHA256 authentication verifies
 * both devices share the same passphrase before any data exchange.
 *
 * Lazy-loaded when the user opens "Sync Devices" from the hamburger menu.
 * Exposed globally as window.kipukasSync.
 */

// This device's id in the sync room (the relay keys each place in a room by
// player id). A fresh one per page load: a sync session does not outlive it.
const SYNC_PLAYER_ID = Array.from(
  crypto.getRandomValues(new Uint8Array(12)),
  (b) => b.toString(16).padStart(2, '0'),
).join('');

/**
 * The relay address for one connection, saying what this device wants:
 * `{ action: 'create', name }` or `{ action: 'join', code, name }`.
 * The relay address itself is defined once in kipukas-api.js.
 */
function relayAddress(want) {
  const url = new URL(globalThis.kipukasRelayUrl());
  for (const [key, value] of Object.entries(want)) url.searchParams.set(key, value);
  url.searchParams.set('player', SYNC_PLAYER_ID);
  return url.toString();
}

let ws = null;
let syncRoomCode = '';
let syncPassphrase = '';
let peerPresent = false;
let authenticated = false;

// ── WASM helpers ───────────────────────────────────────────────────

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

// ── Signaling connection ───────────────────────────────────────────

function connectSignaling(want) {
  return new Promise((resolve, reject) => {
    console.log('[sync] Connecting to the relay:', want.action);
    ws = new WebSocket(relayAddress(want));

    ws.onopen = () => {
      console.log('[sync] Signaling connected');
      resolve();
    };

    ws.onerror = (err) => {
      console.error('[sync] Signaling error:', err);
      reject(err);
    };

    ws.onclose = () => {
      console.log('[sync] Signaling disconnected');
      ws = null;
      peerPresent = false;
      authenticated = false;
      globalThis.dispatchEvent(new CustomEvent('sync-disconnected'));
    };

    ws.onmessage = (event) => {
      handleSignalingMessage(JSON.parse(event.data));
    };
  });
}

function sendToPeer(data) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'relay', data }));
  }
}

// ── Signaling message handler ──────────────────────────────────────

function handleSignalingMessage(msg) {
  console.log('[sync] ← signaling msg:', msg.type);
  switch (msg.type) {
    case 'room_created':
      syncRoomCode = msg.code;
      console.log('[sync] Sync room created:', syncRoomCode);
      globalThis.dispatchEvent(
        new CustomEvent('sync-room-created', { detail: { code: syncRoomCode } }),
      );
      break;

    case 'room_joined':
      syncRoomCode = msg.code;
      console.log('[sync] Joined sync room:', syncRoomCode);
      globalThis.dispatchEvent(
        new CustomEvent('sync-room-joined', { detail: { code: syncRoomCode } }),
      );
      break;

    case 'peer_joined':
      console.log('[sync] Peer device connected');
      peerPresent = true;
      globalThis.dispatchEvent(new CustomEvent('sync-peer-joined'));
      // Initiate mutual authentication
      startAuth();
      break;

    case 'relay':
      if (msg.data) handleRelayedMessage(msg.data);
      break;

    case 'peer_away':
    case 'peer_left':
      console.log('[sync] Peer device disconnected');
      peerPresent = false;
      authenticated = false;
      globalThis.dispatchEvent(new CustomEvent('sync-peer-left'));
      break;

    case 'error':
      console.error('[sync] Server error:', msg.message);
      globalThis.dispatchEvent(
        new CustomEvent('sync-error', { detail: { message: msg.message } }),
      );
      break;
  }
}

// ── Mutual authentication ──────────────────────────────────────────

function startAuth() {
  // Compute HMAC(passphrase, room_code) and send to peer
  postToWasmWithCallback(
    'POST',
    '/api/player/sync/auth',
    'passphrase=' + encodeURIComponent(syncPassphrase) +
      '&room_code=' + encodeURIComponent(syncRoomCode),
    (mac) => {
      if (mac && !mac.includes('error')) {
        sendToPeer({ type: 'sync_auth', mac });
        console.log('[sync] Sent auth proof to peer');
      } else {
        console.error('[sync] Failed to compute auth MAC:', mac);
      }
    },
  );
}

// ── Relayed message protocol ───────────────────────────────────────

function handleRelayedMessage(msg) {
  switch (msg.type) {
    case 'sync_auth': {
      // Peer sent their HMAC proof — verify it
      console.log('[sync] Received auth proof from peer');
      postToWasmWithCallback(
        'POST',
        '/api/player/sync/verify',
        'passphrase=' + encodeURIComponent(syncPassphrase) +
          '&room_code=' + encodeURIComponent(syncRoomCode) +
          '&mac=' + encodeURIComponent(msg.mac),
        (result) => {
          if (result === 'ok') {
            console.log('[sync] Peer authenticated successfully');
            authenticated = true;
            globalThis.dispatchEvent(new CustomEvent('sync-authenticated'));
            // Start yrs sync handshake
            initiatePlayerSync();
          } else {
            console.error('[sync] Peer authentication failed — wrong passphrase');
            globalThis.dispatchEvent(
              new CustomEvent('sync-auth-failed', {
                detail: { message: 'Wrong passphrase on the other device' },
              }),
            );
            disconnect();
          }
        },
      );
      break;
    }

    case 'player_sv': {
      // Peer sent their PLAYER_DOC state vector — compute diff and send
      console.log('[sync] Received PLAYER_DOC state vector from peer');
      postToWasmWithCallback(
        'POST',
        '/api/player/sync/diff',
        'sv=' + msg.sv,
        (diff) => {
          if (diff && !diff.startsWith('{')) {
            sendToPeer({ type: 'player_update', update: diff });
            console.log('[sync] Sent PLAYER_DOC diff to peer');
          }
        },
      );
      // Send our SV back so peer can compute their diff for us
      wasmRequest('GET', '/api/player/sync/sv', '', '', (sv) => {
        if (sv) {
          sendToPeer({ type: 'player_sv_reply', sv });
        }
      });
      break;
    }

    case 'player_sv_reply': {
      // Peer replied with their SV — compute and send our diff
      console.log('[sync] Received PLAYER_DOC state vector reply from peer');
      postToWasmWithCallback(
        'POST',
        '/api/player/sync/diff',
        'sv=' + msg.sv,
        (diff) => {
          if (diff && !diff.startsWith('{')) {
            sendToPeer({ type: 'player_update', update: diff });
            console.log('[sync] Sent PLAYER_DOC diff reply to peer');
          }
        },
      );
      break;
    }

    case 'player_update': {
      // Peer sent a yrs update — apply to our PLAYER_DOC
      console.log('[sync] Received PLAYER_DOC update from peer');
      postToWasmWithCallback(
        'POST',
        '/api/player/sync/apply',
        'update=' + msg.update,
        (result) => {
          if (result === 'ok') {
            // The engine has the merged state stored (kipukas-api.js, SAVE).
            console.log('[sync] PLAYER_DOC update applied');
            globalThis.dispatchEvent(new CustomEvent('sync-updated'));
          } else {
            console.warn('[sync] Failed to apply update:', result);
          }
        },
      );
      break;
    }

    default:
      console.log('[sync] Unknown relayed message:', msg);
  }
}

// ── yrs sync handshake ─────────────────────────────────────────────

function initiatePlayerSync() {
  wasmRequest('GET', '/api/player/sync/sv', '', '', (sv) => {
    if (sv) {
      sendToPeer({ type: 'player_sv', sv });
      console.log('[sync] Sent PLAYER_DOC state vector to peer');
    }
  });
}

// ── Live sync broadcast ────────────────────────────────────────────

/** Broadcast PLAYER_DOC changes to the sync peer.
 *  Called by kipukas-api.js when the engine has the player's state stored
 *  and a sync session is active.
 *  Sends our full diff (from empty SV) — yrs deduplicates already-seen changes. */
function broadcastUpdate() {
  if (!authenticated || !peerPresent) return;
  // Get peer's last known SV — simplest approach: just re-run the handshake
  // by sending our SV. The peer computes the diff for us. This is idempotent.
  wasmRequest('GET', '/api/player/sync/sv', '', '', (sv) => {
    if (sv) {
      sendToPeer({ type: 'player_sv', sv });
    }
  });
}

// ── Disconnect ─────────────────────────────────────────────────────

function disconnect() {
  peerPresent = false;
  authenticated = false;
  syncRoomCode = '';
  syncPassphrase = '';
  if (ws) {
    ws.close();
    ws = null;
  }
  globalThis.dispatchEvent(new CustomEvent('sync-disconnected'));
}

// ── Public API ─────────────────────────────────────────────────────

const kipukasSync = {
  /** Create a sync room. */
  async createRoom(passphrase) {
    syncPassphrase = passphrase;
    try {
      if (ws) ws.close();
      await connectSignaling({ action: 'create', name: '' });
    } catch (_err) {
      globalThis.dispatchEvent(
        new CustomEvent('sync-error', {
          detail: { message: 'Could not connect to signaling server' },
        }),
      );
    }
  },

  /** Join an existing sync room by code. */
  async joinRoom(code, passphrase) {
    syncPassphrase = passphrase;
    const upperCode = code.trim().toUpperCase();
    if (upperCode.length !== 4) {
      globalThis.dispatchEvent(
        new CustomEvent('sync-error', {
          detail: { message: 'Please enter a 4-character room code' },
        }),
      );
      return;
    }
    try {
      if (ws) ws.close();
      await connectSignaling({ action: 'join', code: upperCode, name: '' });
    } catch (_err) {
      globalThis.dispatchEvent(
        new CustomEvent('sync-error', {
          detail: { message: 'Could not connect to signaling server' },
        }),
      );
    }
  },

  /** Disconnect from sync session. */
  disconnect,

  /** Broadcast PLAYER_DOC changes to sync peer. */
  broadcastUpdate,

  /** Check if sync session is active and authenticated. */
  isConnected() {
    return authenticated && peerPresent && ws && ws.readyState === WebSocket.OPEN;
  },
};

globalThis.kipukasSync = kipukasSync;
console.log('[sync] Kipukas device sync module loaded');
