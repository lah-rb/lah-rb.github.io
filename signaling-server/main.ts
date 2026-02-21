/**
 * Kipukas Signaling Server — WebSocket relay for WebRTC connection brokering.
 *
 * Purpose: Only brokers WebRTC connections. Does NOT process game logic.
 * Deploy: Deno Deploy (free tier) or run locally with `deno run --allow-net main.ts`
 *
 * Protocol:
 *   Client → { type: "create", name: "My Room" }
 *     Server → { type: "room_created", code: "ABCD", name: "My Room" }
 *   Client → { type: "join", code: "ABCD", name: "My Room" }
 *     Server → { type: "room_joined", code: "ABCD", name: "My Room" }
 *     Server → (to creator) { type: "peer_joined" }
 *   Client → { type: "rejoin", code: "ABCD" }
 *     Server → { type: "room_joined", code: "ABCD", name: "My Room" }
 *     Server → (to other peer) { type: "peer_joined" }
 *   Client → { type: "sdp_offer"|"sdp_answer"|"ice_candidate", data: ... }
 *     Server → relays to the other peer in the room
 *   On disconnect: grace period (15s), then notify remaining peer { type: "peer_left" }
 */

interface Room {
  code: string;
  name: string;
  peers: WebSocket[];
  /** Timers for grace-period cleanup when a peer disconnects temporarily. */
  graceTimers: Map<WebSocket, number>;
  /** Slots held for peers during grace period (keeps room from being "full"). */
  graceSlots: number;
}

const rooms = new Map<string, Room>();

/** Grace period (ms) before removing a disconnected peer from the room. */
const GRACE_PERIOD_MS = 15_000;

/** Generate a 4-character alphanumeric room code. */
function generateCode(): string {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // Exclude confusable: 0/O, 1/I
  let code: string;
  do {
    code = "";
    for (let i = 0; i < 4; i++) {
      code += chars[Math.floor(Math.random() * chars.length)];
    }
  } while (rooms.has(code)); // Ensure unique
  return code;
}

/** Remove a peer from their room (with grace period). */
function removePeer(ws: WebSocket) {
  for (const [code, room] of rooms) {
    const idx = room.peers.indexOf(ws);
    if (idx !== -1) {
      room.peers.splice(idx, 1);

      // Start a grace period instead of immediately notifying/cleaning up
      const timer = setTimeout(() => {
        room.graceTimers.delete(ws);
        room.graceSlots = Math.max(0, room.graceSlots - 1);

        // Notify remaining peers that the grace period expired
        for (const peer of room.peers) {
          if (peer.readyState === WebSocket.OPEN) {
            peer.send(JSON.stringify({ type: "peer_left" }));
          }
        }
        // Clean up empty rooms (no active peers and no grace slots)
        if (room.peers.length === 0 && room.graceSlots === 0) {
          rooms.delete(code);
          console.log(`[signal] Room ${code} cleaned up (empty after grace)`);
        }
      }, GRACE_PERIOD_MS);

      room.graceTimers.set(ws, timer);
      room.graceSlots += 1;
      console.log(
        `[signal] Peer disconnected from ${code}, grace period started (${GRACE_PERIOD_MS}ms)`,
      );
      return;
    }
  }
}

/** Find the other peer in the same room. */
function getOtherPeer(ws: WebSocket): WebSocket | null {
  for (const room of rooms.values()) {
    const idx = room.peers.indexOf(ws);
    if (idx !== -1) {
      const other = room.peers.find((p, i) =>
        i !== idx && p.readyState === WebSocket.OPEN
      );
      return other ?? null;
    }
  }
  return null;
}

