/**
 * worker.js — the relay's front door, a Cloudflare Worker.
 *
 * The relay lets two players' pages exchange messages. It forwards them
 * without looking inside: all game logic runs in the players' browsers.
 *
 * A page connects with a WebSocket and says what it wants in the address:
 *
 *   /ws?action=create&name=<room name>&player=<id>
 *   /ws?action=join&code=<code>&name=<room name>&player=<id>
 *   /ws?action=rejoin&code=<code>&name=<room name>&player=<id>
 *
 * The intent is in the address, not in a first message, because the worker
 * has to hand the connection to the right room before any message flows.
 * Each room is a Durable Object (src/room-object.js) found by its code.
 *
 * Also: GET /health → "ok".
 */

import { generateCode, isValidCode, originAllowed } from './room.js';

export { Room } from './room-object.js';

/** How many codes to try before giving up on creating a room. */
const CREATE_ATTEMPTS = 8;

const CLOSE_REFUSED = 4003;

/** Hand the connection to the room with this code. */
function toRoom(env, request, url, code) {
  const target = new URL(url);
  target.searchParams.set('code', code);
  const room = env.ROOM.get(env.ROOM.idFromName(code));
  return room.fetch(new Request(target, request));
}

/**
 * Open the socket just long enough to say why it is being closed. A refused
 * upgrade would give the page no reason at all.
 */
function refuse(code, message) {
  const pair = new WebSocketPair();
  pair[1].accept();
  pair[1].send(JSON.stringify({ type: 'error', code, message }));
  pair[1].close(CLOSE_REFUSED, code);
  return new Response(null, { status: 101, webSocket: pair[0] });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (url.pathname === '/health') return new Response('ok');
    if (url.pathname !== '/ws') return new Response('Kipukas relay');

    if ((request.headers.get('Upgrade') ?? '').toLowerCase() !== 'websocket') {
      return new Response('Expected WebSocket', { status: 426 });
    }
    if (!originAllowed(request.headers.get('Origin'), env.ALLOWED_ORIGINS)) {
      return new Response('Origin not allowed', { status: 403 });
    }

    const action = url.searchParams.get('action');

    if (action === 'create') {
      // The relay chooses the code. A room that already has it answers 409
      // before opening a socket, and another code is tried.
      for (let attempt = 0; attempt < CREATE_ATTEMPTS; attempt++) {
        const response = await toRoom(env, request, url, generateCode());
        if (response.status !== 409) return response;
      }
      return refuse('unavailable', 'Could not create a room. Try again.');
    }

    if (action === 'join' || action === 'rejoin') {
      const code = (url.searchParams.get('code') ?? '').trim().toUpperCase();
      if (!isValidCode(code)) return refuse('not_found', 'Room not found');
      return toRoom(env, request, url, code);
    }

    return refuse('bad_request', 'Unknown action');
  },
};
