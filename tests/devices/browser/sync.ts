/**
 * A game moved from one device to another (the Player Data page's sync).
 *
 * Two browsers with nothing in common stand for the two devices. The one
 * with the game shows a code; the other is handed it each of the three
 * ways there are, and takes the game:
 *
 *   - its scanner reads the QR code. The QR code is held up to a pretend
 *     camera, so it is the site's own reader that reads it;
 *   - the code is typed;
 *   - the address the QR code carries is opened, as a phone's camera app
 *     opens it.
 *
 * Against the built site a relay is run here (`wrangler dev`, as the
 * relay's own tests run it) and both browsers are pointed at it. Against
 * the live site they use the live relay.
 *
 *   deno run -A browser/sync.ts [site]
 */
import type { Browser, Page } from 'puppeteer-core';
import type { Camera, Checks } from './site.ts';
import {
  alone,
  go,
  holdUp,
  launch,
  pretendCameras,
  ready,
  scanner,
  sleep,
  tap,
  until,
  watch,
} from './site.ts';

const CARD = 'brox_the_defiant';
const OTHER_CARD = 'hilbert_king_of_avian_frogs';
const PHONE: Camera[] = [{ id: 'back-1', label: 'Back Camera', facing: 'environment' }];

type Engine = typeof globalThis & {
  kipukasViews: { post(route: string, body: string): Promise<string> };
  kipukasWorker: Worker;
};

// ── A relay of our own ─────────────────────────────────────────────

function freePort(): number {
  const listener = Deno.listen({ port: 0 });
  const port = (listener.addr as Deno.NetAddr).port;
  listener.close();
  return port;
}

/** The relay, run locally with storage of its own that is thrown away. */
export async function startRelay() {
  const port = freePort();
  const storage = await Deno.makeTempDir({ prefix: 'kipukas-relay-check-' });
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
      '--show-interactive-dev-session=false',
    ],
    cwd: new URL('../../../signaling-server/', import.meta.url).pathname,
    stdin: 'null',
    stdout: 'null',
    stderr: 'null',
  }).spawn();
  const start = Date.now();
  for (;;) {
    try {
      if (await (await fetch(`http://127.0.0.1:${port}/health`)).text() === 'ok') break;
    } catch (_) { /* not up yet */ }
    if (Date.now() - start > 60_000) {
      child.kill();
      throw new Error('the local relay did not start');
    }
    await sleep(200);
  }
  return {
    address: `ws://127.0.0.1:${port}/ws`,
    async stop() {
      try {
        child.kill('SIGTERM');
      } catch (_) { /* already gone */ }
      await child.status;
      await Deno.remove(storage, { recursive: true }).catch(() => {});
    },
  };
}

// ── A device ───────────────────────────────────────────────────────

/** A browser with storage of its own, with one page open. */
async function device(browser: Browser, relay: string | null) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  await page.setViewport({ width: 420, height: 900 });
  const seen = watch(page);
  await pretendCameras(page, PHONE, false);
  if (relay) {
    await page.evaluateOnNewDocument((relay: string) => {
      localStorage.setItem('kipukas_relay_url', relay);
    }, relay);
  }
  return { page, seen, close: () => context.close() };
}

/** Ask the page's engine for a change. */
const post = (page: Page, route: string, body: string) =>
  page.evaluate(
    (route, body) => (globalThis as Engine).kipukasViews.post(route, body),
    route,
    body,
  );

/** What the page's engine draws for a route. */
const drawn = (page: Page, route: string): Promise<string> =>
  page.evaluate((route) => {
    const url = new URL(route, location.origin);
    return new Promise<string>((resolve) => {
      const channel = new MessageChannel();
      channel.port1.onmessage = (message) => resolve(message.data?.html || '');
      (globalThis as Engine).kipukasWorker.postMessage(
        { method: 'GET', pathname: url.pathname, search: url.search, body: '' },
        [channel.port2],
      );
    });
  }, route);

/** The game on a device: which of a card's slots are marked, and its timers by name. */
async function game(page: Page, card: string) {
  const damage = await drawn(page, `/api/game/damage?card=${card}`);
  const marked = [...damage.matchAll(/aria-pressed="(true|false)" aria-label="[^"]* damage /g)]
    .map((slot) => slot[1] === 'true');
  const timers = await drawn(page, '/api/game/turns?display=alarms');
  return { marked: marked.map((m) => (m ? 'x' : '-')).join(''), timers };
}