/** Case-insensitive room name comparison for co-authentication. */
function namesMatch(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

function handleWebSocket(ws: WebSocket) {
  ws.addEventListener("message", (event) => {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(event.data as string);
    } catch {
      return;
    }

    switch (msg.type) {
      case "create": {
        const code = generateCode();
        const name = (msg.name as string) || "";
        const room: Room = {
          code,
          name,
          peers: [ws],
          graceTimers: new Map(),
          graceSlots: 0,
        };
        rooms.set(code, room);
        ws.send(JSON.stringify({ type: "room_created", code, name }));
        console.log(`[signal] Room created: ${code} "${name}"`);
        break;
      }

      case "join": {
        const code = ((msg.code as string) || "").toUpperCase();
        const name = (msg.name as string) || "";
        const room = rooms.get(code);
        if (!room) {
          ws.send(JSON.stringify({ type: "error", message: "Room not found" }));
          return;
        }
        // Co-authenticate: room name must match (if the room has a name set)
        if (room.name && !namesMatch(name, room.name)) {
          ws.send(
            JSON.stringify({
              type: "error",
              message: "Room name does not match",
            }),
          );
          return;
        }
        // Clean up stale peers (closed connections whose close event hasn't fired yet)
        room.peers = room.peers.filter((p) => p.readyState === WebSocket.OPEN);
        if (room.peers.length >= 2) {
          ws.send(JSON.stringify({ type: "error", message: "Room is full" }));
          return;
        }
        // Cancel any grace timer (this might be the same user reconnecting)
        for (const [oldWs, timer] of room.graceTimers) {
          clearTimeout(timer);
          room.graceTimers.delete(oldWs);
          room.graceSlots = Math.max(0, room.graceSlots - 1);
        }
        room.peers.push(ws);
        ws.send(JSON.stringify({ type: "room_joined", code, name: room.name }));
        // Notify the other peer (if any)
        for (const peer of room.peers) {
          if (peer !== ws && peer.readyState === WebSocket.OPEN) {
            peer.send(JSON.stringify({ type: "peer_joined" }));
          }
        }
        console.log(`[signal] Peer joined room: ${code}`);
        break;
      }

      case "rejoin": {
        // Rejoin is like join but skips name validation (peer already authenticated).
        // Used for automatic reconnection after page navigation.
        const code = ((msg.code as string) || "").toUpperCase();
        const room = rooms.get(code);
        if (!room) {
          ws.send(
            JSON.stringify({
              type: "error",
              message: "Room not found (expired)",
            }),
          );
          return;
        }
        // Clean up stale peers (closed connections whose close event hasn't fired yet)
        room.peers = room.peers.filter((p) => p.readyState === WebSocket.OPEN);
        if (room.peers.length >= 2) {
          ws.send(JSON.stringify({ type: "error", message: "Room is full" }));
          return;
        }
        // Cancel any grace timers
        for (const [oldWs, timer] of room.graceTimers) {
          clearTimeout(timer);
          room.graceTimers.delete(oldWs);
          room.graceSlots = Math.max(0, room.graceSlots - 1);
        }
        room.peers.push(ws);
        ws.send(JSON.stringify({ type: "room_joined", code, name: room.name }));
        // Notify the other peer
        for (const peer of room.peers) {
          if (peer !== ws && peer.readyState === WebSocket.OPEN) {
            peer.send(JSON.stringify({ type: "peer_joined" }));
          }
        }
        console.log(`[signal] Peer rejoined room: ${code}`);
        break;
      }

      case "sdp_offer":
      case "sdp_answer":
      case "ice_candidate": {
        const other = getOtherPeer(ws);
        if (other) {
          console.log(`[signal] Relaying ${msg.type} to peer`);
          other.send(JSON.stringify(msg));
        } else {
          console.warn(`[signal] Cannot relay ${msg.type}: no other peer found in room`);
        }
        break;
      }

      default:
        break;
    }
  });

  ws.addEventListener("close", () => {
    removePeer(ws);
  });

  ws.addEventListener("error", () => {
    removePeer(ws);
  });
}

// ── Cloudflare TURN credential generation ──────────────────────────

const CF_TURN_KEY_ID = Deno.env.get("CF_TURN_KEY_ID") || "";
const CF_TURN_API_TOKEN = Deno.env.get("CF_TURN_API_TOKEN") || "";

/** Cache TURN credentials to avoid hitting the API on every request. */
let cachedTurnCreds: { iceServers: unknown; expiry: number } | null = null;

async function getTurnCredentials(): Promise<unknown> {
  const now = Date.now();
  // Return cached if still valid (refresh 5 min before expiry)
  if (cachedTurnCreds && cachedTurnCreds.expiry > now + 300_000) {
    return cachedTurnCreds.iceServers;
  }

  if (!CF_TURN_KEY_ID || !CF_TURN_API_TOKEN) {
    console.warn("[signal] TURN credentials not configured (missing env vars)");
    return null;
  }

  try {
    const ttl = 86400; // 24 hours
    const resp = await fetch(
      `https://rtc.live.cloudflare.com/v1/turn/keys/${CF_TURN_KEY_ID}/credentials/generate`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${CF_TURN_API_TOKEN}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ ttl }),
      },
    );

    if (!resp.ok) {
      console.error("[signal] Cloudflare TURN API error:", resp.status, await resp.text());
      return null;
    }

    const data = await resp.json();
    cachedTurnCreds = {
      iceServers: data.iceServers,
      expiry: now + ttl * 1000,
    };
    console.log("[signal] TURN credentials generated, valid for", ttl, "seconds");
    return data.iceServers;
  } catch (err) {
    console.error("[signal] Failed to fetch TURN credentials:", err);
    return null;
  }
}

// ── CORS helper ────────────────────────────────────────────────────

const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "*",
};

// ── Server ─────────────────────────────────────────────────────────

const port = parseInt(Deno.env.get("PORT") || "8787");

Deno.serve({ port }, async (req) => {
  const url = new URL(req.url);

  // CORS preflight
  if (req.method === "OPTIONS") {
    return new Response(null, { status: 204, headers: CORS_HEADERS });
  }

  // Health check
  if (url.pathname === "/health") {
    return new Response("ok", { status: 200 });
  }

  // TURN credentials endpoint
  if (url.pathname === "/turn-credentials" && req.method === "GET") {
    const iceServers = await getTurnCredentials();
    if (!iceServers) {
      return new Response(JSON.stringify({ iceServers: null }), {
        status: 200,
        headers: { "Content-Type": "application/json", ...CORS_HEADERS },
      });
    }
    return new Response(JSON.stringify({ iceServers }), {
      status: 200,
      headers: { "Content-Type": "application/json", ...CORS_HEADERS },
    });
  }

  // WebSocket upgrade
  if (url.pathname === "/ws") {
    if (req.headers.get("upgrade")?.toLowerCase() !== "websocket") {
      return new Response("Expected WebSocket", { status: 400 });
    }
    const { socket, response } = Deno.upgradeWebSocket(req);
    handleWebSocket(socket);
    return response;
  }

  return new Response("Kipukas Signaling Server", { status: 200 });
});

console.log(`[signal] Kipukas signaling server running on :${port}`);
