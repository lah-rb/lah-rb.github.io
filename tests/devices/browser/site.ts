/**
 * What the browser checks share: the built site served locally, a browser to
 * drive, the tally of checks, and the things every check does to a page
 * (wait for it, open its scanner, read what the scanner shows).
 *
 * A check is a module that exports `run(site, checks)`. Run by itself it
 * serves the repository's `_site` and reports; `all.ts` runs them in turn.
 */
import puppeteer from 'puppeteer-core';
import type { Browser, Page } from 'puppeteer-core';

const REPO = new URL('../../../', import.meta.url).pathname;

/** Where Chrome is, unless CHROME says otherwise. */
const CHROME = Deno.env.get('CHROME') ||
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const FIREFOX = Deno.env.get('FIREFOX') || '/Applications/Firefox.app/Contents/MacOS/firefox';

/** BROWSER=firefox drives Firefox; anything else, Chrome. */
export const BROWSER = Deno.env.get('BROWSER') === 'firefox' ? 'firefox' : 'chrome';

export const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Wait until `test` says yes. A page that is changing is asked again. */
export async function until(what: string, test: () => Promise<boolean>, ms = 20000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      if (await test()) return;
    } catch (_) { /* the page is changing */ }
    await sleep(120);
  }
  throw new Error('timed out: ' + what);
}

// ── The site ───────────────────────────────────────────────────────

const TYPES: Record<string, string> = {
  html: 'text/html; charset=utf-8',
  js: 'text/javascript',
  mjs: 'text/javascript',
  css: 'text/css',
  json: 'application/json',
  wasm: 'application/wasm',
  svg: 'image/svg+xml',
  jxl: 'image/jxl',
  png: 'image/png',
  jpg: 'image/jpeg',
  ico: 'image/x-icon',
  webmanifest: 'application/manifest+json',
  woff2: 'font/woff2',
  txt: 'text/plain',
  xml: 'application/xml',
};

/**
 * Serve the built site (`_site`, as `deno task build` leaves it) on a local
 * port: /name is name.html and /name/ is name/index.html, as GitHub Pages
 * serves them. Nothing is cached, so a rebuilt site is the one checked.
 */
export function serveBuiltSite(port = 4317) {
  const server = Deno.serve(
    { port, hostname: '127.0.0.1', onListen: () => {} },
    async (request) => {
      let path = decodeURIComponent(new URL(request.url).pathname);
      if (path.includes('..')) return new Response('no', { status: 400 });
      if (path.endsWith('/')) path += 'index.html';
      for (const candidate of [path, path + '.html', path + '/index.html']) {
        try {
          const body = await Deno.readFile(`${REPO}_site${candidate}`);
          const type = TYPES[candidate.split('.').pop() || ''] || 'application/octet-stream';
          return new Response(body, {
            headers: { 'content-type': type, 'cache-control': 'no-store' },
          });
        } catch (_) { /* the next candidate */ }
      }
      return new Response('not found', { status: 404 });
    },
  );
  return { site: `http://localhost:${port}`, stop: () => server.shutdown() };
}

// ── The browser ────────────────────────────────────────────────────

/**
 * A browser of its own, headless, with nothing of the user's in it. It is
 * given a pretend camera that needs no permission; `pretendCameras` puts
 * named ones in a page instead.
 */
export function launch(): Promise<Browser> {
  if (BROWSER === 'firefox') {
    return puppeteer.launch({
      browser: 'firefox',
      executablePath: FIREFOX,
      headless: true,
      extraPrefsFirefox: {
        'media.navigator.streams.fake': true,
        'media.navigator.permission.disabled': true,
      },
    });
  }
  return puppeteer.launch({
    executablePath: CHROME,
    headless: true,
    args: [
      '--no-first-run',
      '--no-default-browser-check',
      '--use-fake-ui-for-media-stream',
      '--use-fake-device-for-media-stream',
    ],
  });
}

// ── The tally ──────────────────────────────────────────────────────

export class Checks {
  results: [string, boolean][] = [];

