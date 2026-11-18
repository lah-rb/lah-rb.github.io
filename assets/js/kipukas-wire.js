/**
 * kipukas-wire.js — the engine's hands: its sockets, its waits, and the
 * handover.
 *
 * The game engine only answers requests. It cannot open a socket, wait for
 * anything, or reach the page. What it wants done it queues, and whoever
 * holds the engine takes the queue after every post (the handover: page.rs
 * in the engine). This module is that holder's part, and it decides nothing:
 *
 * - It opens, sends on and closes the sockets the engine names, starts and
 *   forgets the waits it names, and reports what each does (wire.rs in the
 *   engine). When to connect, what to send, when to give a socket up and how
 *   long to wait before trying again are all the engine's (link.rs and
 *   game/connection.rs).
 * - It passes the page its part of each handover: what to store, what to
 *   draw again.
 *
 * In the browser the holder is the engine's worker (kipukas-worker.js). The
 * relay's tests hold an engine the same way, without a browser
 * (signaling-server/test/relay_test.ts).
 */

const SOCKET_OPEN = 1;

/**
 * @param {object} opts
 * @param {(method: string, path: string, body: string) => string} opts.request
 *   Ask the engine.
 * @param {(message: Record<string, unknown>) => void} opts.toPage
 *   Pass the page its part of a handover: SAVE, ROOM, REDRAW, REFRESH, GO
 *   or RELOAD.
 * @param {(handed: Record<string, unknown>) => void} [opts.also]
 *   Called with each handover, for a part that is the holder's own to do
 *   (the worker's QR decoder).
 * @param {typeof WebSocket} [opts.WebSocketImpl]
 */
export function createWire(opts) {
  const { request, toPage } = opts;
  const WS = opts.WebSocketImpl || globalThis.WebSocket;

  const sockets = new Map(); // name → socket
  const waits = new Map(); // name → timeout id

  // What sockets and waits have done that the engine has not been told yet.
  // They are told one at a time, each followed by its handover: telling can
  // lead to more to tell (a socket that cannot even be created).
  const untold = [];
  let telling = false;

  /** Tell the engine what a socket did, or that a wait is over. */
  function tell(report) {
    untold.push(report);
    if (telling) return;
    telling = true;
    try {
      while (untold.length) {
        request('POST', '/api/wire', JSON.stringify(untold.shift()));
        handOver();
      }
    } catch (err) {
      console.error('[kipukas-wire] Could not report to the engine:', err);
    } finally {
      telling = false;
    }
  }

  /** Do what the engine asked of its sockets and waits, in the order asked. */
  function carryOut(asked) {
    for (const ask of asked) {
      if ('open' in ask) {
        const name = ask.open;
        let socket;
        try {
          socket = new WS(ask.url);
        } catch (_) {
          tell({ socket: name, event: 'close' });
          continue;
        }
        sockets.set(name, socket);
        socket.onopen = () => tell({ socket: name, event: 'open' });
        socket.onmessage = (event) => {
          if (typeof event.data === 'string') {
            tell({ socket: name, event: 'message', data: event.data });
          }
        };
        socket.onerror = () => tell({ socket: name, event: 'error' });
        socket.onclose = () => {
          sockets.delete(name);
          tell({ socket: name, event: 'close' });
        };
      } else if ('send' in ask) {
        const socket = sockets.get(ask.send);
        if (socket && socket.readyState === SOCKET_OPEN) {
          try {
            socket.send(ask.text);
          } catch (_) { /* closing: the engine hears of that by itself */ }
        }
      } else if ('close' in ask) {
        // The engine has given this socket up: nothing more is reported of it.
        const socket = sockets.get(ask.close);
        sockets.delete(ask.close);
        if (socket) {
          socket.onopen =
            socket.onmessage =
            socket.onerror =
            socket.onclose =
              null;
          try {
            socket.close();
          } catch (_) { /* already closed */ }
        }
      } else if ('set' in ask) {
        const name = ask.set;
        clearTimeout(waits.get(name));
        waits.set(
          name,
          setTimeout(() => {
            waits.delete(name);
            tell({ timer: name });
          }, ask.ms),
        );
      } else if ('clear' in ask) {
        clearTimeout(waits.get(ask.clear));
        waits.delete(ask.clear);
      }
    }
  }

  /**
   * After a post: take what the engine has queued and see it done.
   *
   * The page's part goes first, and of that what to store goes first, so
   * that what follows finds storage as the engine left it. The sockets and
   * waits go last: doing them can itself lead to a post and another
   * handover, whose messages must follow this one's.
   */
  function handOver() {
    let handed;
    try {
      handed = JSON.parse(request('POST', '/api/handover', ''));
    } catch (err) {
      console.error('[kipukas-wire] Could not take what the engine has queued:', err);
      return;
    }
    if (handed.save) toPage({ type: 'SAVE', writes: handed.save });
    if ('room' in handed) toPage({ type: 'ROOM', inRoom: handed.room });
    if (handed.redraw) toPage({ type: 'REDRAW', routes: handed.redraw });
    if (handed.refresh) toPage({ type: 'REFRESH' });
    if (handed.go) toPage({ type: 'GO', path: handed.go });
    if (handed.reload) toPage({ type: 'RELOAD' });
    if (opts.also) opts.also(handed);
    if (handed.wire) carryOut(handed.wire);
  }

  /**
   * Pass on what the page reports of itself or of storage (store.rs), with
   * the names of the sockets found open just now: one that died while the
   * phone slept may not have said so yet.
   */
  function reportPage(report) {
    const alive = [];
    for (const [name, socket] of sockets) {
      if (socket.readyState === SOCKET_OPEN) alive.push(name);
    }
    request('POST', '/api/page', JSON.stringify({ ...report, alive }));
    handOver();
  }

  /** Close every socket and forget every wait, telling the engine nothing. */
  function shutDown() {
    carryOut([
      ...Array.from(sockets.keys(), (name) => ({ close: name })),
      ...Array.from(waits.keys(), (name) => ({ clear: name })),
    ]);
  }

  return { handOver, reportPage, shutDown };
}
