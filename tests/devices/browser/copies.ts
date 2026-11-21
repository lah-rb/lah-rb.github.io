/**
 * More than one copy of a card in play.
 *
 * One browser. A card's page has a button at the lower left that puts
 * another copy in play. Each copy takes its own damage, the page's address
 * says which is shown, and a copy can be taken out again, which is asked
 * about first.
 *
 *   deno run -A browser/copies.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, sleep, until } from './site.ts';
import { device, post } from './sync.ts';

const CARD = 'brox_the_defiant';
const TRAP = 'timebattle';

/** What a card's page shows of its copies, and of the damage of the one shown. */
function shown(page: Page) {
  return page.evaluate(() => {
    const view = document.querySelector('[data-engine-view^="/api/card/copies"]');
    const control = view?.querySelector('[data-copies]');
    const marked = [...document.querySelectorAll('[data-card-detail] [aria-pressed]')]
      .filter((slot) => (slot.getAttribute('aria-label') || '').includes(' damage '))
      .map((slot) => (slot.getAttribute('aria-pressed') === 'true' ? 'x' : '-')).join('');
    return {
      copies: Number(control?.getAttribute('data-copies') ?? 0),
      copy: Number(control?.getAttribute('data-copy') ?? 0),
      /** The labels of what the control offers, in order. */
      offers: [...(control?.querySelectorAll('a, button') ?? [])].map((b) =>
        b.getAttribute('aria-label') || ''
      ),
      asking: control?.querySelector('[role="alertdialog"]')?.textContent?.replace(/\s+/g, ' ')
        .trim() ?? '',
      /** Where the control is: its distance from the left and the bottom of the window. */
      at: control
        ? [
          Math.round(control.getBoundingClientRect().left),
          Math.round(innerHeight - control.getBoundingClientRect().bottom),
        ]
        : [],
      marked,
      hash: location.hash,
    };
  });
}

/** Press what the control offers under this label. */
function press(page: Page, label: string) {
  return page.evaluate((label) => {
    const offered = [...document.querySelectorAll('[data-copies] a, [data-copies] button')]
      .find((b) => (b.getAttribute('aria-label') || '').startsWith(label));
    if (!(offered instanceof HTMLElement)) throw new Error(`the control offers no "${label}"`);
    offered.click();
  }, label);
}

/** Mark the first damage slot of the copy the page shows. */
function hurt(page: Page) {
  return page.evaluate(() => {
    const slot = [...document.querySelectorAll('[data-card-detail] [aria-pressed]')]
      .find((slot) => (slot.getAttribute('aria-label') || '').includes(' damage '));
    if (!(slot instanceof HTMLElement)) throw new Error('no damage slot on the page');
    slot.click();
  });
}

export async function run(site: string, checks: Checks) {
  const browser = await launch();
  try {
    const { page, seen, close } = await device(browser, null);

    await go(page, `${site}/${CARD}/`);
    await until('the control', async () => (await shown(page)).copies === 1);
    const one = await shown(page);
    checks.check(
      'copies: a card that fights has one button at the lower left, to put another copy in play',
      one.offers.join() === 'Put another copy of this card in play' && one.at[0] <= 16 &&
        one.at[1] <= 16,
      JSON.stringify({ offers: one.offers, at: one.at }),
    );

    // ── Another copy ──

    await press(page, 'Put another copy');
    await until('the second copy', async () => (await shown(page)).copy === 2);
    const two = await shown(page);
    checks.check(
      'copies: the button adds a copy and the page turns to it, by its address',
      two.copies === 2 && two.hash === '#2' && !two.marked.includes('x') &&
        two.offers[0] === 'Copy 2 of 2. Show the next copy',
      JSON.stringify(two),
    );

    // Each takes its own damage.
    await hurt(page);
    await until('damage', async () => (await shown(page)).marked.startsWith('x'));
    await press(page, 'Copy 2 of 2');
    await until('the first copy', async () => (await shown(page)).copy === 1);
    const first = await shown(page);
    await press(page, 'Copy 1 of 2');
    await until('the second copy', async () => (await shown(page)).copy === 2);
    const second = await shown(page);
    checks.check(
      'copies: each copy has its own damage, and the copy tile turns from one to the next',
      first.hash === '#1' && !first.marked.includes('x') && second.hash === '#2' &&
        second.marked.startsWith('x'),
      JSON.stringify({ first: first.marked, second: second.marked }),
    );

    // The address is all that says which copy: a page loaded with it shows it.
    await go(page, `${site}/${CARD}/#2`);
    await until('the second copy', async () => (await shown(page)).copy === 2);
    const loaded = await shown(page);
    await go(page, `${site}/${CARD}/`);
    await until('the first copy', async () => (await shown(page)).copy === 1);
    checks.check(
      "copies: a page loaded at a copy's address shows that copy, and the plain address the first",
      loaded.marked.startsWith('x') && !(await shown(page)).marked.includes('x') &&
        (await shown(page)).copies === 2,
      JSON.stringify({ loaded: loaded.marked }),
    );

    // ── Taking one out ──

    await go(page, `${site}/${CARD}/#2`);
    await until('the second copy', async () => (await shown(page)).copy === 2);
    await press(page, 'Take this copy out of play');
    await until('asked', async () => !!(await shown(page)).asking);
    const asked = await shown(page);
    await press(page, 'Keep this copy');
    await until('kept', async () => !(await shown(page)).asking);
    const kept = await shown(page);
    checks.check(
      'copies: taking a copy out is asked about first, and no keeps it',
      asked.asking.startsWith('Take copy 2 out of play?') && asked.copies === 2 &&
        kept.copies === 2 && kept.marked.startsWith('x'),
      asked.asking,
    );
    await press(page, 'Take this copy out of play');
    await until('asked', async () => !!(await shown(page)).asking);
    await press(page, 'Take this copy out of play');
    await until('one copy', async () => (await shown(page)).copies === 1);
    const out = await shown(page);
    checks.check(
      'copies: yes takes it out with its damage, and the page turns to the copy that is left',
      out.copy === 1 && out.hash === '#1' && !out.marked.includes('x') &&
        out.offers.join() === 'Put another copy of this card in play',
      JSON.stringify(out),
    );

    // ── New Game, and a card that does not fight ──

    await press(page, 'Put another copy');
    await until('two copies', async () => (await shown(page)).copies === 2);
    // New Game, as the toolbar's button asks for it: the engine clears the
    // game, then every view on the page is drawn again. The address still
    // says the second copy, of which there is none now.
    await post(page, '/api/game/damage', 'action=new_game');
    await page.evaluate(() =>
      (globalThis as unknown as { kipukasViews: { refresh(): Promise<unknown> } }).kipukasViews
        .refresh()
    );
    await until('one copy', async () => (await shown(page)).copies === 1);
    const cleared = await shown(page);
    await go(page, `${site}/${TRAP}/`);
    await sleep(800);
    const trap = await shown(page);
    checks.check(
      'copies: New Game leaves one copy, and a card that does not fight has no such button',
      cleared.copies === 1 && cleared.copy === 1 && trap.copies === 0,
      JSON.stringify({ cleared: cleared.copies, trap: trap.copies }),
    );

    checks.check(
      'copies: no script errors',
      seen.problems.length === 0,
      seen.problems.slice(0, 3).join(' | '),
    );
    await close();
  } finally {
    await browser.close();
  }
}

if (import.meta.main) await alone(run);
