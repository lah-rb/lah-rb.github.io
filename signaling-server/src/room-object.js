/**
 * room-object.js — one room, as a Cloudflare Durable Object.
 *
 * Cloudflare runs exactly one instance of this object per room code and
 * routes every connection for that code to it, so the two players always
 * meet in the same place. The rules for who may enter are in room.js; this
 * file applies them to sockets and storage.
 *
 * What makes the room durable:
 *
 * - The room (its name and its two players) is in the object's storage, not
 *   in memory. Both players can be disconnected for hours (two locked
 *   phones) and the room is still there when they come back.
 * - Sockets use the hibernation API: between messages the object is evicted
 *   from memory while the sockets stay open. Nothing here may live in an
 *   instance field; who a socket belongs to is kept in its attachment.
 * - A room with nobody connected is deleted after ROOM_TTL_SECONDS. A player
 *   who returns after that simply recreates it (see `admit` in room.js).
 *
 * Messages the relay sends:
 *   room_created  { code, name, v, peer }             reply to create
 *   room_joined   { code, name, v, peer, resumed, recreated }  reply to join / rejoin
 *                 peer is 'present' | 'away' | 'none'
 *   peer_joined   { peerId }   both players are connected (sent to both)
 *   peer_away     {}           the other player's connection closed
 *   peer_left     {}           the other player left the room for good
 *   superseded    {}           this player connected again elsewhere; this socket is closed
 *   relay         { data }     a message from the other player, passed on untouched
 *   pong          {}           reply to ping
 *   error         { code, message }   then the socket is closed
 *
 * Messages a player sends:
 *   relay { data }   pass `data` to the other player
 *   leave {}         give up this player's place in the room
 *   ping  {}         heartbeat
 */

import { DurableObject } from 'cloudflare:workers';
import { admit, leave, peerOf, PROTOCOL_VERSION, touch } from './room.js';

const ROOM_KEY = 'room';
const DEFAULT_ROOM_TTL_SECONDS = 12 * 60 * 60;

// WebSocket close codes in the application range.
const CLOSE_SUPERSEDED = 4001;
const CLOSE_REFUSED = 4003;

const SOCKET_OPEN = 1;

const PING = JSON.stringify({ type: 'ping' });
const PONG = JSON.stringify({ type: 'pong' });

function send(ws, message) {
  try {
    ws.send(JSON.stringify(message));
  } catch (_) { /* the socket is closing */ }
}

function close(ws, code, reason) {
  try {
    ws.close(code, reason);
  } catch (_) { /* already closed */ }
}

