/**
 * Whether a species is under the player's control: the stack on its page.
 *
 * One browser. A species nobody has played is rogue. Card effects are
 * added with the page's own button, its archetype is declared in the
 * affinity tool's way, and the species is under control once the stack
 * reaches its tameability. New Game takes both away again.
 *
 * (Loyalty, the third part of the stack, comes of days of play in a room:
 * the engine's own tests cover it.)
 *
 *   deno run -A browser/control.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, until } from './site.ts';
import { device, post } from './sync.ts';

/** A species of tameability 5, one that needs no taming, and a character. */
const SPECIES = 'knightsoul_of_binding_time';
const FREE = 'balanced_inline_processing_colony';
const CHARACTER = 'brox_the_defiant';

/** What a card's page says of the player's control of it. */
function control(page: Page) {
  return page.evaluate(() => {
    const block = document.querySelector('[data-control]');
    const part = (name: string) =>
      block?.querySelector(`[data-stack="${name}"]`)?.textContent?.trim() ?? '';
    return {
      standing: block?.getAttribute('data-control') ?? '',
      said: block?.querySelector('p')?.textContent?.replace(/\s+/g, ' ').trim() ?? '',
      /** The bar: each square's colour, or `-` for one not filled. */
      bar: [...(block?.querySelectorAll('[role="img"] .flex-1') ?? [])].map((square) =>
        square.classList.contains('bg-amber-400')
          ? 'a'
          : square.classList.contains('bg-red-500')
          ? 'e'
          : square.classList.contains('bg-sky-400')
          ? 'l'
          : '-'
      ).join(''),
      affinity: part('affinity'),
      effects: part('effects'),
      loyalty: part('loyalty'),
      hint: [...(block?.querySelectorAll('p') ?? [])].map((p) => p.textContent ?? '')
        .find((text) => text.startsWith('Declaring')) ?? '',
    };
  });
}

/** Press one of the two card-effects buttons on the page. */
function step(page: Page, label: string) {
  return page.evaluate((label) => {
    const button = document.querySelector(`[data-control] button[aria-label="${label}"]`);
    if (!(button instanceof HTMLButtonElement)) throw new Error(`no "${label}" button`);
    button.click();
  }, label);
}

export async function run(site: string, checks: Checks) {
  const browser = await launch();
  try {
    const { page, seen, close } = await device(browser, null);

    await go(page, `${site}/${SPECIES}/`);
    await until('the stack', async () => !!(await control(page)).standing);
    const first = await control(page);
    checks.check(
      'control: a species nobody has played is rogue, with nothing in its stack',
      first.standing === 'rogue' && first.said === 'Rogue 0 of 5' && first.bar === '----------' &&
        first.affinity === '0' && first.effects === '0' && first.loyalty === '0 of 4',
      JSON.stringify(first),
    );
    const archetype = first.hint.match(/^Declaring (\w+) as your affinity for this game adds 3\.$/)
      ?.[1] ?? '';
    checks.check(
      'control: it says what declaring its archetype would add',
      !!archetype,
      first.hint,
    );

    // ── Card effects, by the page's own buttons ──

    await step(page, 'One more from card effects');
    await until('one', async () => (await control(page)).effects === '1');
    await step(page, 'One more from card effects');
    await until('two', async () => (await control(page)).effects === '2');
    const two = await control(page);
    checks.check(
      'control: the plus button adds a point from card effects, and the bar shows it',
      two.said === 'Rogue 2 of 5' && two.bar === 'ee--------',
      JSON.stringify(two),
    );
    await step(page, 'One more from card effects');
    await until('three', async () => (await control(page)).effects === '3');
    const most = await page.evaluate(() =>
      (document.querySelector(
        '[data-control] button[aria-label="One more from card effects"]',
      ) as HTMLButtonElement).disabled
    );
    await step(page, 'One less from card effects');
    await until('two again', async () => (await control(page)).effects === '2');
    checks.check('control: card effects stop at three, and the minus button takes one back', most);

    // ── Affinity, declared as the affinity tool declares it ──

    await post(page, '/api/player/affinity', `archetype=${archetype}`);
    await until('held', async () => (await control(page)).standing === 'held');
    const held = await control(page);
    checks.check(
      'control: with its archetype declared the stack reaches its tameability, and it is under control',
      held.said.endsWith('Under your control 5 of 5') && held.bar === 'aaaee-----' &&
        held.affinity === '3' && !held.hint,
      JSON.stringify(held),
    );

    // It is stored: the next page has it.
    await go(page, `${site}/${SPECIES}/`);
    await until('the stack', async () => !!(await control(page)).standing);
    checks.check(
      'control: the stack is as it was on the next page',
      (await control(page)).bar === 'aaaee-----',
      (await control(page)).bar,
    );

    // ── New Game ──

    await post(page, '/api/game/damage', 'action=new_game');
    await go(page, `${site}/${SPECIES}/`);
    await until('the stack', async () => !!(await control(page)).standing);
    const cleared = await control(page);
    checks.check(
      'control: New Game takes the affinity and the card effects away',
      cleared.standing === 'rogue' && cleared.bar === '----------',
      JSON.stringify(cleared),
    );

    // ── What needs no taming ──

    await go(page, `${site}/${FREE}/`);
    await until('the stack', async () => !!(await control(page)).standing);
    const free = await control(page);
    await go(page, `${site}/${CHARACTER}/`);
    await until(
      'the damage tracker',
      () => page.evaluate(() => !!document.querySelector('[data-engine-post="/api/game/damage"]')),
    );
    const character = await control(page);
    checks.check(
      'control: a species of tameability 0 needs no taming, and a character says nothing of it',
      free.standing === 'free' && free.said === 'Needs no taming' && !free.bar &&
        !character.standing,
      JSON.stringify({ free: free.said, character: character.standing }),
    );

    checks.check(
      'control: no script errors',
      seen.problems.length === 0,
      seen.problems.slice(0, 3).join(' | '),
    );
    await close();
  } finally {
    await browser.close();
  }
}

if (import.meta.main) await alone(run);
