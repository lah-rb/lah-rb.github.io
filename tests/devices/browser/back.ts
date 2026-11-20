/**
 * A page that is left with its scanner open and come back to with Back.
 *
 * A browser may keep such a page whole and bring it back with its engine as
 * it was (the Back button, in CONTRIBUTING.md). Its scanner must be closed,
 * with no camera held, and must open again with what was chosen on the page
 * visited in between.
 *
 *   deno run -A browser/back.ts [site]
 */
import type { Checks } from './site.ts';
import {
  alone,
  go,
  launch,
  openScanner,
  pressQR,
  ready,
  scanner,
  sleep,
  tap,
  until,
  watch,
} from './site.ts';

type Shown = typeof globalThis & { __persisted?: boolean };

export async function run(site: string, checks: Checks) {
  const browser = await launch();
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 420, height: 860 });
    const seen = watch(page);
    await page.evaluateOnNewDocument(() => {
      addEventListener('pageshow', (event) => {
        (globalThis as Shown).__persisted = event.persisted;
      });
    });

    // A card page; the scanner is opened and its camera runs.
    await go(page, site + '/brox_the_defiant/');
    await openScanner(page);
    checks.check('Back: the scanner opens and its camera runs', true);

    // The page is left with the scanner open, as a code that is read leaves it.
    const track = await page.evaluateHandle(() => {
      const video = document.querySelector('[data-qr-camera] video') as HTMLVideoElement;
      return (video.srcObject as MediaStream).getVideoTracks()[0];
    });
    await page.evaluate(() => {
      location.href = '/hilbert_king_of_avian_frogs/';
    });
    await until('second page', () => page.evaluate(() => location.pathname.startsWith('/hilbert')));
    await ready(page);

    // On the second page the player turns the screen light on: a choice the first has not seen.
    await openScanner(page);
    await tap(page, 'light');
    await until(
      'light stored',
      () => page.evaluate(() => localStorage.getItem('kipukas-scanner-light') === 'true'),
    );
    await tap(page, 'close');
    await until('closed', async () => !(await scanner(page)).open);

    await page.goBack({ waitUntil: 'load' }).catch(() => {});
    await until(
      'first page again',
      () => page.evaluate(() => location.pathname.startsWith('/brox')),
    );
    await ready(page);
    await sleep(1200);
    const persisted = await page.evaluate(() => (globalThis as Shown).__persisted === true);
    const back = await scanner(page);
    let camera = 'gone with the page';
    try {
      camera = await page.evaluate((track) => track.readyState, track);
    } catch (_) { /* the page was loaded anew */ }
    checks.note(
      `the page was ${
        persisted ? 'brought back from the cache' : 'loaded anew'
      }; the camera it had is ${camera}`,
    );
    checks.check(
      'Back: on the page come back to, the scanner is closed and holds no camera',
      !back.open && !back.showing && camera !== 'live',
      JSON.stringify(back),
    );

    // Opened again, the camera runs, and the light chosen on the other page is on.
    await pressQR(page);
    await until('camera live again', async () => !!(await scanner(page)).showing);
    const again = await scanner(page);
    checks.check(
      'Back: opened again, the camera runs and the light chosen on the other page is on',
      again.lit,
      `light on: ${again.lit}`,
    );
    await tap(page, 'light');
    await sleep(500);
    checks.check(
      'Back: no script errors',
      seen.problems.length === 0,
      seen.problems.slice(0, 3).join(' | '),
    );
  } finally {
    await browser.close();
  }
}

if (import.meta.main) await alone(run);
