/**
 * Kipukas Signaling Server — WebSocket relay for WebRTC connection brokering.
 *
 * Purpose: Only brokers WebRTC connections. Does NOT process game logic.
 * Deploy: Deno Deploy (free tier) or run locally with `deno run --allow-net main.ts`
 *
 * Protocol:
 *   Client → { type: "create", name: "My Room" }
 *     Server → { type: "room_created", code: "ABCD", name: "My Room" }
 *   Client → { type: "join", code: "ABCD" }
 *     Server → { type: "room_joined", code: "ABCD", name: "My Room" }
 *     Server → (to creator) { type: "peer_joined" }
 *   Client → { type: "sdp_offer"|"sdp_answer"|"ice_candidate", data: ... }
 *     Server → relays to the other peer in the room
 *   On disconnect: notify remaining peer { type: "peer_left" }
 */

interface Room {
  code: string;
  name: string;
  peers: WebSocket[];
}

const rooms = new Map<string, Room>();

/** Generate a 4-character alphanumeric room code. */
function generateCode(): string {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'; // Exclude confusable: 0/O, 1/I
  let code: string;
  do {
    code = '';
    for (let i = 0; i < 4; i++) {
      code += chars[Math.floor(Math.random() * chars.length)];
    }
  } while (rooms.has(code)); // Ensure unique
  return code;
}

/** Remove a peer from their room and notify the other peer. */
function removePeer(ws: WebSocket) {
  for (const [code, room] of rooms) {
    const idx = room.peers.indexOf(ws);
    if (idx !== -1) {
      room.peers.splice(idx, 1);
      // Notify remaining peer
      for (const peer of room.peers) {
        if (peer.readyState === WebSocket.OPEN) {
          peer.send(JSON.stringify({ type: 'peer_left' }));
        }
      }
      // Clean up empty rooms
      if (room.peers.length === 0) {
        rooms.delete(code);
      }
      return;
    }
  }
}

/** Find the other peer in the same room. */
function getOtherPeer(ws: WebSocket): WebSocket | null {
  for (const room of rooms.values()) {
    const idx = room.peers.indexOf(ws);
    if (idx !== -1) {
      const other = room.peers.find((p, i) => i !== idx && p.readyState === WebSocket.OPEN);
      return other ?? null;
    }
  }
  return null;
}

function handleWebSocket(ws: WebSocket) {
  ws.addEventListener('message', (event) => {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(event.data as string);
    } catch {
      return;
    }

    switch (msg.type) {
      case 'create': {
        const code = generateCode();
        const name = (msg.name as string) || '';
        const room: Room = { code, name, peers: [ws] };
        rooms.set(code, room);
        ws.send(JSON.stringify({ type: 'room_created', code, name }));
        console.log(`[signal] Room created: ${code} "${name}"`);
        break;
      }

      case 'join': {
        const code = ((msg.code as string) || '').toUpperCase();
        const room = rooms.get(code);
        if (!room) {
          ws.send(JSON.stringify({ type: 'error', message: 'Room not found' }));
          return;
        }
        if (room.peers.length >= 2) {
          ws.send(JSON.stringify({ type: 'error', message: 'Room is full' }));
          return;
        }
        room.peers.push(ws);
        ws.send(JSON.stringify({ type: 'room_joined', code, name: room.name }));
        // Notify the creator
        const creator = room.peers[0];
        if (creator && creator.readyState === WebSocket.OPEN) {
          creator.send(JSON.stringify({ type: 'peer_joined' }));
        }
        console.log(`[signal] Peer joined room: ${code}`);
        break;
      }

      case 'sdp_offer':
      case 'sdp_answer':
      case 'ice_candidate': {
        const other = getOtherPeer(ws);
        if (other) {
          other.send(JSON.stringify(msg));
        }
        break;
      }

      default:
        break;
    }
  });

  ws.addEventListener('close', () => {
    removePeer(ws);
  });

  ws.addEventListener('error', () => {
    removePeer(ws);
  });
}

// ── Server ─────────────────────────────────────────────────────────

const port = parseInt(Deno.env.get('PORT') || '8787');

Deno.serve({ port }, (req) => {
  const url = new URL(req.url);

  // Health check
  if (url.pathname === '/health') {
    return new Response('ok', { status: 200 });
  }

  // WebSocket upgrade
  if (url.pathname === '/ws') {
    if (req.headers.get('upgrade')?.toLowerCase() !== 'websocket') {
      return new Response('Expected WebSocket', { status: 400 });
    }
    const { socket, response } = Deno.upgradeWebSocket(req);
    handleWebSocket(socket);
    return response;
  }

  // CORS preflight for browser WebSocket connections
  if (req.method === 'OPTIONS') {
    return new Response(null, {
      status: 204,
      headers: {
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, OPTIONS',
        'Access-Control-Allow-Headers': '*',
      },
    });
  }

  return new Response('Kipukas Signaling Server', { status: 200 });
});

console.log(`[signal] Kipukas signaling server running on :${port}`);