/** The sync section of the Player Data page, as it is drawn. */
function section(page: Page) {
  return page.evaluate(() => {
    const view = document.querySelector('[data-engine-view^="/api/player/sync"]');
    const text = (view?.textContent || '').replace(/\s+/g, ' ').trim();
    return {
      text,
      /** The question over the page, if one is asked. */
      question: view?.querySelector('[role="dialog"]')?.getAttribute('aria-label') || '',
      /** The code shown to be typed, if one is. */
      code: view?.querySelector('p.font-mono')?.textContent || '',
      /** The QR code shown, as markup. */
      qr: view?.querySelector('svg[role="img"]')?.outerHTML || '',
      error: view?.querySelector('[role="alert"]')?.textContent || '',
    };
  });
}

/** Press the section's button that asks the engine for `action`. */
function press(page: Page, action: string) {
  return page.evaluate((action) => {
    const button = [
      ...document.querySelectorAll('[data-engine-view^="/api/player/sync"] [data-engine-body]'),
    ].find((b) => (b.getAttribute('data-engine-body') || '').startsWith(`action=${action}`));
    if (!(button instanceof HTMLElement)) throw new Error(`the section has no "${action}"`);
    button.click();
  }, action);
}

const says = (page: Page, what: string, ms = 20000) =>
  until(`"${what}"`, async () => (await section(page)).text.includes(what), ms);

/** Have the sender show a code: asked, answered yes, and the code drawn. */
async function invite(page: Page) {
  await press(page, 'ask');
  await until('asked', async () => (await section(page)).question === 'Sync a new device');
  await press(page, 'invite');
  await until('a code', async () => !!(await section(page)).code);
  return await section(page);
}

