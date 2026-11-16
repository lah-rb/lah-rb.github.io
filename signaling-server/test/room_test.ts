// Tests for src/room.js — the relay's room rules.
//
// Run from signaling-server/:  deno task test

import {
  admit,
  generateCode,
  isValidCode,
  isValidPlayerId,
  leave,
  namesMatch,
  originAllowed,
  peerOf,
  touch,
} from '../src/room.js';

// ── Tiny assertions (no remote imports, so the tests run offline) ──

function assertEquals<T>(actual: T, expected: T, what = '') {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    throw new Error(`${what ? what + ': ' : ''}expected ${e}, got ${a}`);
  }
}

function assert(cond: unknown, what: string) {
  if (!cond) throw new Error(what);
}

// deno-lint-ignore no-explicit-any
type Result = any;

// The rules return a tagged union (accepted or refused). The tests look at
// both shapes freely, so they call it through a loosely typed alias.
const decide = admit as unknown as (...args: unknown[]) => Result;

const A = 'player-a';
const B = 'player-b';
const C = 'player-c';

function create(player = A, name = 'Table'): Result {
  return decide(
    null,
    { action: 'create', code: 'ABCD', name, player },
    [],
    1000,
  );
}

/** A room with A and B in it. */
function full(): Result {
  const room = create().room;
  return decide(
    room,
    { action: 'join', code: 'ABCD', name: 'table', player: B },
    [A],
    2000,
  ).room;
}

const ids = (room: Result) => room.players.map((p: { id: string }) => p.id);

// ── create ─────────────────────────────────────────────────────────

Deno.test('create starts a room with its creator in it', () => {
  const r = create();
  assert(r.ok, 'accepted');
  assertEquals(r.reply, 'room_created');
  assertEquals(r.room.code, 'ABCD');
  assertEquals(r.room.name, 'Table');
  assertEquals(ids(r.room), [A]);
  assertEquals(r.resumed, false);
});

Deno.test('create is refused when the code is in use, so the relay can pick another', () => {
  const r = decide(
    create().room,
    { action: 'create', code: 'ABCD', name: 'x', player: B },
    [],
    1,
  );
  assertEquals(r.ok, false);
  assertEquals(r.error.code, 'code_taken');
});

Deno.test('room names are trimmed and capped', () => {
  const r = create(A, '  ' + 'n'.repeat(200) + '  ');
  assertEquals(r.room.name.length, 64);
});

// ── join ───────────────────────────────────────────────────────────

Deno.test('join needs the room to exist', () => {
  const r = decide(
    null,
    { action: 'join', code: 'ABCD', name: 'Table', player: B },
    [],
    1,
  );
  assertEquals(r.error.code, 'not_found');
  assertEquals(r.error.message, 'Room not found');
});

Deno.test('join needs the room name, compared without case or outer spaces', () => {
  const room = create().room;
  const wrong = decide(
    room,
    { action: 'join', code: 'ABCD', name: 'Other', player: B },
    [A],
    1,
  );
  assertEquals(wrong.error.code, 'name_mismatch');
  const right = decide(
    room,
    { action: 'join', code: 'ABCD', name: '  tABLE ', player: B },
    [A],
    1,
  );
  assert(right.ok, 'accepted');
  assertEquals(right.reply, 'room_joined');
  assertEquals(ids(right.room), [A, B]);
});

Deno.test('a room without a name lets anyone with the code in', () => {
  const room = create(A, '').room;
  const r = decide(
    room,
    { action: 'join', code: 'ABCD', name: 'anything', player: B },
    [A],
    1,
  );
  assert(r.ok, 'accepted');
});

Deno.test('a third player cannot join while both players are connected', () => {
  const r = decide(
    full(),
    { action: 'join', code: 'ABCD', name: 'Table', player: C },
    [A, B],
    1,
  );
  assertEquals(r.error.code, 'full');
  assertEquals(r.error.message, 'Room is full');
});

