/**
 * The scanner's switch-camera button, on devices with different cameras.
 *
 * Where the running camera says which way it faces, the button changes
 * between that way and the other, and the browser chooses the camera; where
 * it does not say, or the browser has none that faces the other way, the
 * button goes round the device's cameras (`switchCamera` in qr-camera.js).
 * The cameras here are pretend ones, so every device can be tried.
 *
 *   deno run -A browser/switch.ts [site]
 */
import type { Browser } from 'puppeteer-core';
import type { Camera, Checks } from './site.ts';
import {
  asked,
  go,
  inWords,
  launch,
  openScanner,
  pretendCameras,
  scanner,
  sleep,
  until,
  watch,
} from './site.ts';
import { alone } from './site.ts';

const FRONT: Camera = { id: 'front-1', label: 'Front Camera', facing: 'user' };
const rear = (label: string, id: string): Camera => ({ id, label, facing: 'environment' });
// As Safari listed them on an iPhone 14 Pro Max with iOS 26.
const IPHONE: Camera[] = [
  FRONT,
  rear('Back Triple Camera', 'triple'),
  rear('Back Dual Wide Camera', 'dual-wide'),
  rear('Back Ultra Wide Camera', 'ultra-wide'),
  rear('Back Dual Camera', 'dual'),
  rear('Back Camera', 'back'),
  rear('Back Telephoto Camera', 'tele'),
];
const BUILT_IN: Camera = { id: 'built-in', label: 'Built-in Camera', facing: '' };
const USB: Camera = { id: 'usb', label: 'USB Camera', facing: '' };

/** A first-time visitor whose browser has these cameras, on a card page. */
async function visitor(browser: Browser, site: string, cameras: Camera[], listsBefore: boolean) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  await page.setViewport({ width: 420, height: 860 });
  const seen = watch(page);
  await pretendCameras(page, cameras, listsBefore);
  await go(page, site + '/brox_the_defiant/');
  await openScanner(page);
  return {
    page,
    seen,
    /** Press the switch button and wait for the scanner to settle on a live camera. */
    async press() {
      const before = (await asked(page)).length;
      await page.evaluate(() =>
        (document.querySelector('[data-qr-switch]') as HTMLElement).click()
      );
      await until('camera asked for', async () => (await asked(page)).length > before, 3000).catch(
        () => {},
      );
      await sleep(700);
      await until('camera live', async () => !!(await scanner(page)).showing);
      return await scanner(page);
    },
    /** The cameras shown after each of `times` presses, after the one showing now. */
    async presses(times: number) {
      const shown = [(await scanner(page)).showing];
      const mirrored = [(await scanner(page)).mirrored];
      for (let press = 0; press < times; press++) {
        const now = await this.press();
        shown.push(now.showing);
        mirrored.push(now.mirrored);
      }
      return { shown: shown.join(' > '), mirrored: mirrored.join(',') };
    },
    close: () => context.close(),
  };
}

export async function run(site: string, checks: Checks) {
  const browser = await launch();
  try {
    // An iPhone, which lists each rear lens as a camera of its own.
    {
      const v = await visitor(browser, site, IPHONE, false);
      const { shown, mirrored } = await v.presses(4);
      checks.check(
        'switch, an iPhone: the button changes between the front camera and the rear one the browser chooses',
        shown ===
          'Front Camera > Back Triple Camera > Front Camera > Back Triple Camera > Front Camera',
        shown,
      );
      const all = await asked(v.page);
      checks.check(
        'switch, an iPhone: a camera is asked for only by the way it faces',
        all.every((a) => !a.id),
        inWords(all),
      );
      checks.check(
        'switch, an iPhone: the picture is mirrored for the front camera only',
        mirrored === 'true,false,true,false,true',
        mirrored,
      );
      const chosen = await v.press();
      checks.check(
        'switch, an iPhone: the camera changed to is stored as the choice',
        chosen.stored === 'Back Triple Camera',
        chosen.stored,
      );
      await go(v.page, site + '/hilbert_king_of_avian_frogs/');
      await openScanner(v.page);
      const next = await scanner(v.page);
      const nextAsked = inWords(await asked(v.page));
      checks.check(
        'switch, an iPhone: the next page asks for a rear camera and shows it unmirrored',
        next.showing === 'Back Triple Camera' && !next.mirrored &&
          nextAsked === 'facing environment',
        `${next.showing}; ${nextAsked}`,
      );
      checks.check(
        'switch, an iPhone: no script errors',
        v.seen.problems.length === 0,
        v.seen.problems.slice(0, 2).join(' | '),
      );
      await v.close();
    }

    // A phone with one camera each way.
    {
      const v = await visitor(browser, site, [FRONT, rear('Back Camera', 'back')], false);
      const { shown } = await v.presses(3);
      checks.check(
        'switch, a phone with two cameras: the button changes between them',
        shown === 'Front Camera > Back Camera > Front Camera > Back Camera',
        shown,
      );
      await v.close();
    }

    // A computer with two cameras that do not say which way they face.
    {
      const v = await visitor(browser, site, [BUILT_IN, USB], true);
      const { shown } = await v.presses(3);
      checks.check(
        'switch, a computer: the button goes round its cameras',
        shown === 'Built-in Camera > USB Camera > Built-in Camera > USB Camera',
        shown,
      );
      const changes = inWords((await asked(v.page)).slice(1));
      checks.check(
        'switch, a computer: each change asks for a camera by its id',
        changes === 'id usb, id built-in, id usb',
        changes,
      );
      await v.close();
    }

    // A computer whose own camera says it faces the player, with a second that says nothing.
    {
      const v = await visitor(browser, site, [{ ...BUILT_IN, facing: 'user' }, USB], true);
      const { shown } = await v.presses(4);
      checks.check(
        'switch, a computer whose camera says it faces the player: the second camera is still reached',
        shown === 'Built-in Camera > USB Camera > Built-in Camera > USB Camera > Built-in Camera',
        shown,
      );
      // Asked once for one that faces away and given the same camera again, its cameras are gone round.
      const changes = inWords((await asked(v.page)).slice(1));
      checks.check(
        'switch, a computer whose camera says it faces the player: one that faces away is asked for only once',
        changes === 'facing environment, id usb, id built-in, id usb, id built-in',
        changes,
      );
      checks.check(
        'switch, a computer whose camera says it faces the player: no script errors',
        v.seen.problems.length === 0,
        v.seen.problems.slice(0, 2).join(' | '),
      );
      await v.close();
    }

    // A device with one camera.
    {
      const v = await visitor(browser, site, [FRONT], false);
      const once = await v.press();
      const twice = await v.press();
      checks.check(
        'switch, one camera: the button leaves it running',
        once.showing === 'Front Camera' && twice.showing === 'Front Camera' && twice.open,
        `${once.showing}, ${twice.showing}; asked ${inWords(await asked(v.page))}`,
      );
      checks.check(
        'switch, one camera: no script errors',
        v.seen.problems.length === 0,
        v.seen.problems.slice(0, 2).join(' | '),
      );
      await v.close();
    }
  } finally {
    await browser.close();
  }
}

if (import.meta.main) await alone(run);