  check(name: string, ok: boolean, detail = '') {
    this.results.push([name, ok]);
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
  }

  note(text: string) {
    console.log(`      ${text}`);
  }

  get passed() {
    return this.results.filter(([, ok]) => ok).length;
  }

  get allPassed() {
    return this.passed === this.results.length;
  }
}

/** Script errors a page has had, as they happen. Ask it for `problems` at the end. */
export function watch(page: Page) {
  const problems: string[] = [];
  page.on('pageerror', (error) => problems.push(String((error as Error)?.message || error)));
  page.on('console', (message) => {
    if (message.type() !== 'error') return;
    if (/favicon|Failed to load resource/.test(message.text())) return;
    problems.push('[console] ' + message.text().slice(0, 200));
  });
  // Firefox under automation reports these of the automation's own frames.
  const ours = () =>
    problems.filter((p) => !/Permission denied to access property "(length|__bidi_args)"/.test(p));
  return {
    get problems() {
      return ours();
    },
  };
}

/**
 * Run one check by itself: against the site named on the command line (the
 * live one, say), or else against the built site, served here.
 */
export async function alone(run: (site: string, checks: Checks) => Promise<void>) {
  const given = Deno.args[0];
  const served = given ? null : serveBuiltSite();
  const site = given || served!.site;
  const checks = new Checks();
  try {
    await run(site, checks);
  } catch (error) {
    checks.check('the run finished', false, String(error));
  } finally {
    if (served) await served.stop();
  }
  console.log(`${checks.passed}/${checks.results.length} checks passed (${BROWSER}, ${site})`);
  Deno.exit(checks.allPassed ? 0 : 1);
}

// ── A page of the site ─────────────────────────────────────────────

type Engine = typeof globalThis & { kipukasViews?: unknown };

/** Wait until the page has its engine, its service worker and its toolbar. */
export function ready(page: Page) {
  return until(
    'page ready',
    () =>
      page.evaluate(() =>
        !!(globalThis as Engine).kipukasViews && !!navigator.serviceWorker.controller &&
        !!document.querySelector('button[aria-label="Toggle QR Scanner"]')
      ),
    40000,
  );
}

/** Go to a page of the site and wait for it. */
export async function go(page: Page, address: string) {
  await page.goto(address, { waitUntil: 'load' });
  await ready(page);
  // A first visit loads itself again once its service worker has the site.
  await sleep(700);
  await ready(page);
}

/** What the scanner on a page shows. */
export function scanner(page: Page) {
  return page.evaluate(() => {
    const holder = document.querySelector('[data-qr-camera]');
    const video = holder && holder.querySelector('video');
    const stream = video && video.srcObject instanceof MediaStream ? video.srcObject : null;
    const track = stream ? stream.getVideoTracks()[0] : null;
    let stored = '';
    try {
      stored = JSON.parse(localStorage.getItem('kipukas-scanner-camera') || '{}').label || '';
    } catch (_) { /* none */ }
    return {
      /** The camera privacy notice is up. */
      notice: /Privacy Notice/.test(document.getElementById('qr-container')?.textContent || ''),
      /** The camera's place is drawn. */
      open: !!holder,
      /** The name of the camera whose picture is live, or ''. */
      showing: track && track.readyState === 'live' ? track.label : '',
      /** The way the engine has the page ask for a camera, and whether the picture is mirrored. */
      facing: holder ? holder.getAttribute('data-camera-facing') || '' : '',
      mirrored: !!holder && holder.hasAttribute('data-mirrored'),
      /** The screen light is on. */
      lit: !!document.querySelector('#qr-container [aria-label="Turn the screen light off"]'),
      /** The name of the camera stored as the player's choice, or ''. */
      stored,
    };
  });
}

/** Press a control of the open scanner, named by what it posts to the engine. */
export function tap(page: Page, action: string) {
  return page.evaluate((action) => {
    const control = document.querySelector(`#qr-container [data-engine-body="action=${action}"]`);
    if (!(control instanceof HTMLElement)) throw new Error(`the scanner has no "${action}"`);
    control.click();
  }, action);
}

