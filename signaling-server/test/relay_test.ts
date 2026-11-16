// Tests for the relay as a whole: the worker and its room objects running
// locally (`wrangler dev`), driven over real WebSockets.
//
// Run from signaling-server/:  deno task test
//
// The local relay is started once with a 2-second room lifetime so that
// expiry can be tested, and with its own throwaway storage.

import { createLink } from '../../assets/js/kipukas-link.js';

// ── Tiny assertions (no remote imports, so the tests run offline) ──

function assertEquals<T>(actual: T, expected: T, what = '') {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what ? what + ': ' : ''}expected ${e}, got ${a}`);
}

function assert(cond: unknown, what: string) {
  if (!cond) throw new Error(what);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function until(cond: () => boolean, what: string, ms = 5000) {
  const start = Date.now();
  while (!cond()) {
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

// ── The local relay ────────────────────────────────────────────────

const ROOM_TTL_SECONDS = 2;

function freePort(): number {
  const listener = Deno.listen({ port: 0 });
  const port = (listener.addr as Deno.NetAddr).port;
  listener.close();
  return port;
}

async function startRelay() {
  const port = freePort();
  const storage = await Deno.makeTempDir({ prefix: 'kipukas-relay-test-' });
  const cwd = new URL('..', import.meta.url).pathname;
  const child = new Deno.Command(Deno.execPath(), {
    args: [
      'run',
      '-A',
      'npm:wrangler@4',
      'dev',
      '--port',
      String(port),
      '--ip',
      '127.0.0.1',
      '--persist-to',
      storage,
      '--var',
      `ROOM_TTL_SECONDS:${ROOM_TTL_SECONDS}`,
      '--show-interactive-dev-session=false',
    ],
    cwd,
    stdin: 'null',
    stdout: 'null',
    stderr: 'null',
  }).spawn();

  const base = `http://127.0.0.1:${port}`;
  const start = Date.now();
  for (;;) {
    try {
      const r = await fetch(`${base}/health`);
      const text = await r.text();
      if (text === 'ok') break;
    } catch (_) { /* not up yet */ }
    if (Date.now() - start > 60_000) {
      child.kill();
      throw new Error('local relay did not start');
    }
    await sleep(200);
  }

  return {
    ws: `ws://127.0.0.1:${port}/ws`,
    http: base,
    async stop() {
      try {
        child.kill('SIGTERM');
      } catch (_) { /* already gone */ }
      await child.status;
      await Deno.remove(storage, { recursive: true }).catch(() => {});
    },
  };
}

// ── A player's socket ──────────────────────────────────────────────

type Msg = Record<string, unknown>;

class Player {
  msgs: Msg[] = [];
  closed: { code: number; reason: string } | null = null;
  private ws: WebSocket;

  constructor(base: string, params: Record<string, string>) {
    const url = new URL(base);
    for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
    this.ws = new WebSocket(url);
    this.ws.onmessage = (e) => this.msgs.push(JSON.parse(e.data));
    this.ws.onclose = (e) => (this.closed = { code: e.code, reason: e.reason });
    this.ws.onerror = () => {};
  }

  count(type: string) {
    return this.msgs.filter((m) => m.type === type).length;
  }
  last(type: string): Msg {
    return [...this.msgs].reverse().find((m) => m.type === type) ?? {};
  }
  async got(type: string, n = 1, ms = 5000) {
    await until(
      () => this.count(type) >= n,
      `${type} x${n} (have ${JSON.stringify(this.msgs)})`,
      ms,
    );
    return this.last(type);
  }
  send(msg: Msg | string) {
    this.ws.send(typeof msg === 'string' ? msg : JSON.stringify(msg));
  }
  async close() {
    if (!this.closed) this.ws.close();
    await until(() => this.closed !== null, 'socket close');
  }
}

let nextId = 0;
const newId = () => `player-${++nextId}`;

// ── Tests ──────────────────────────────────────────────────────────