Deno.test('a new player takes the place of one who is away, never of one who is connected', () => {
  const r = decide(
    full(),
    { action: 'join', code: 'ABCD', name: 'Table', player: C },
    [A],
    3000,
  );
  assert(r.ok, 'accepted');
  assertEquals(r.evicted, B);
  assertEquals(ids(r.room), [A, C]);
});

Deno.test('when both players are away, the one gone longest gives up their place', () => {
  let room = full();
  room = touch(room, B, 5000); // B left recently
  room = touch(room, A, 4000); // A left earlier
  const r = decide(
    room,
    { action: 'join', code: 'ABCD', name: 'Table', player: C },
    [],
    6000,
  );
  assertEquals(r.evicted, A);
  assertEquals(ids(r.room), [C, B]);
});

Deno.test('taking a place still needs the room name', () => {
  const r = decide(
    full(),
    { action: 'join', code: 'ABCD', name: 'nope', player: C },
    [A],
    1,
  );
  assertEquals(r.error.code, 'name_mismatch');
});

// ── rejoin ─────────────────────────────────────────────────────────

Deno.test('a player already in the room comes back without the name', () => {
  for (const action of ['rejoin', 'join']) {
    const r = decide(
      full(),
      { action, code: 'ABCD', name: '', player: B },
      [A],
      9000,
    );
    assert(r.ok, action);
    assertEquals(r.reply, 'room_joined');
    assertEquals(r.resumed, true);
    assertEquals(ids(r.room), [A, B]);
    assertEquals(r.room.players[1].seen, 9000);
  }
});

Deno.test('a rejoin never fills the room: the player already has their place', () => {
  // Both "connected" from the relay's point of view (the old socket has not
  // closed yet). The returning player is still let in.
  const r = decide(
    full(),
    { action: 'rejoin', code: 'ABCD', name: 'Table', player: B },
    [A, B],
    1,
  );
  assert(r.ok, 'accepted');
});

Deno.test('rejoining a room the relay has forgotten brings it back', () => {
  const r = decide(
    null,
    { action: 'rejoin', code: 'WXYZ', name: 'Table', player: A },
    [],
    7000,
  );
  assert(r.ok, 'accepted');
  assertEquals(r.reply, 'room_joined');
  assertEquals(r.recreated, true);
  assertEquals(r.room.code, 'WXYZ');
  assertEquals(r.room.name, 'Table');
  assertEquals(ids(r.room), [A]);

  // The other player comes back the same way and finds the first.
  const second = decide(
    r.room,
    { action: 'rejoin', code: 'WXYZ', name: 'Table', player: B },
    [A],
    1,
  );
  assert(second.ok, 'second player accepted');
  assertEquals(ids(second.room), [A, B]);
});

Deno.test('rejoin by a player the room has not seen needs the name', () => {
  const room = create().room;
  const wrong = decide(
    room,
    { action: 'rejoin', code: 'ABCD', name: 'guess', player: C },
    [A],
    1,
  );
  assertEquals(wrong.error.code, 'name_mismatch');
  const right = decide(
    room,
    { action: 'rejoin', code: 'ABCD', name: 'Table', player: C },
    [A],
    1,
  );
  assert(right.ok, 'accepted with the name');
});

Deno.test('a player whose place was taken is told the room is full', () => {
  const taken = decide(
    full(),
    { action: 'join', code: 'ABCD', name: 'Table', player: C },
    [A],
    1,
  );
  const back = decide(taken.room, {
    action: 'rejoin',
    code: 'ABCD',
    name: 'Table',
    player: B,
  }, [
    A,
    C,
  ], 1);
  assertEquals(back.error.code, 'full');
});

// ── validation ─────────────────────────────────────────────────────

Deno.test('a missing or malformed player id is refused', () => {
  for (const player of ['', 'has space', 'a:b', 'x'.repeat(65), undefined]) {
    const r = decide(
      null,
      { action: 'create', code: 'ABCD', name: 'T', player } as Result,
      [],
      1,
    );
    assertEquals(r.error.code, 'bad_request', String(player));
  }
  assert(isValidPlayerId('0f3a9c-AB_12'), 'hex-ish id');
});