/** Press the toolbar's QR button. The toolbar is woken first, or it swallows the press. */
export function pressQR(page: Page) {
  return page.evaluate(() => {
    const bar = document.querySelector('.ghost-toolbar');
    if (bar) bar.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Shift' }));
    const button = document.querySelector('button[aria-label="Toggle QR Scanner"]');
    if (!(button instanceof HTMLElement)) throw new Error('no QR button on this page');
    button.click();
  });
}

/** Open the scanner on its camera, accepting the privacy notice if it asks. */
export async function openScanner(page: Page) {
  await pressQR(page);
  await until('notice or camera', async () => {
    const now = await scanner(page);
    return now.notice || now.open;
  });
  if ((await scanner(page)).notice) await tap(page, 'accept');
  await until('camera live', async () => !!(await scanner(page)).showing);
  await sleep(300);
}

// ── Pretend cameras ────────────────────────────────────────────────

export type Camera = { id: string; label: string; facing: '' | 'user' | 'environment' };
export type Asked = { id: string; facing: string; gave: string };
type Pretend = typeof globalThis & { __asked?: Asked[] };

/**
 * Give every page this tab loads these cameras in place of the browser's.
 * They are chosen between as a browser chooses: the camera of the id asked
 * for; else the first that faces the way asked for; else the first.
 *
 * `listsBefore`: the browser names its cameras before any has been allowed
 * on the page. Safari does not; a browser that keeps the permission does.
 */
export function pretendCameras(page: Page, cameras: Camera[], listsBefore: boolean) {
  return page.evaluateOnNewDocument(
    (cameras: Camera[], listsBefore: boolean) => {
      let allowed = false;
      const picture = () => {
        const canvas = document.createElement('canvas');
        canvas.width = 640;
        canvas.height = 480;
        const ctx = canvas.getContext('2d')!;
        let hue = 0;
        setInterval(() => {
          ctx.fillStyle = `hsl(${(hue += 7) % 360} 60% 50%)`;
          ctx.fillRect(0, 0, 640, 480);
        }, 100);
        return canvas;
      };
      const media = navigator.mediaDevices;
      media.enumerateDevices = () => {
        const named = cameras.map((camera) => ({
          kind: 'videoinput',
          deviceId: camera.id,
          label: camera.label,
          groupId: '',
        }));
        const unnamed = [{ kind: 'videoinput', deviceId: '', label: '', groupId: '' }];
        return Promise.resolve(
          (allowed || listsBefore ? named : unnamed) as unknown as MediaDeviceInfo[],
        );
      };
      media.getUserMedia = (constraints?: MediaStreamConstraints) => {
        const video = constraints && typeof constraints.video === 'object' ? constraints.video : {};
        const word = (said: unknown): string => {
          if (typeof said === 'string') return said;
          const terms = (said || {}) as { exact?: string; ideal?: string };
          return terms.exact || terms.ideal || '';
        };
        const id = word(video.deviceId);
        const facing = word(video.facingMode);
        const camera = cameras.find((c) => id && c.id === id) ||
          cameras.find((c) => facing && c.facing === facing) || cameras[0];
        allowed = true;
        ((globalThis as Pretend).__asked ||= []).push({ id, facing, gave: camera.label });
        const stream = picture().captureStream(10);
        const track = stream.getVideoTracks()[0];
        Object.defineProperty(track, 'label', { value: camera.label });
        track.getSettings = () => ({
          deviceId: camera.id,
          width: 640,
          height: 480,
          ...(camera.facing ? { facingMode: camera.facing } : {}),
        });
        return Promise.resolve(stream);
      };
    },
    cameras,
    listsBefore,
  );
}

/** What this page has asked of its pretend cameras, in order. */
export function asked(page: Page): Promise<Asked[]> {
  return page.evaluate(() => (globalThis as Pretend).__asked || []);
}

/** The same, in words: "facing user, id usb". */
export const inWords = (asked: Asked[]) =>
  asked.map((a) => (a.id ? `id ${a.id}` : `facing ${a.facing}`)).join(', ');