Deno.test('relay', async (t) => {
  const relay = await startRelay();
  const open: Player[] = [];
  const player = (params: Record<string, string>) => {
    const p = new Player(relay.ws, params);
    open.push(p);
    return p;
  };
  /** Create a room and return its creator, their id and the code. */
  const createRoom = async (name = 'Table') => {
    const id = newId();
    const p = player({ action: 'create', name, player: id });
    const reply = await p.got('room_created');
    return { p, id, code: reply.code as string };
  };
  /** A room with two connected players. */
  const pair = async () => {
    const a = await createRoom();
    const idB = newId();
    const b = player({ action: 'join', code: a.code, name: 'Table', player: idB });
    await b.got('peer_joined');
    await a.p.got('peer_joined');
    return { a: a.p, idA: a.id, b, idB, code: a.code };
  };

  try {
    await t.step('health check answers', async () => {
      const r = await fetch(`${relay.http}/health`);
      assertEquals(await r.text(), 'ok');
      const plain = await fetch(`${relay.http}/ws?action=create&player=x`);
      assertEquals(plain.status, 426, 'a plain request to /ws is told to upgrade');
      await plain.body?.cancel();
    });

    await t.step('create gives a 4-character code and the protocol version', async () => {
      const { p, code } = await createRoom('My Room');
      const reply = p.last('room_created');
      assert(/^[A-Z2-9]{4}$/.test(code), `code ${code}`);
      assertEquals(reply.name, 'My Room');
      assertEquals(reply.v, 2);
      assertEquals(reply.peer, 'none');
    });

    await t.step('join is refused, with a reason, for a wrong code or a wrong name', async () => {
      const { code } = await createRoom();
      const missing = player({ action: 'join', code: 'ZZZZ', name: 'Table', player: newId() });
      assertEquals((await missing.got('error')).code, 'not_found');
      assertEquals(missing.last('error').message, 'Room not found');
      await until(() => missing.closed !== null, 'refused socket closes');
      assertEquals(missing.closed!.code, 4003);

      const wrongName = player({ action: 'join', code, name: 'Chair', player: newId() });
      assertEquals((await wrongName.got('error')).code, 'name_mismatch');

      const malformed = player({ action: 'join', code: 'nope!', name: 'Table', player: newId() });
      assertEquals((await malformed.got('error')).code, 'not_found');

      const noId = player({ action: 'join', code, name: 'Table', player: '' });
      assertEquals((await noId.got('error')).code, 'bad_request');
    });

    await t.step('when the second player joins, each is told who the other is', async () => {
      const { a, idA, b, idB } = await pair();
      assertEquals(b.last('room_joined').peer, 'present');
      assertEquals(b.last('room_joined').v, 2);
      assertEquals(b.last('peer_joined').peerId, idA);
      assertEquals(a.last('peer_joined').peerId, idB);
      // Codes are case-insensitive for the joiner.
      const { code } = await createRoom();
      const lower = player({
        action: 'join',
        code: code.toLowerCase(),
        name: 'TABLE',
        player: newId(),
      });
      await lower.got('room_joined');
    });

    await t.step('relay passes a message to the other player only, untouched', async () => {
      const { a, b } = await pair();
      a.send({ type: 'relay', data: { type: 'yrs_update', update: 'AAEC', n: [1, 2] } });
      const got = await b.got('relay');
      assertEquals(got, { type: 'relay', data: { type: 'yrs_update', update: 'AAEC', n: [1, 2] } });
      b.send({ type: 'relay', data: { type: 'hello' } });
      await a.got('relay');
      await sleep(150);
      assertEquals(a.count('relay'), 1, 'A does not get its own message back');
      assertEquals(b.count('relay'), 1, 'B does not get its own message back');
    });

    await t.step(
      'a player is told at once when the other disconnects, and when they return',
      async () => {
        const { a, b, idB, code } = await pair();
        const before = Date.now();
        await b.close();
        await a.got('peer_away');
        assert(Date.now() - before < 2000, 'peer_away arrives promptly');

        const b2 = player({ action: 'rejoin', code, name: 'Table', player: idB });
        const reply = await b2.got('room_joined');
        assertEquals(reply.resumed, true);
        assertEquals(reply.recreated, false);
        assertEquals(reply.peer, 'present');
        await b2.got('peer_joined');
        await a.got('peer_joined', 2);
      },
    );

    await t.step('a returning player sees that the other is away', async () => {
      const { a, b, idB, code } = await pair();
      await a.close();
      await b.got('peer_away');
      await b.close();
      const b2 = player({ action: 'rejoin', code, name: '', player: idB });
      assertEquals((await b2.got('room_joined')).peer, 'away');
      await sleep(150);
      assertEquals(b2.count('peer_joined'), 0);
    });

    await t.step('a second connection by the same player replaces the first', async () => {
      const { a, b, idB, code } = await pair();
      const b2 = player({ action: 'rejoin', code, name: 'Table', player: idB });
      await b2.got('room_joined');
      await b.got('superseded');
      await until(() => b.closed !== null, 'superseded socket closes');
      assertEquals(b.closed!.code, 4001);
      await sleep(300);
      assertEquals(a.count('peer_away'), 0, 'the other player is not told they left');
      // Messages now go to the new socket only.
      a.send({ type: 'relay', data: { n: 1 } });
      await b2.got('relay');
      assertEquals(b.count('relay'), 0);
    });

    await t.step('a third player is refused while both are connected', async () => {
      const { code } = await pair();
      const c = player({ action: 'join', code, name: 'Table', player: newId() });
      assertEquals((await c.got('error')).code, 'full');
      assertEquals(c.last('error').message, 'Room is full');
    });

    await t.step('a new player can take the place of one who is away', async () => {
      const { a, b, idA, code } = await pair();
      await a.close();
      await b.got('peer_away');
      const idC = newId();
      const c = player({ action: 'join', code, name: 'Table', player: idC });
      await c.got('room_joined');
      assertEquals((await b.got('peer_joined', 2)).peerId, idC);
      // The displaced player finds the room full.
      const back = player({ action: 'rejoin', code, name: 'Table', player: idA });
      assertEquals((await back.got('error')).code, 'full');
    });

    await t.step('leaving tells the other player and frees the place', async () => {
      const { a, b, code } = await pair();
      b.send({ type: 'leave' });
      await a.got('peer_left');
      await until(() => b.closed !== null, 'leaver socket closes');
      const c = player({ action: 'join', code, name: 'Table', player: newId() });
      await c.got('peer_joined');
      await a.got('peer_joined', 2);
    });

    await t.step('pings are answered', async () => {
      const { p } = await createRoom();
      p.send('{"type":"ping"}'); // the exact form kipukas-link.js sends
      await p.got('pong');
      p.send({ type: 'ping', t: 123 });
      await p.got('pong', 2);
    });

    await t.step('rejoining a room the relay does not have brings it back', async () => {
      const [idA, idB] = [newId(), newId()];
      const a = player({ action: 'rejoin', code: 'QQ77', name: 'Table', player: idA });
      const reply = await a.got('room_joined');
      assertEquals(reply.recreated, true);
      assertEquals(reply.code, 'QQ77');
      assertEquals(reply.peer, 'none');
      const b = player({ action: 'rejoin', code: 'QQ77', name: 'Table', player: idB });
      assertEquals((await b.got('room_joined')).recreated, false);
      assertEquals((await b.got('peer_joined')).peerId, idA);
      assertEquals((await a.got('peer_joined')).peerId, idB);
      // A stranger still needs the name.
      await b.close();
      const stranger = player({ action: 'rejoin', code: 'QQ77', name: 'guess', player: newId() });
      assertEquals((await stranger.got('error')).code, 'name_mismatch');
    });

    await t.step('a room outlives both players being disconnected', async () => {
      const { a, b, idA, idB, code } = await pair();
      await a.close();
      await b.close();
      await sleep(600); // well inside the 2-second test lifetime
      const a2 = player({ action: 'rejoin', code, name: 'Table', player: idA });
      const reply = await a2.got('room_joined');
      assertEquals(reply.recreated, false, 'the room was still there');
      assertEquals(reply.peer, 'away');
      const b2 = player({ action: 'rejoin', code, name: 'Table', player: idB });
      await b2.got('peer_joined');
      await a2.got('peer_joined');
    });

    await t.step('a room nobody returns to is deleted after its lifetime', async () => {
      const { p, id, code } = await createRoom();
      await p.close();
      await sleep(ROOM_TTL_SECONDS * 1000 + 1500);
      const stranger = player({ action: 'join', code, name: 'Table', player: newId() });
      assertEquals((await stranger.got('error')).code, 'not_found');
      // Its own player can still bring it back.
      const back = player({ action: 'rejoin', code, name: 'Table', player: id });
      assertEquals((await back.got('room_joined')).recreated, true);
    });

    await t.step('a room with someone connected does not expire', async () => {
      const { p, code } = await createRoom();
      await sleep(ROOM_TTL_SECONDS * 1000 + 1500);
      const b = player({ action: 'join', code, name: 'Table', player: newId() });
      await b.got('peer_joined');
      await p.got('peer_joined');
    });

    await t.step('the site client: create, join, drop and resume', async () => {
      // The real connection state machine from the site, with the same
      // address-per-attempt scheme kipukas-multiplayer.js uses.
      const side = (id: string) => {
        const s = {
          id,
          msgs: [] as Msg[],
          intent: {} as Record<string, string>,
          count: (type: string) => s.msgs.filter((m) => m.type === type).length,
          link: createLink({
            url: () => {
              const url = new URL(relay.ws);
              for (const [k, v] of Object.entries(s.intent)) url.searchParams.set(k, v);
              url.searchParams.set('player', id);
              return url.toString();
            },
            onMessage: (m: Msg) => s.msgs.push(m),
          }),
        };
        return s;
      };
      const [a, b] = [side(newId()), side(newId())];
      try {
        a.intent = { action: 'create', name: 'Table' };
        a.link.start();
        await until(() => a.count('room_created') === 1, 'room_created');
        a.link.settled();
        const code = a.msgs.find((m) => m.type === 'room_created')!.code as string;
        a.intent = { action: 'rejoin', code, name: 'Table' };

        b.intent = { action: 'join', code, name: 'Table' };
        b.link.start();
        await until(
          () => a.count('peer_joined') === 1 && b.count('peer_joined') === 1,
          'peer_joined',
        );
        b.link.settled();
        b.intent = { action: 'rejoin', code, name: 'Table' };
        assertEquals(b.link.relayVersion(), 2);

        // B loses its socket, as on a page change, and resumes.
        b.link.bounce();
        await until(() => a.count('peer_away') === 1, 'A told B is away');
        await until(() => b.count('room_joined') === 2, 'B resumes');
        await until(() => a.count('peer_joined') === 2, 'A told B is back');

        b.link.send({ type: 'relay', data: { type: 'hello', from: 'b' } });
        a.link.send({ type: 'relay', data: { type: 'hello', from: 'a' } });
        await until(() => a.count('relay') === 1 && b.count('relay') === 1, 'relayed messages');
      } finally {
        a.link.stop();
        b.link.stop();
      }
    });
  } finally {
    for (const p of open) await p.close().catch(() => {});
    await relay.stop();
    await sleep(200); // let sockets finish closing for Deno's leak check
  }
});