export async function run(site: string, checks: Checks) {
  const relay = /^https?:\/\/(localhost|127\.0\.0\.1)/.test(site) ? await startRelay() : null;
  const browser = await launch();
  try {
    const a = await device(browser, relay?.address ?? null);
    const b = await device(browser, relay?.address ?? null);

    // Device A has a game: damage on the card's second slot, and a timer.
    await go(a.page, `${site}/${CARD}/`);
    await post(a.page, '/api/game/damage', `card=${CARD}&slot=2`);
    await post(a.page, '/api/game/turns', 'action=add&turns=5&name=siege&color_set=red');
    // Device B has another: damage on the first slot, another card damaged, a timer of its own.
    await go(b.page, `${site}/${CARD}/`);
    await post(b.page, '/api/game/damage', `card=${CARD}&slot=1`);
    await post(b.page, '/api/game/damage', `card=${OTHER_CARD}&action=wasted`);
    await post(b.page, '/api/game/turns', 'action=add&turns=2&name=theirs&color_set=blue');
    const before = await game(a.page, CARD);
    checks.check(
      'sync: the two devices start with different games',
      before.marked.startsWith('-x') && before.timers.includes('siege') &&
        (await game(b.page, CARD)).marked.startsWith('x-'),
      JSON.stringify(before.marked),
    );

    // ── The menu leads to the page ──

    const linked = await a.page.evaluate(() =>
      [...document.querySelectorAll('#hamburger-menu a')].map((link) =>
        `${link.textContent?.trim()}=${link.getAttribute('href')}`
      )
    );
    checks.check(
      'sync: the menu has Player Data, a page',
      linked.includes('Player Data=/player_data'),
      linked.join(', '),
    );

    await go(a.page, `${site}/player_data`);
    await go(b.page, `${site}/player_data`);
    const opens = await section(a.page);
    checks.check(
      'sync: the page opens on the button, the field for a code, and no question',
      opens.text.includes('Sync a new device') && opens.text.includes('Use this code') &&
        !opens.question,
      opens.text.slice(0, 120),
    );
    const scanButton = (page: Page) =>
      page.evaluate(() => {
        const button = [...document.querySelectorAll('button')].find((b) =>
          (b.textContent || '').includes('Or scan the code it shows')
        );
        return !!button && button.getClientRects().length > 0;
      });
    const offeredAtFirst = await scanButton(a.page);
    const backup = await a.page.evaluate(() =>
      document.querySelector('[data-engine-view="/api/player/backup"]')?.textContent || ''
    );
    checks.check(
      'sync: the page holds the backup too',
      backup.includes('Export Backup') && backup.includes('Import Backup'),
      backup.replace(/\s+/g, ' ').slice(0, 80),
    );

    // ── The sender is asked first ──

    await press(a.page, 'ask');
    await until('asked', async () => !!(await section(a.page)).question);
    const asked = await section(a.page);
    checks.check(
      'sync: the sender is asked, and told the other device loses its game',
      asked.question === 'Sync a new device' &&
        asked.text.includes('removes the game in progress on the other device') && !asked.code,
      asked.text.slice(-200),
    );
    await press(a.page, 'close');
    await until('put away', async () => !(await section(a.page)).question);
    checks.check('sync: answered no, nothing is shown', !(await section(a.page)).code);

    // ── By the scanner ──

    const shown = await invite(a.page);
    checks.check(
      'sync: answered yes, a QR code and a code to type are shown',
      /^[A-Z0-9]{4}(-[A-Z2-9]{5}){4}$/.test(shown.code) && shown.qr.includes('<path') &&
        shown.text.includes('Waiting for the other device'),
      shown.code.replace(/[A-Z0-9]/g, 'X'),
    );
    // B's camera sees A's screen. B opens its scanner from the page's button.
    checks.check(
      'sync: the button that opens the scanner is offered until this device shows a code',
      offeredAtFirst && !(await scanButton(a.page)) && (await scanButton(b.page)),
    );
    await holdUp(b.page, shown.qr);
    await b.page.evaluate(() => {
      const button = [...document.querySelectorAll('button')].find((b) =>
        (b.textContent || '').includes('Or scan the code it shows')
      );
      if (!button) throw new Error('no scan button on the page');
      button.click();
    });
    await until('notice', async () => (await scanner(b.page)).notice);
    await tap(b.page, 'accept');
    await until('asked', async () => !!(await section(b.page)).question, 30000);
    const offered = await section(b.page);
    const after = await scanner(b.page);
    checks.check(
      "sync: the site's scanner reads the QR code, and the receiver is asked over a closed scanner",
      offered.question === 'Take the game from another device' && !after.open && !after.showing,
      JSON.stringify({ question: offered.question, scanner: after.open }),
    );
    checks.check(
      'sync: the receiver is told what it would lose',
      offered.text.includes("This device's game will be removed: 2 cards with damage, 1 timer."),
      offered.text.slice(-260),
    );
    checks.check(
      'sync: reading the code moved nothing',
      (await game(b.page, CARD)).marked.startsWith('x-') &&
        (await section(a.page)).text.includes('Waiting for the other device'),
    );
    const address = await b.page.evaluate(() => location.pathname + location.hash);
    checks.check(
      'sync: the page stays, with the invitation in its address',
      address.startsWith('/player_data#sync='),
      address.replace(/=.*/, '=…'),
    );
    await holdUp(b.page, null);

    await press(b.page, 'receive');
    await says(b.page, 'This device now has the game from your other device.');
    await says(a.page, 'The other device now has this game.');
    const moved = await game(b.page, CARD);
    const kept = await game(a.page, CARD);
    checks.check(
      "sync: the receiver has the sender's game, and none of its own",
      moved.marked === before.marked && moved.timers.includes('siege') &&
        !moved.timers.includes('theirs') &&
        !(await drawn(b.page, `/api/game/damage?card=${OTHER_CARD}`)).includes(
          'data-wasted aria-pressed="true"',
        ),
      JSON.stringify({ marked: moved.marked, siege: moved.timers.includes('siege') }),
    );
    checks.check(
      'sync: the sender keeps its game',
      kept.marked === before.marked && kept.timers.includes('siege'),
      kept.marked,
    );
    // It is stored, not only held: the next page has it.
    await go(b.page, `${site}/${CARD}/`);
    const stored = await game(b.page, CARD);
    checks.check(
      'sync: the game taken is the one the next page loads with',
      stored.marked === before.marked && stored.timers.includes('siege'),
      stored.marked,
    );

    // ── What is kept of it ──

    /** The sync history a page draws: the sum's parts, and how many merges are listed. */
    const history = (page: Page) =>
      page.evaluate(() => {
        const view = document.querySelector('[data-engine-view^="/api/player/history"]');
        const part = (name: string) =>
          Number(view?.querySelector(`[data-sum="${name}"]`)?.textContent ?? NaN);
        return {
          shown: !!view?.querySelector('[data-sync-history]'),
          merges: part('merges'),
          gained: part('gained'),
          held: part('held'),
          distance: part('distance'),
          listed: view?.querySelectorAll('[data-merge]').length ?? 0,
        };
      });
    await go(b.page, `${site}/player_data`);
    const unasked = await history(b.page);
    await go(b.page, `${site}/player_data#history`);
    await until('the history', async () => (await history(b.page)).shown);
    const theirs = await history(b.page);
    await go(a.page, `${site}/player_data#history`);
    await until('the history', async () => (await history(a.page)).shown);
    const ours = await history(a.page);
    checks.check(
      'sync: the merge and the sync sum are kept, the same on both devices, and drawn only at #history',
      !unasked.shown && theirs.merges === 1 && theirs.listed === 1 && theirs.gained > 0 &&
        theirs.held > 0 && theirs.distance > 0 && JSON.stringify(theirs) === JSON.stringify(ours),
      JSON.stringify({ unasked: unasked.shown, theirs, ours }),
    );

    // ── A code works once ──

    await go(b.page, `${site}/player_data`);
    const used = shown.code;
    await b.page.type('input[name="invite"]', used);
    await b.page.keyboard.press('Enter');
    await until('asked', async () => !!(await section(b.page)).question);
    await press(b.page, 'receive');
    await until('refused', async () => !!(await section(b.page)).error);
    const refused = await section(b.page);
    checks.check(
      'sync: a code that has been used is refused, and says so',
      refused.error.includes('has ended or has been used'),
      refused.error,
    );

    // ── By typing ──

    // The game has changed on A since: a second timer.
    await press(a.page, 'close');
    await post(a.page, '/api/game/turns', 'action=add&turns=3&name=ambush&color_set=green');
    const second = await invite(a.page);
    checks.check('sync: each code is new', second.code !== used);
    await b.page.type('input[name="invite"]', 'not a code');
    await b.page.keyboard.press('Enter');
    await until('refused', async () => (await section(b.page)).error.includes('not a sync code'));
    const kept_typed = await b.page.$eval(
      'input[name="invite"]',
      (field) => (field as HTMLInputElement).value,
    );
    checks.check(
      'sync: what is not a code is said, and stays in its field',
      kept_typed === 'not a code',
      kept_typed,
    );
    // Typed in small letters, without the dashes.
    await b.page.$eval('input[name="invite"]', (field) => {
      (field as HTMLInputElement).value = '';
    });
    await b.page.type('input[name="invite"]', second.code.replaceAll('-', '').toLowerCase());
    await b.page.keyboard.press('Enter');
    await until('asked', async () => !!(await section(b.page)).question);
    await press(b.page, 'receive');
    await says(b.page, 'This device now has the game');
    await says(a.page, 'The other device now has this game.');
    checks.check(
      'sync: a typed code moves the game as it is now',
      (await game(b.page, CARD)).timers.includes('ambush'),
    );

    // ── By the address, as a camera app opens it ──

    await press(a.page, 'close');
    const third = await invite(a.page);
    const link = `${site}/player_data#sync=${
      third.code.slice(0, 5) + third.code.slice(5).replaceAll('-', '')
    }`;
    await b.page.goto(`${site}/${CARD}/`, { waitUntil: 'load' });
    await ready(b.page);
    await b.page.goto(link, { waitUntil: 'load' });
    await ready(b.page);
    await until('asked', async () => !!(await section(b.page)).question);
    checks.check(
      'sync: the address a camera app opens asks the same question',
      (await section(b.page)).question === 'Take the game from another device',
    );
    // Opening it moved nothing. Said no to, it is not asked again on this page.
    await press(b.page, 'decline');
    await until('put away', async () => !(await section(b.page)).question);
    await sleep(500);
    checks.check(
      'sync: said no to, nothing moves and the question is not asked again',
      !(await section(b.page)).question &&
        (await section(a.page)).text.includes('Waiting for the other device'),
    );
    // The page loaded again, it is asked again, and this time taken.
    await b.page.reload({ waitUntil: 'load' });
    await ready(b.page);
    await until('asked', async () => !!(await section(b.page)).question);
    await press(b.page, 'receive');
    await says(b.page, 'This device now has the game');
    await says(a.page, 'The other device now has this game.');
    checks.check('sync: the address moves the game too', true);

    // ── A code that is not the one shown ──

    await press(a.page, 'close');
    await press(b.page, 'close');
    const fourth = await invite(a.page);
    // The room's code, and a secret that is not its secret.
    const wrong = fourth.code.slice(0, 5) + 'ABCDE-FGHJK-LMNPQ-RSTUV';
    await b.page.type('input[name="invite"]', wrong);
    await b.page.keyboard.press('Enter');
    await until('asked', async () => !!(await section(b.page)).question);
    await press(b.page, 'receive');
    await until('refused', async () => !!(await section(b.page)).error);
    const mistyped = await section(b.page);
    checks.check(
      'sync: a code that is not the one shown is refused, and the sender goes on waiting',
      mistyped.error.includes('not the code the other device is showing') &&
        (await section(a.page)).text.includes('Waiting for the other device'),
      mistyped.error,
    );

    // ── Called off ──

    await press(a.page, 'close');
    await until('called off', async () => !(await section(a.page)).code);
    await b.page.$eval('input[name="invite"]', (field) => {
      (field as HTMLInputElement).value = '';
    });
    await b.page.type('input[name="invite"]', fourth.code);
    await b.page.keyboard.press('Enter');
    await until('asked', async () => !!(await section(b.page)).question);
    await press(b.page, 'receive');
    await until('refused', async () => !!(await section(b.page)).error);
    const gone = await section(b.page);
    checks.check(
      'sync: a code the sender called off no longer works',
      gone.error.includes('has ended or has been used'),
      gone.error,
    );

    const problems = [...a.seen.problems, ...b.seen.problems];
    checks.check('sync: no script errors', problems.length === 0, problems.slice(0, 3).join(' | '));
    await a.close();
    await b.close();
  } finally {
    await browser.close();
    if (relay) await relay.stop();
  }
}

if (import.meta.main) await alone(run);
