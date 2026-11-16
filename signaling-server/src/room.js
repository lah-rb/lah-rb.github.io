/**
 * room.js — the relay's room rules, with no dependency on where they run.
 *
 * A room holds at most two players. Each player has an id of their own
 * choosing (random, made by the page when the room is created or joined) and
 * keeps their place in the room for as long as the room exists, whether or
 * not they are connected at the moment. That is the point of the design: on
 * this site every page change closes the socket, so "connected" is not a
 * useful notion of who is in a room.
 *
 * Everything here is a pure function over plain data, so it is tested
 * directly (test/room_test.ts). src/room-object.js applies it to a Cloudflare
 * Durable Object, one per room.
 *
 * The room as stored:
 *   { code, name, created, players: [{ id, seen }] }
 * `seen` is when the player last connected or disconnected (ms since epoch).
 */

/** Bumped when the relay's messages change. Sent as `v` in room replies. */
export const PROTOCOL_VERSION = 2;

export const MAX_PLAYERS = 2;

/** Room names and ids are capped so a client cannot store arbitrary data. */
const MAX_NAME_LENGTH = 64;

// Letters and digits that are hard to confuse when read aloud or typed:
// no 0/O and no 1/I.
const CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';

/** A 4-character room code. */
export function generateCode(random = Math.random) {
  let code = '';
  for (let i = 0; i < 4; i++) {
    code += CODE_ALPHABET[Math.floor(random() * CODE_ALPHABET.length)];
  }
  return code;
}

/**
 * Whether `code` has the shape of a room code. Deliberately looser than
 * generateCode(): a mistyped code should be told "Room not found", not be
 * rejected as malformed.
 */
export function isValidCode(code) {
  return typeof code === 'string' && /^[A-Z0-9]{4}$/.test(code);
}

/** Player ids are chosen by the page; only a conservative character set is accepted. */
export function isValidPlayerId(id) {
  return typeof id === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(id);
}

/** Room names are the players' shared secret, compared without case or outer spaces. */
export function namesMatch(a, b) {
  return String(a ?? '').trim().toLowerCase() ===
    String(b ?? '').trim().toLowerCase();
}

function cleanName(name) {
  return String(name ?? '').trim().slice(0, MAX_NAME_LENGTH);
}

function refuse(code, message) {
  return { ok: false, error: { code, message } };
}

function accept(reply, room, extra = {}) {
  return {
    ok: true,
    reply,
    room,
    resumed: false,
    recreated: false,
    evicted: null,
    ...extra,
  };
}

function newRoom(code, name, player, now) {
  return {
    code,
    name: cleanName(name),
    created: now,
    players: [{ id: player, seen: now }],
  };
}

function withPlayers(room, players) {
  return { ...room, players };
}

/**
 * Decide what happens when a player connects.
 *
 * @param {object|null} room  The stored room, or null if there is none.
 * @param {{action: string, code: string, name?: string, player: string}} request
 *   `create`  start a room under `code` (the relay picks the code)
 *   `join`    enter an existing room: needs the room name
 *   `rejoin`  come back to a room this player is already in
 * @param {string[]} live  Ids of the players connected right now.
 * @param {number} now  Milliseconds since epoch.
 * @returns {{ok: true, reply: 'room_created'|'room_joined', room: object,
 *            resumed: boolean, recreated: boolean, evicted: string|null}
 *          |{ok: false, error: {code: string, message: string}}}
 *
 * Error codes: bad_request, code_taken, not_found, name_mismatch, full.
 * The messages are what the page shows the player.
 */
export function admit(room, request, live, now) {
  const { action, code, name, player } = request;

  if (!isValidPlayerId(player)) {
    return refuse('bad_request', 'Missing or invalid player id');
  }

  if (action === 'create') {
    if (room) return refuse('code_taken', 'Room code already in use');
    return accept('room_created', newRoom(code, name, player, now));
  }

  if (action !== 'join' && action !== 'rejoin') {
    return refuse('bad_request', 'Unknown action');
  }

  if (!room) {
    if (action === 'join') return refuse('not_found', 'Room not found');
    // The relay no longer has this room: it expired, or its storage was
    // lost. The players hold all the game state, so the room simply comes
    // back under the same code and the other player rejoins it the same way.
    return accept('room_joined', newRoom(code, name, player, now), {
      resumed: true,
      recreated: true,
    });
  }

  // Already one of the room's players: welcome back, whichever action was
  // used and whether or not they remember the name.
  if (room.players.some((p) => p.id === player)) {
    const players = room.players.map((
      p,
    ) => (p.id === player ? { ...p, seen: now } : p));
    return accept('room_joined', withPlayers(room, players), { resumed: true });
  }

  // A player this room has not seen must know the room name.
  if (room.name && !namesMatch(name, room.name)) {
    return refuse('name_mismatch', 'Room name does not match');
  }

  if (room.players.length < MAX_PLAYERS) {
    return accept(
      'room_joined',
      withPlayers(room, [...room.players, { id: player, seen: now }]),
    );
  }

  // The room is full. A place whose player is away can be taken over: that
  // is how someone who lost their saved session (cleared storage, a different
  // browser) gets back into their own game by joining again. A player who is
  // connected is never displaced. If both are away, the one gone longest goes.
  const away = room.players
    .filter((p) => !live.includes(p.id))
    .sort((a, b) => a.seen - b.seen)[0];
  if (!away) return refuse('full', 'Room is full');

  const players = room.players.map((
    p,
  ) => (p.id === away.id ? { id: player, seen: now } : p));
  return accept('room_joined', withPlayers(room, players), {
    evicted: away.id,
  });
}

/** Remove a player for good. Returns the room without them, or null if it is now empty. */
export function leave(room, player) {
  if (!room) return null;
  const players = room.players.filter((p) => p.id !== player);
  return players.length === 0 ? null : withPlayers(room, players);
}

/** Record when a player was last seen (on disconnect). */
export function touch(room, player, now) {
  if (!room) return null;
  return withPlayers(
    room,
    room.players.map((p) => (p.id === player ? { ...p, seen: now } : p)),
  );
}

/** The other player's id, or null if this player is alone in the room. */
export function peerOf(room, player) {
  const other = room?.players.find((p) => p.id !== player);
  return other ? other.id : null;
}

/**
 * Whether a page at `origin` may use the relay.
 *
 * This only stops other websites from using the relay through their
 * visitors' browsers. It is not authentication: anything that is not a
 * browser page can send any Origin it likes, or none.
 *
 * @param {string|null} origin   The request's Origin header.
 * @param {string|undefined} allowed  Comma-separated origins. Empty means any.
 */
export function originAllowed(origin, allowed) {
  if (!origin) return true; // not a browser page
  const list = String(allowed ?? '').split(',').map((s) => s.trim()).filter(
    Boolean,
  );
  if (list.length === 0) return true; // not configured
  if (list.includes(origin)) return true;
  // A copy of the site being worked on: this machine, or a machine on the
  // local network (the dev server is reached from phones by its LAN address).
  return LOCAL_ORIGIN.test(origin);
}

const LOCAL_ORIGIN = new RegExp(
  '^https?://(' +
    'localhost|127\\.0\\.0\\.1|\\[::1\\]|' +
    '10\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}|' +
    '192\\.168\\.\\d{1,3}\\.\\d{1,3}|' +
    '172\\.(1[6-9]|2\\d|3[01])\\.\\d{1,3}\\.\\d{1,3}|' +
    '[a-z0-9-]+\\.local' +
    ')(:\\d+)?$',
  'i',
);
