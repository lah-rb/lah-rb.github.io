// Tests for assets/js/kipukas-link.js — the relay connection state machine.
//
// Run from the repo root:  deno test tests/
//
// These drive the link with a fake socket and manual timers, so they are
// deterministic. The link is also exercised against the real relay, running
// locally, in signaling-server/test/relay_test.ts.

import { createLink } from '../assets/js/kipukas-link.js';

// ── Tiny assertions (no remote imports, so the tests run offline) ──

function assertEquals<T>(actual: T, expected: T, what = '') {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what ? what + ': ' : ''}expected ${e}, got ${a}`);
}

function assert(cond: unknown, what: string) {
  if (!cond) throw new Error(what);
}

// ── Fakes ──────────────────────────────────────────────────────────

type Handler = ((event: { data?: string }) => void) | null;

class FakeSocket {
  static instances: FakeSocket[] = [];
  readyState = 0;
  sent: Record<string, unknown>[] = [];
  onopen: Handler = null;
  onmessage: Handler = null;
  onerror: Handler = null;
  onclose: Handler = null;

  constructor(public url: string) {
    FakeSocket.instances.push(this);
  }
  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  /** Like a real socket, close() does not report `close` synchronously. */
  close() {
    this.readyState = 3;
  }

  // Test controls
  succeed() {
    this.readyState = 1;
    this.onopen?.({});
  }
  /** A failed connection reports `error` and then `close`. */
  fail() {
    this.readyState = 3;
    this.onerror?.({});
    this.onclose?.({});
  }
  serverClose() {
    this.readyState = 3;
    this.onclose?.({});
  }
  receive(msg: unknown) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  pings() {
    return this.sent.filter((m) => m.type === 'ping').length;
  }
}

class Clock {
  now = 0;
  private nextId = 1;
  private timers = new Map<number, { at: number; fn: () => void }>();

  set = (fn: () => void, ms: number) => {
    const id = this.nextId++;
    this.timers.set(id, { at: this.now + ms, fn });
    return id;
  };
  clear = (id: unknown) => {
    this.timers.delete(id as number);
  };
  pending() {
    return this.timers.size;
  }
  /** Run every timer due within the next `ms`, in time order. */
  advance(ms: number) {
    const end = this.now + ms;
    for (;;) {
      let nextId = 0;
      let nextAt = Infinity;
      for (const [id, t] of this.timers) {
        if (t.at <= end && t.at < nextAt) {
          nextAt = t.at;
          nextId = id;
        }
      }
      if (!nextId) break;
      const t = this.timers.get(nextId)!;
      this.timers.delete(nextId);
      this.now = t.at;
      t.fn();
    }
    this.now = end;
  }
}

function setup(extra: Record<string, unknown> = {}) {
  FakeSocket.instances = [];
  const clock = new Clock();
  const states: string[] = [];
  const messages: Record<string, unknown>[] = [];
  let opens = 0;
  const link = createLink({
    url: () => 'ws://fake/ws',
    WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    setTimer: clock.set,
    clearTimer: clock.clear,
    random: () => 0, // no jitter
    onOpen: () => opens++,
    onMessage: (m: Record<string, unknown>) => messages.push(m),
    onState: (next: string) => states.push(next),
    ...extra,
  });
  const sockets = () => FakeSocket.instances;
  return { link, clock, states, messages, sockets, opens: () => opens };
}

// ── Unit tests ─────────────────────────────────────────────────────

Deno.test('opens one socket and reports open once', () => {
  const t = setup();
  t.link.start();
  assertEquals(t.link.state(), 'connecting');
  t.sockets()[0].succeed();
  assertEquals(t.link.state(), 'open');
  assertEquals(t.opens(), 1);
  assertEquals(t.sockets().length, 1);
});

Deno.test('start while connecting or open does not create a second socket', () => {
  const t = setup();
  t.link.start();
  t.link.start();
  t.sockets()[0].succeed();
  t.link.start();
  assertEquals(t.sockets().length, 1);
});

Deno.test('a failed open (error then close) schedules exactly one retry', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].fail();
  assertEquals(t.link.state(), 'waiting');
  assertEquals(t.clock.pending(), 1, 'pending timers');
  t.clock.advance(1000);
  assertEquals(t.sockets().length, 2, 'sockets after first retry');
  // Nothing else is queued behind it: only the new socket's connect timeout.
  assertEquals(t.clock.pending(), 1, 'pending timers after retry');
});

Deno.test('a late close from an abandoned socket is ignored', () => {
  const t = setup();
  t.link.start();
  const first = t.sockets()[0];
  first.succeed();
  first.serverClose();
  t.clock.advance(1000);
  const second = t.sockets()[1];
  second.succeed();
  assertEquals(t.link.state(), 'open');

  first.onclose?.({}); // the old socket reports close again, late
  first.onerror?.({});
  assertEquals(t.link.state(), 'open');
  assertEquals(t.sockets().length, 2);
  assert(t.link.send({ type: 'relay' }), 'send should still reach the current socket');
  assertEquals(second.sent.length, 1);
});

Deno.test('retries never stop and the delay is capped', () => {
  const t = setup();
  t.link.start();
  for (let i = 0; i < 25; i++) {
    t.sockets()[i].fail();
    assertEquals(t.link.state(), 'waiting', `attempt ${i}`);
    const delay = Math.min(1000 * 2 ** i, 15_000); // jitter is disabled in tests
    t.clock.advance(delay - 1);
    assertEquals(t.sockets().length, i + 1, `no retry before ${delay}ms (attempt ${i})`);
    t.clock.advance(1);
    assertEquals(t.sockets().length, i + 2, `retry at ${delay}ms (attempt ${i})`);
  }
});

Deno.test('backoff grows: 1s, 2s, 4s', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].fail();
  t.clock.advance(999);
  assertEquals(t.sockets().length, 1);
  t.clock.advance(1);
  assertEquals(t.sockets().length, 2);
  t.sockets()[1].fail();
  t.clock.advance(1999);
  assertEquals(t.sockets().length, 2);
  t.clock.advance(1);
  assertEquals(t.sockets().length, 3);
  t.sockets()[2].fail();
  t.clock.advance(3999);
  assertEquals(t.sockets().length, 3);
  t.clock.advance(1);
  assertEquals(t.sockets().length, 4);
});

Deno.test('gives up when shouldRetry says no', () => {
  let keep = true;
  const t = setup({ shouldRetry: () => keep });
  t.link.start();
  t.sockets()[0].fail();
  assertEquals(t.link.state(), 'waiting');
  t.clock.advance(1000);
  keep = false;
  t.sockets()[1].fail();
  assertEquals(t.link.state(), 'idle');
  assertEquals(t.clock.pending(), 0);
});

Deno.test('kick reconnects at once from waiting', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].fail();
  t.clock.advance(1000);
  t.sockets()[1].fail(); // now waiting ~2s
  t.link.kick();
  assertEquals(t.sockets().length, 3);
  assertEquals(t.link.state(), 'connecting');
  assertEquals(t.clock.pending(), 1, 'only the connect timeout is pending');
});

Deno.test('kick drops a socket that claims to be open but is not', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].succeed();
  t.sockets()[0].readyState = 3; // died without reporting close (phone slept)
  t.link.kick();
  assertEquals(t.link.state(), 'waiting');
});

Deno.test('stop closes the socket, cancels retries and stays idle', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].succeed();
  t.link.stop();
  assertEquals(t.link.state(), 'idle');
  assertEquals(t.sockets()[0].readyState, 3);
  assertEquals(t.clock.pending(), 0);
  t.sockets()[0].onclose?.({});
  t.clock.advance(60_000);
  assertEquals(t.sockets().length, 1);
});

Deno.test('a connect that never opens times out and retries', () => {
  const t = setup();
  t.link.start();
  t.clock.advance(10_000);
  assertEquals(t.link.state(), 'waiting');
  t.clock.advance(1000);
  assertEquals(t.sockets().length, 2);
});

Deno.test('pings are always the same message, so the relay can answer without waking', () => {
  const t = setup();
  t.link.start();
  const s = t.sockets()[0];
  s.succeed();
  t.clock.advance(20_000);
  t.clock.advance(20_000);
  const pings = s.sent.filter((m) => m.type === 'ping');
  assertEquals(pings.length, 2);
  assertEquals(JSON.stringify(pings[0]), '{"type":"ping"}');
  assertEquals(JSON.stringify(pings[1]), '{"type":"ping"}');
});

Deno.test('before the relay says which protocol it speaks, a missing pong is tolerated', () => {
  const t = setup();
  t.link.start();
  const s = t.sockets()[0];
  s.succeed();
  s.receive({ type: 'room_joined', code: 'ABCD' }); // no `v`
  t.clock.advance(20_000);
  assertEquals(s.pings(), 1);
  t.clock.advance(100_000);
  assertEquals(t.link.state(), 'open');
  assertEquals(s.pings(), 6);
});

Deno.test('once the relay has announced v2, a missing pong drops the socket and a pong keeps it', () => {
  const t = setup();
  t.link.start();
  const s = t.sockets()[0];
  s.succeed();
  s.receive({ type: 'room_joined', code: 'ABCD', v: 2 });

  t.clock.advance(20_000);
  s.receive({ type: 'pong' });
  t.clock.advance(15_000);
  assertEquals(t.link.state(), 'open', 'pong arrived');

  t.clock.advance(5_000); // second ping at 40s, no pong
  t.clock.advance(10_000);
  assertEquals(t.link.state(), 'waiting', 'pong missing');
  assertEquals(t.messages.length, 1, 'pong is not passed to the caller');
});

Deno.test('bounce backs off until the caller reports settled', () => {
  const t = setup();
  t.link.start();
  t.sockets()[0].succeed();
  t.link.bounce(); // refused: retry in 1s
  t.clock.advance(1000);
  t.sockets()[1].succeed();
  t.link.bounce(); // refused again: retry in 2s, not 1s
  t.clock.advance(1000);
  assertEquals(t.sockets().length, 2);
  t.clock.advance(1000);
  assertEquals(t.sockets().length, 3);
  t.sockets()[2].succeed();
  t.link.settled();
  t.link.bounce(); // back to 1s
  t.clock.advance(1000);
  assertEquals(t.sockets().length, 4);
});

Deno.test('send reports false when the socket is not open', () => {
  const t = setup();
  assertEquals(t.link.send({ type: 'relay' }), false);
  t.link.start();
  assertEquals(t.link.send({ type: 'relay' }), false);
  t.sockets()[0].succeed();
  assertEquals(t.link.send({ type: 'relay' }), true);
});
