/**
 * kipukas-link.js — one connection state machine for the relay socket.
 *
 * Owns exactly one WebSocket at a time and exactly one retry timer. It knows
 * nothing about rooms, the DOM or the game engine: the caller says what to
 * send when a socket opens and handles the messages that arrive. That keeps
 * it drivable from `deno test` (see tests/link_test.ts).
 *
 * States:
 *   idle        not wanted, no socket
 *   connecting  socket created, not open yet
 *   open        socket open
 *   waiting     socket lost, a retry is scheduled
 *
 * Every socket gets a generation number. Events from any socket other than the
 * current one are ignored, so a late `close` from an abandoned socket can never
 * tear down or re-schedule a newer one.
 */

const PING_INTERVAL_MS = 20_000;
const PONG_TIMEOUT_MS = 10_000;
const CONNECT_TIMEOUT_MS = 10_000;
const BASE_RETRY_MS = 1_000;
const MAX_RETRY_MS = 15_000;
const SOCKET_OPEN = 1;

/**
 * @param {object} opts
 * @param {() => string} opts.url             Relay address, read on every attempt.
 * @param {() => void} [opts.onOpen]          A socket opened: send create/join/rejoin now.
 * @param {(msg: Record<string, unknown>) => void} [opts.onMessage]  Parsed relay message.
 * @param {(next: string, prev: string) => void} [opts.onState]  State changed.
 * @param {() => boolean} [opts.shouldRetry]  Asked when a socket is lost. False ends retrying.
 * @param {(...args: unknown[]) => void} [opts.log]
 * @param {typeof WebSocket} [opts.WebSocketImpl]   Injected in tests.
 * @param {(fn: () => void, ms: number) => unknown} [opts.setTimer]
 * @param {(id: unknown) => void} [opts.clearTimer]
 * @param {() => number} [opts.random]
 * @param {() => number} [opts.now]
 */