Deno.test('an unknown action is refused', () => {
  const r = decide(null, { action: 'destroy', code: 'ABCD', player: A }, [], 1);
  assertEquals(r.error.code, 'bad_request');
});

Deno.test('generated codes are 4 characters without 0, O, 1 or I', () => {
  for (let i = 0; i < 500; i++) {
    const code = generateCode();
    assert(isValidCode(code), code);
    assert(!/[0O1I]/.test(code), code);
  }
  assertEquals(generateCode(() => 0), 'AAAA');
  assertEquals(generateCode(() => 0.999999), '9999');
});

Deno.test('code shape is checked loosely so a typo reads as "not found"', () => {
  assert(
    isValidCode('AB0O'),
    'confusable characters still have the right shape',
  );
  assert(!isValidCode('abcd'), 'lower case');
  assert(!isValidCode('ABC'), 'too short');
  assert(!isValidCode('ABCDE'), 'too long');
  assert(!isValidCode('AB-D'), 'punctuation');
});

Deno.test('namesMatch ignores case and outer spaces only', () => {
  assert(namesMatch(' My Room', 'my room '), 'case and spaces');
  assert(!namesMatch('My Room', 'MyRoom'), 'inner spaces matter');
  assert(namesMatch(undefined, ''), 'missing is empty');
});

// ── leave, touch, peerOf ───────────────────────────────────────────

Deno.test('leaving frees the place; the last player leaving ends the room', () => {
  const room = leave(full(), A);
  assertEquals(ids(room), [B]);
  assertEquals(leave(room, B), null);
  assertEquals(leave(null, A), null);
  // Leaving a room you are not in changes nothing.
  assertEquals(ids(leave(full(), C)), [A, B]);
});

Deno.test('after one player leaves, someone else can join', () => {
  const room = leave(full(), B);
  const r = decide(
    room,
    { action: 'join', code: 'ABCD', name: 'Table', player: C },
    [A],
    1,
  );
  assert(r.ok, 'accepted');
  assertEquals(r.evicted, null);
});

Deno.test('peerOf names the other player', () => {
  assertEquals(peerOf(full(), A), B);
  assertEquals(peerOf(full(), B), A);
  assertEquals(peerOf(create().room, A), null);
  assertEquals(peerOf(null, A), null);
});

// ── originAllowed ──────────────────────────────────────────────────

Deno.test('only the site, and local copies of it, may use the relay from a browser', () => {
  const allowed = 'https://www.kipukas.cards, https://kipukas.cards';
  assert(originAllowed('https://www.kipukas.cards', allowed), 'the site');
  assert(
    originAllowed('https://kipukas.cards', allowed),
    'the site without www',
  );
  assert(!originAllowed('https://evil.example', allowed), 'another site');
  assert(
    !originAllowed('https://www.kipukas.cards.evil.example', allowed),
    'lookalike',
  );
  assert(
    !originAllowed('https://localhost.evil.example', allowed),
    'localhost lookalike',
  );

  for (
    const local of [
      'http://localhost:4000',
      'http://127.0.0.1:4173',
      'http://192.168.1.20:4000',
      'http://10.0.0.5:4000',
      'http://172.20.1.1:4000',
      'http://macbook.local:4000',
    ]
  ) {
    assert(originAllowed(local, allowed), local);
  }
  assert(
    !originAllowed('http://172.32.0.1:4000', allowed),
    'not a private range',
  );
  assert(
    !originAllowed('http://11.0.0.1:4000', allowed),
    'not a private range',
  );
});

Deno.test('no Origin header, or no configured list, is allowed', () => {
  assert(
    originAllowed(null, 'https://www.kipukas.cards'),
    'not a browser page',
  );
  assert(originAllowed('https://anything.example', ''), 'not configured');
  assert(
    originAllowed('https://anything.example', undefined),
    'not configured',
  );
});
