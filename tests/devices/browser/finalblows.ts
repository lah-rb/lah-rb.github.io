/**
 * A Final Blow in a room: rolled as a KEAL means bout between the two
 * cards' archetypal genes, and the same on both screens.
 *
 * Two browsers in a room. One player's card has every KEAL means injured,
 * so its fists tool offers Final Blows; the other attacks it. Both screens
 * must then show the dice the two cards themselves roll (not a D20 for
 * each player, which is what a Final Blow was until the rules changed), the
 * genes' matchup as the chart has it (not doubled), and what the motives
 * do in the rules' words (a first win nullified, not "2 of 3").
 *
 * Against the built site a relay is run here; against the live site the
 * browsers use the live relay (see sync.ts).
 *
 *   deno run -A browser/finalblows.ts [site]
 *
 * With SHOTS set to a folder, a picture of the attacker's result is left
 * there.
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, until } from './site.ts';
import { device, drawn, post, startRelay } from './sync.ts';

const ATTACKER = 'liliel_healing_fairy';
const DEFENDER = 'brox_the_defiant';

/** A field of a card's file. */
async function field(card: string, name: string) {
  const file = await Deno.readTextFile(new URL(`../../../cards/${card}.html`, import.meta.url));
  return file.match(new RegExp(`^${name}:\\s*"?([^"\\n]+?)"?\\s*$`, 'm'))?.[1] ?? '';
}

/** How many injuries a card's KEAL means can take in all. */
async function injuries(card: string) {
  const file = await Deno.readTextFile(new URL(`../../../cards/${card}.html`, import.meta.url));
  const means = file.split(/^keal_means:\s*$/m)[1]?.split(/^\S/m)[0] ?? '';
  return [...means.matchAll(/^\s+count:\s*(\d+)/gm)].reduce((sum, m) => sum + Number(m[1]), 0);
}

/** What a page's engine shows of the round, for the card its player plays. */
async function result(page: Page, card: string) {
  const html = await drawn(page, `/api/room/fists?card=${card}`);
  const rolls = (who: string) =>
    html.match(new RegExp(`data-rolls="${who}">Rolls: <strong>([^<]+)<`))?.[1] ?? '';
  return {
    shown: html.includes('data-modifier="attack"') && html.includes('Final Blows'),
    attack: html.match(/data-modifier="attack">([+-]?\d+)</)?.[1] ?? '',
    genes: html.match(/\((\w+ vs \w+ = [+-]?\d+)[,)]/)?.[1] ?? '',
    attacker: rolls('attacker'),
    defender: rolls('defender'),
    nullified: html.includes("Defender&#x27;s first win is nullified") ||
      html.includes("Defender's first win is nullified"),
    old: ['Both Players Roll', 'Damage Bonus', '×2', 'must win 2 of 3'].filter((text) =>
      html.includes(text)
    ),
  };
}

export async function run(site: string, checks: Checks) {
  const relay = /^https?:\/\/(localhost|127\.0\.0\.1)/.test(site) ? await startRelay() : null;
  const browser = await launch();
  try {
    const a = await device(browser, relay?.address ?? null);
    const b = await device(browser, relay?.address ?? null);

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

    // ── The defender's card is downed: its fists tool offers Final Blows ──

    for (let slot = 1; slot <= await injuries(DEFENDER); slot++) {
      await post(b.page, '/api/game/damage', `card=${DEFENDER}&slot=${slot}`);
    }
    await until(
      'Final Blows offered',
      async () => (await drawn(b.page, `/api/room/fists?card=${DEFENDER}`)).includes('State Final Blows'),
    );
    const offered = await drawn(b.page, `/api/room/fists?card=${DEFENDER}`);
    const own = await field(DEFENDER, 'die');
    checks.check(
      'final blows: the downed card is told to roll its own die, not a D20 for everyone',
      offered.includes(`data-rolls="final">${own}<`) && offered.includes('You Roll') &&
        !offered.includes('Both Players Roll'),
      own,
    );

    // ── It states Final Blows, the other player attacks ──

    await post(b.page, '/api/room/fists', `action=final&card=${DEFENDER}`);
    await post(a.page, '/api/room/fists', `action=choose&card=${ATTACKER}&role=attacking`);
    await post(a.page, '/api/room/fists', `action=choose&card=${ATTACKER}&keal=1`);
    await post(a.page, '/api/room/fists', `action=lock&card=${ATTACKER}`);
    await until(
      'both results',
      async () => (await result(a.page, ATTACKER)).shown && (await result(b.page, DEFENDER)).shown,
    );
    const [hers, his] = [await result(a.page, ATTACKER), await result(b.page, DEFENDER)];

    checks.check(
      'final blows: both screens show the same result',
      JSON.stringify(hers) === JSON.stringify(his),
      JSON.stringify({ hers, his }),
    );
    checks.check(
      'final blows: each card rolls its own dice',
      hers.attacker === await field(ATTACKER, 'atk_die') && hers.defender === own,
      JSON.stringify({ attacker: hers.attacker, defender: hers.defender }),
    );
    const [gene, against] = [
      await field(ATTACKER, 'genetic_disposition'),
      await field(DEFENDER, 'genetic_disposition'),
    ];
    const matchup = hers.genes.match(/= ([+-]?\d+)$/)?.[1] ?? '';
    checks.check(
      "final blows: the attack die's modifier is the genes' matchup, as the chart has it",
      hers.genes.startsWith(`${gene} vs ${against} = `) && matchup !== '' &&
        Number(hers.attack) === Number(matchup) && Math.abs(Number(matchup)) <= 3,
      JSON.stringify({ genes: hers.genes, attack: hers.attack }),
    );
    checks.check(
      "final blows: a Society attacker's effect is said in the rules' words, and nothing of the old display is left",
      hers.nullified && hers.old.length === 0,
      JSON.stringify({ nullified: hers.nullified, old: hers.old }),
    );

    const shots = Deno.env.get('SHOTS');
    if (shots) {
      // The dialog as a player sees it.
      await a.page.evaluate(() => {
        document.querySelector('.ghost-toolbar')?.dispatchEvent(
          new KeyboardEvent('keydown', { bubbles: true }),
        );
        (document.querySelector('button[aria-label="Toggle Fists Combat"]') as HTMLElement)?.click();
      });
      await new Promise((done) => setTimeout(done, 900));
      await a.page.screenshot({ path: `${shots}/final_blows_result.png` });
    }

    const problems = [...a.seen.problems, ...b.seen.problems];
    checks.check(
      'final blows: no script errors',
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