export class Room extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    // Heartbeats are answered by the runtime without waking this object.
    this.ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair(PING, PONG));
  }

  // ── Who is connected ─────────────────────────────────────────────

  /** The player a socket belongs to (set when it was accepted). */
  playerOf(ws) {
    return ws.deserializeAttachment()?.player ?? null;
  }

  /** Open sockets, optionally leaving one out (the one that is closing). */
  openSockets(except = null) {
    return this.ctx.getWebSockets().filter((ws) => ws !== except && ws.readyState === SOCKET_OPEN);
  }

  /** Ids of the players with at least one open socket. */
  livePlayers(except = null) {
    return [...new Set(this.openSockets(except).map((ws) => this.playerOf(ws)).filter(Boolean))];
  }

  /** Open sockets that belong to anyone but `player`. */
  socketsOfOthers(player, except = null) {
    return this.openSockets(except).filter((ws) => this.playerOf(ws) !== player);
  }

  roomTtlMs() {
    const seconds = Number(this.env.ROOM_TTL_SECONDS);
    return (Number.isFinite(seconds) && seconds > 0 ? seconds : DEFAULT_ROOM_TTL_SECONDS) * 1000;
  }

  // ── A player connects ────────────────────────────────────────────

  async fetch(request) {
    const params = new URL(request.url).searchParams;
    const wanted = {
      action: params.get('action') ?? '',
      code: params.get('code') ?? '',
      name: params.get('name') ?? '',
      player: params.get('player') ?? '',
    };

    const stored = (await this.ctx.storage.get(ROOM_KEY)) ?? null;
    const result = admit(stored, wanted, this.livePlayers(), Date.now());

    // The worker picked a code that is in use. It tries another; no socket yet.
    if (!result.ok && result.error.code === 'code_taken') {
      return new Response('Room code in use', { status: 409 });
    }

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];

    if (!result.ok) {
      // Refusing the upgrade would leave the page with no reason at all, so
      // the socket is opened just long enough to say why.
      server.accept();
      send(server, { type: 'error', ...result.error });
      close(server, CLOSE_REFUSED, result.error.code);
      return new Response(null, { status: 101, webSocket: client });
    }

    const room = result.room;
    await this.ctx.storage.put(ROOM_KEY, room);
    await this.ctx.storage.deleteAlarm(); // someone is here: the room is not expiring

    // One connection per player; the newest wins. A second tab, or a page
    // load whose predecessor's socket has not finished closing, replaces the
    // earlier socket instead of being turned away.
    for (const older of this.ctx.getWebSockets(wanted.player)) {
      send(older, { type: 'superseded' });
      close(older, CLOSE_SUPERSEDED, 'superseded');
    }

    this.ctx.acceptWebSocket(server, [wanted.player]);
    server.serializeAttachment({ player: wanted.player });

    const peerId = peerOf(room, wanted.player);
    const peerLive = peerId !== null && this.livePlayers().includes(peerId);

    send(server, {
      type: result.reply,
      code: room.code,
      name: room.name,
      v: PROTOCOL_VERSION,
      resumed: result.resumed,
      recreated: result.recreated,
      peer: peerId === null ? 'none' : peerLive ? 'present' : 'away',
    });

    if (peerLive) {
      // Both players are connected: tell each who the other is. This is what
      // starts their state handshake, so it is sent on every (re)connect.
      send(server, { type: 'peer_joined', peerId });
      for (const ws of this.socketsOfOthers(wanted.player)) {
        send(ws, { type: 'peer_joined', peerId: wanted.player });
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  // ── Messages ─────────────────────────────────────────────────────

  async webSocketMessage(ws, message) {
    if (typeof message !== 'string') return;
    const player = this.playerOf(ws);
    if (!player) return;

    let type;
    try {
      type = JSON.parse(message).type;
    } catch (_) {
      return;
    }

    if (type === 'relay') {
      // Passed on exactly as received: the relay never looks inside.
      for (const other of this.socketsOfOthers(player)) {
        try {
          other.send(message);
        } catch (_) { /* closing */ }
      }
    } else if (type === 'leave') {
      await this.playerLeft(player, ws);
    } else if (type === 'ping') {
      // A ping that is not byte-for-byte the canonical one misses the
      // automatic reply above.
      try {
        ws.send(PONG);
      } catch (_) { /* closing */ }
    }
  }

  /** A player gave up their place. */
  async playerLeft(player, ws) {
    const room = leave((await this.ctx.storage.get(ROOM_KEY)) ?? null, player);
    for (const other of this.socketsOfOthers(player)) send(other, { type: 'peer_left' });
    for (const mine of this.ctx.getWebSockets(player)) close(mine, 1000, 'left');
    if (room) {
      await this.ctx.storage.put(ROOM_KEY, room);
      await this.expireIfEmpty(ws);
    } else {
      await this.ctx.storage.deleteAll();
    }
  }

  // ── A connection ends ────────────────────────────────────────────

  async webSocketClose(ws, _code, _reason, _wasClean) {
    close(ws, 1000, 'closed'); // complete the closing handshake
    await this.socketGone(ws);
  }

  async webSocketError(ws, _error) {
    close(ws, 1011, 'error');
    await this.socketGone(ws);
  }

  async socketGone(ws) {
    const player = this.playerOf(ws);
    if (!player) return;

    // The player may still be here on a newer socket (this one was
    // superseded). Then nothing has changed for the other player.
    if (!this.livePlayers(ws).includes(player)) {
      const stored = (await this.ctx.storage.get(ROOM_KEY)) ?? null;
      if (stored && stored.players.some((p) => p.id === player)) {
        await this.ctx.storage.put(ROOM_KEY, touch(stored, player, Date.now()));
        for (const other of this.socketsOfOthers(player, ws)) send(other, { type: 'peer_away' });
      }
    }
    await this.expireIfEmpty(ws);
  }

  /** With nobody connected, the room is kept for a while and then deleted. */
  async expireIfEmpty(except = null) {
    if (this.openSockets(except).length === 0) {
      await this.ctx.storage.setAlarm(Date.now() + this.roomTtlMs());
    }
  }

  async alarm() {
    if (this.openSockets().length === 0) {
      await this.ctx.storage.deleteAll();
    }
  }
}