export function createLink(opts) {
  const WS = opts.WebSocketImpl || globalThis.WebSocket;
  const setTimer = opts.setTimer || ((fn, ms) => globalThis.setTimeout(fn, ms));
  const clearTimer = opts.clearTimer || ((id) => globalThis.clearTimeout(id));
  const random = opts.random || Math.random;
  const now = opts.now || Date.now;
  const log = opts.log || (() => {});

  let state = 'idle';
  let wanted = false;
  let socket = null;
  let generation = 0; // bumped for every socket and again when that socket is lost
  let attempts = 0; // consecutive losses since the caller last called settled()
  let relayVersion = 1; // the relay advertises `v` in room_created / room_joined

  let retryTimer = null;
  let connectTimer = null;
  let pingTimer = null;
  let pongTimer = null;

  function setState(next) {
    if (state === next) return;
    const prev = state;
    state = next;
    if (opts.onState) opts.onState(next, prev);
  }

  function cancel(id) {
    if (id !== null) clearTimer(id);
    return null;
  }

  function cancelSocketTimers() {
    connectTimer = cancel(connectTimer);
    pingTimer = cancel(pingTimer);
    pongTimer = cancel(pongTimer);
  }

  /** Exponential backoff with jitter, capped so retries stay frequent. */
  function retryDelay() {
    const base = Math.min(BASE_RETRY_MS * 2 ** Math.min(attempts, 10), MAX_RETRY_MS);
    return base + random() * base * 0.3;
  }

  function sendRaw(obj) {
    if (!socket || socket.readyState !== SOCKET_OPEN) return false;
    try {
      socket.send(JSON.stringify(obj));
      return true;
    } catch (err) {
      log('send failed', err);
      return false;
    }
  }

  function scheduleRetry() {
    setState('waiting');
    const delay = retryDelay();
    attempts++;
    log(`retry in ${Math.round(delay)}ms (attempt ${attempts})`);
    retryTimer = setTimer(() => {
      retryTimer = null;
      open();
    }, delay);
  }

  /** The socket of generation `gen` is gone: closed, failed, or abandoned. */
  function lost(gen) {
    if (gen !== generation) return; // stale event from an older socket
    generation++; // anything else this socket reports is stale too
    cancelSocketTimers();
    socket = null;

    if (!wanted || (opts.shouldRetry && !opts.shouldRetry())) {
      wanted = false;
      setState('idle');
      return;
    }
    scheduleRetry();
  }

  /** Abandon the current socket ourselves (timeout, dead socket, bounce, stop). */
  function drop(gen) {
    if (gen !== generation) return;
    const ws = socket;
    lost(gen); // first, so the close event this triggers is already stale
    try {
      if (ws) ws.close();
    } catch (_) { /* already closed */ }
  }

  function ping(gen) {
    if (gen !== generation) return;
    sendRaw({ type: 'ping', t: now() });
    // Only a v2 relay answers pings. Against the current relay the ping is
    // ignored, which still counts as traffic that keeps the connection warm.
    if (relayVersion >= 2 && pongTimer === null) {
      pongTimer = setTimer(() => {
        pongTimer = null;
        log('no pong from relay, dropping socket');
        drop(gen);
      }, PONG_TIMEOUT_MS);
    }
  }

  function startHeartbeat(gen) {
    const tick = () => {
      if (gen !== generation) return;
      ping(gen);
      pingTimer = setTimer(tick, PING_INTERVAL_MS);
    };
    pingTimer = setTimer(tick, PING_INTERVAL_MS);
  }

  function open() {
    if (!wanted || state === 'connecting' || state === 'open') return;
    retryTimer = cancel(retryTimer);

    const gen = ++generation;
    let ws;
    try {
      ws = new WS(opts.url());
    } catch (err) {
      log('could not create socket', err);
      lost(gen);
      return;
    }
    socket = ws;
    setState('connecting');

    connectTimer = setTimer(() => {
      connectTimer = null;
      if (gen === generation && state === 'connecting') {
        log('connect timed out');
        drop(gen);
      }
    }, CONNECT_TIMEOUT_MS);

    ws.onopen = () => {
      if (gen !== generation) return;
      connectTimer = cancel(connectTimer);
      relayVersion = 1;
      setState('open');
      startHeartbeat(gen);
      if (opts.onOpen) opts.onOpen();
    };

    ws.onmessage = (event) => {
      if (gen !== generation) return;
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch (_) {
        return;
      }
      if (!msg || typeof msg !== 'object') return;
      if (typeof msg.v === 'number') relayVersion = msg.v;
      if (msg.type === 'pong') {
        pongTimer = cancel(pongTimer);
        return;
      }
      if (opts.onMessage) opts.onMessage(msg);
    };

    // A failed socket reports `error` and then `close`. Both end in lost(),
    // which acts once per socket, so one failure schedules one retry.
    ws.onerror = () => drop(gen);
    ws.onclose = () => lost(gen);
  }

  return {
    /** Want a connection. Connects now, or immediately if a retry was pending. */
    start() {
      wanted = true;
      open();
    },

    /** Stop wanting a connection: close the socket and cancel any retry. */
    stop() {
      wanted = false;
      retryTimer = cancel(retryTimer);
      if (socket) drop(generation);
      else setState('idle');
    },

    /**
     * Something suggests the network is back (tab visible, `online`, page
     * restored). Retry now instead of waiting out the backoff, and check that
     * an apparently open socket is still alive.
     */
    kick() {
      if (!wanted) return;
      if (state === 'waiting') {
        attempts = 0;
        open();
      } else if (state === 'open') {
        if (!socket || socket.readyState !== SOCKET_OPEN) drop(generation);
        else ping(generation);
      }
    },

    /**
     * Close the current socket and reconnect after the usual backoff. Used when
     * the relay answered but refused us for now (a rejoin told "Room is full").
     */
    bounce() {
      if (socket) drop(generation);
    },

    /**
     * The relay accepted us (room created or joined). Resets the backoff, so a
     * socket that opens and is refused over and over still backs off.
     */
    settled() {
      attempts = 0;
    },

    /** Send a message. Returns false if the socket is not open. */
    send(obj) {
      return sendRaw(obj);
    },

    state() {
      return state;
    },

    relayVersion() {
      return relayVersion;
    },
  };
}
