/**
 * A choice made in one tab is taken up by another that is already open.
 *
 * The settings and the scanner's choices are single keys in the browser's
 * storage, which every tab shares, but each tab has an engine of its own.
 * A tab learns of another's choice twice over: when the browser reports the
 * write (a `storage` event), and when the tab comes back into view and is
 * handed what is stored (`visibilitychange`). Both are tried here, the
 * second with the tab made deaf to the first.
 *
 *   deno run -A browser/tabs.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Camera, Checks } from './site.ts';
import {
  alone,
  asked,
  go,
  inWords,
  launch,
  openScanner,
  pressQR,
  pretendCameras,
  scanner,
  sleep,
  tap,
  until,
  watch,
} from './site.ts';

const PHONE: Camera[] = [
  { id: 'front-1', label: 'Front Camera', facing: 'user' },
  { id: 'back-1', label: 'Back Camera', facing: 'environment' },
];

const CARD = '/brox_the_defiant/';

type Deaf = typeof globalThis & { __deaf?: boolean };
type Views = typeof globalThis & {
  kipukasViews: { send(path: string, body: string): Promise<unknown> };
};

const pressSwitch = async (page: Page) => {
  const before = (await asked(page)).length;
  await page.evaluate(() => (document.querySelector('[data-qr-switch]') as HTMLElement).click());
  await until('camera asked for', async () => (await asked(page)).length > before);
  await sleep(600);
};

export async function run(site: string, checks: Checks) {
  const browser = await launch();
  try {
    // Two tabs of one browser: they share its storage.
    const context = await browser.createBrowserContext();
    const a = await context.newPage();
    const b = await context.newPage();
    const seen = [watch(a), watch(b)];
    for (const page of [a, b]) {
      await page.setViewport({ width: 420, height: 860 });
      await pretendCameras(page, PHONE, false);
    }
    // Tab A can be made deaf to what the browser reports of storage. This
    // listener is added before the site's, so it is heard first.
    await a.evaluateOnNewDocument(() => {
      addEventListener('storage', (event) => {
        if ((globalThis as Deaf).__deaf) event.stopImmediatePropagation();
      }, true);
    });
    // Tab A is opened first and its scanner used once: the privacy notice is
    // accepted and the front camera runs.
    await go(a, site + CARD);
    await openScanner(a);
    await until('stored', async () => (await scanner(a)).stored === 'Front Camera');
    await tap(a, 'close');
    await until('closed', async () => !(await scanner(a)).open);
    // Tab B is opened second, in front of A, which is out of view from here
    // on. It is the same card's page, so that B writes nothing of the
    // player's own state: a tab loads itself again when it is shown after
    // another has written that (a card seen for the first time is enough),
    // and that is not what is tried here.
    await go(b, site + CARD);
    const turnPrivacy = () =>
      b.evaluate(() =>
        (globalThis as Views).kipukasViews.send('/api/settings', 'toggle=qr-privacy-accepted')
      );
    const privacyStored = (value: string) =>
      until(
        'privacy stored',
        () => b.evaluate((value) => localStorage.getItem('qr-privacy-accepted') === value, value),
      );

    // ── The browser reports the other tab's write ──

    // In B the acceptance of the privacy notice is taken back, as the
    // settings page's switch does. A's scanner asks again.
    await turnPrivacy();
    await privacyStored('false');
    await sleep(400);
    await pressQR(a);
    await until('scanner drawn', async () => (await scanner(a)).open || (await scanner(a)).notice);
    const asksAgain = await scanner(a);
    checks.check(
      'tabs: the privacy notice taken back in one tab is asked for again in another',
      asksAgain.notice && !asksAgain.open,
      JSON.stringify({ notice: asksAgain.notice, open: asksAgain.open }),
    );
    // In B it is accepted again. A's notice stays until it is answered there: no camera
    // starts in a tab nobody is looking at. Opened anew, A goes straight to the camera.
    await turnPrivacy();
    await privacyStored('true');
    await sleep(600);
    const stays = await scanner(a);
    await tap(a, 'close');
    await until('closed', async () => !(await scanner(a)).notice);
    await pressQR(a);
    await until('camera live', async () => !!(await scanner(a)).showing);
    const straight = await scanner(a);
    checks.check(
      'tabs: accepted in one tab, the notice is not asked for again in another, and starts no camera there by itself',
      stays.notice && !stays.showing && !straight.notice && straight.showing === 'Front Camera',
      JSON.stringify({ stayed: stays.notice, then: straight.showing }),
    );
    await tap(a, 'close');
    await until('closed', async () => !(await scanner(a)).open);

    // In B the player changes to the rear camera. A asks for it when next opened.
    await openScanner(b);
    await pressSwitch(b);
    await until('stored', async () => (await scanner(b)).stored === 'Back Camera');
    await sleep(400);
    const before = (await asked(a)).length;
    await openScanner(a);
    const second = await scanner(a);
    const wanted = inWords((await asked(a)).slice(before));
    checks.check(
      'tabs: the camera chosen in one tab is the one another asks for',
      second.showing === 'Back Camera' && !second.mirrored && wanted === 'id back-1',
      `${second.showing}; asked ${wanted}`,
    );

    // In B the screen light is turned on. A's scanner, which is open, lights
    // up, and stays on its camera.
    const startsBefore = (await asked(a)).length;
    await tap(b, 'light');
    await until('lit in the other tab', async () => (await scanner(a)).lit, 8000).catch(() => {});
    const lit = await scanner(a);
    checks.check(
      'tabs: the light turned on in one tab shows in the open scanner of another, which keeps its camera',
      lit.lit && lit.showing === 'Back Camera' && (await asked(a)).length === startsBefore,
      JSON.stringify({ lit: lit.lit, showing: lit.showing }),
    );

    // ── The tab comes back into view ──

    // A hears nothing of storage from here on, as a tab put to sleep may not.
    await a.evaluate(() => {
      (globalThis as Deaf).__deaf = true;
    });
    await tap(b, 'light');
    await pressSwitch(b);
    await until('stored', async () => (await scanner(b)).stored === 'Front Camera');
    await sleep(800);
    const asleep = await scanner(a);
    const hidden = await a.evaluate(() => document.visibilityState === 'hidden');
    checks.note(
      `deaf to storage and ${hidden ? 'out of view' : 'in view'}, tab A still has the light ${
        asleep.lit ? 'on' : 'off'
      } and wants a camera facing "${asleep.facing}"`,
    );
    // It is shown again: the page hands its engine what is stored now. (A
    // browser that had it in view all along is told so by hand.)
    await a.bringToFront();
    if (!hidden) {
      await a.evaluate(() => {
        document.dispatchEvent(new Event('visibilitychange'));
      });
    }
    await until(
      'taken up',
      async () => !(await scanner(a)).lit && (await scanner(a)).facing === 'user',
      8000,
    ).catch(() => {});
    const shown = await scanner(a);
    checks.check(
      'tabs: a tab that comes back into view takes up what was chosen while it was away, and its scanner stays open',
      asleep.lit && asleep.facing === 'environment' && !shown.lit && shown.facing === 'user' &&
        shown.open && shown.showing === 'Back Camera',
      JSON.stringify({ lit: shown.lit, wants: shown.facing, showing: shown.showing }),
    );
    await a.evaluate(() => {
      (globalThis as Deaf).__deaf = false;
    });

    // ── The privacy notice taken back while a scanner is open ──

    await turnPrivacy();
    await until('notice in the other tab', async () => (await scanner(a)).notice, 8000).catch(
      () => {},
    );
    const taken = await scanner(a);
    checks.check(
      'tabs: the privacy notice taken back in one tab stops the camera of a scanner open in another',
      taken.notice && !taken.showing,
      JSON.stringify({ notice: taken.notice, showing: taken.showing }),
    );

    const problems = seen.flatMap((tab) => tab.problems);
    checks.check('tabs: no script errors', problems.length === 0, problems.slice(0, 3).join(' | '));
    await context.close();
  } finally {
    await browser.close();
  }
}

if (import.meta.main) await alone(run);
