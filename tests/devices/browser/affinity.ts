/**
 * Affinity in a round of fists: both players' screens give the same bonus.
 *
 * Two browsers in a room. Each player declares an affinity for the
 * archetype of the card they play, one attacks and the other defends, and
 * the two results must say the same: three more on the attack die for the
 * attacker's affinity, and a defence die modifier of three for the
 * defender's. Then a round in which only the defender has an affinity for
 * their card, which neither screen may give to the attacker.
 *
 * Against the built site a relay is run here; against the live site the
 * browsers use the live relay (see sync.ts).
 *
 *   deno run -A browser/affinity.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, until } from './site.ts';
import { device, drawn, post, startRelay } from './sync.ts';

const ATTACKER = 'brox_the_defiant';
const DEFENDER = 'liliel_healing_fairy';

/** The archetype a card's file gives it. */
async function archetypeOf(card: string) {
  const file = await Deno.readTextFile(new URL(`../../../cards/${card}.html`, import.meta.url));
  return file.match(/^genetic_disposition:\s*"?(\w+)"?\s*$/m)?.[1] ?? '';
}

/** What a page's engine shows of the round, for the card its player plays. */
async function result(page: Page, card: string) {
  const html = await drawn(page, `/api/room/fists?card=${card}`);
  const number = (die: string) =>
    html.match(new RegExp(`data-modifier="${die}">([+-]?\\d+)<`))?.[1] ?? '';
  const affinity = (who: string) =>
    html.match(new RegExp(`data-affinity="${who}">([^<]+)<`))?.[1] ?? '';
  return {
    shown: html.includes('Combat Result'),
    attack: number('attack'),
    defence: number('defense'),
    attacker: affinity('attacker'),
    defender: affinity('defender'),
  };
}

/** Pick a role and the card's first keal means, and lock them in. */
async function lockIn(page: Page, card: string, role: string) {
  await post(page, '/api/room/fists', `action=choose&card=${card}&role=${role}`);
  await post(page, '/api/room/fists', `action=choose&card=${card}&keal=1`);
  await post(page, '/api/room/fists', `action=lock&card=${card}`);
}

/** Play a round, the first player attacking, and give both results. */
async function round(a: Page, b: Page) {
  await lockIn(a, ATTACKER, 'attacking');
  await lockIn(b, DEFENDER, 'defending');
  await until(
    'both results',
    async () => (await result(a, ATTACKER)).shown && (await result(b, DEFENDER)).shown,
  );
  return [await result(a, ATTACKER), await result(b, DEFENDER)];
}

export async function run(site: string, checks: Checks) {
  const relay = /^https?:\/\/(localhost|127\.0\.0\.1)/.test(site) ? await startRelay() : null;
  const browser = await launch();
  try {
    const a = await device(browser, relay?.address ?? null);
    const b = await device(browser, relay?.address ?? null);
    const [hers, his] = [await archetypeOf(ATTACKER), await archetypeOf(DEFENDER)];

    await go(a.page, `${site}/${ATTACKER}/`);
    await go(b.page, `${site}/${DEFENDER}/`);
    await post(a.page, '/api/room/status', 'action=create&name=Table');
    await until(
      'a room',
      async () => /Code: <strong>[A-Z0-9]{4}</.test(await drawn(a.page, '/api/room/status')),
    );
    const code =
      (await drawn(a.page, '/api/room/status')).match(/Code: <strong>([A-Z0-9]{4})</)![1];
    await post(b.page, '/api/room/status', `action=join&code=${code}&name=Table`);
    await until(
      'both in the room',
      async () =>
        (await drawn(a.page, '/api/room/pill')).includes('>Connected<') &&
        (await drawn(b.page, '/api/room/pill')).includes('>Connected<'),
    );

    // ── Neither has declared: what the two keal means make of each other ──

    const [plainA, plainB] = await round(a.page, b.page);
    checks.check(
      'affinity: with none declared both screens show the same attack modifier and nothing of affinity',
      plainA.attack !== '' && JSON.stringify(plainA) === JSON.stringify(plainB) &&
        !plainA.attacker &&
        !plainA.defence,
      JSON.stringify(plainA),
    );
    const plain = Number(plainA.attack);

    // ── Both declare the archetype of the card they play ──

    await post(a.page, '/api/room/fists', 'action=reset');
    await until('a new round', async () => !(await result(b.page, DEFENDER)).shown);
    await post(a.page, '/api/player/affinity', `archetype=${hers}`);
    await post(b.page, '/api/player/affinity', `archetype=${his}`);
    const [bothA, bothB] = await round(a.page, b.page);
    checks.check(
      "affinity: the attacker's affinity adds three to the attack die, on both screens",
      Number(bothA.attack) === plain + 3 && bothA.attack === bothB.attack &&
        bothA.attacker === `+3 from affinity (${hers})` && bothB.attacker === bothA.attacker,
      JSON.stringify({ plain, a: bothA.attack, b: bothB.attack, line: bothB.attacker }),
    );
    checks.check(
      "affinity: the defender's affinity gives the defence die three, on both screens",
      bothA.defence === '+3' && bothB.defence === '+3' &&
        bothA.defender === `+3 from affinity (${his})` && bothB.defender === bothA.defender,
      JSON.stringify({ a: bothA.defence, b: bothB.defence, line: bothA.defender }),
    );

    // ── The attacker starts a new game: only the defender has an affinity now ──

    await post(a.page, '/api/room/fists', 'action=reset');
    await until('a new round', async () => !(await result(b.page, DEFENDER)).shown);
    await post(a.page, '/api/game/damage', 'action=new_game');
    // She declares the archetype of his card: nothing to hers, and nothing to his.
    await post(a.page, '/api/player/affinity', `archetype=${his}`);
    const [oneA, oneB] = await round(a.page, b.page);
    checks.check(
      "affinity: a player's affinity is never given to the other player's card",
      Number(oneA.attack) === plain && oneA.attack === oneB.attack && !oneA.attacker &&
        !oneB.attacker && oneA.defence === '+3' && oneB.defence === '+3',
      JSON.stringify({ a: oneA, b: oneB }),
    );

    const problems = [...a.seen.problems, ...b.seen.problems];
    checks.check(
      'affinity: no script errors',
      problems.length === 0,
      problems.slice(0, 3).join(' | '),
    );
    await post(a.page, '/api/room/status', 'action=leave');
    await post(b.page, '/api/room/status', 'action=leave');
    await a.close();
    await b.close();
  } finally {
    await browser.close();
    if (relay) await relay.stop();
  }
}

if (import.meta.main) await alone(run);
