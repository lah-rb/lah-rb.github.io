/**
 * A copy of a card handed to the other player, in a room.
 *
 * Two browsers in a room. One has a card in play with damage on it and
 * hands it to the other: it leaves the first player's cards as it is and
 * arrives among the second's, marked as having come from the opponent.
 * Handed back, it is the first player's own again.
 *
 * Against the built site a relay is run here; against the live site the
 * browsers use the live relay (see sync.ts).
 *
 *   deno run -A browser/handover.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, until } from './site.ts';
import { press, shown } from './copies.ts';
import { device, drawn, post, startRelay } from './sync.ts';

const CARD = 'brox_the_defiant';
const OTHER = 'liliel_healing_fairy';
const HAND = 'Hand this copy to your opponent';

const connected = async (page: Page) =>
  (await drawn(page, '/api/room/pill')).includes('>Connected<');

/** Hand the copy a page shows to the opponent: asked, then yes. Returns the question. */
async function handOver(page: Page) {
  await until('the button', async () => (await shown(page)).offers.includes(HAND));
  await press(page, HAND);
  await until('asked', async () => !!(await shown(page)).asking);
  const asked = (await shown(page)).asking;
  await press(page, HAND);
  await until('handed over', async () => !(await shown(page)).asking);
  return asked;
}

export async function run(site: string, checks: Checks) {
  const relay = /^https?:\/\/(localhost|127\.0\.0\.1)/.test(site) ? await startRelay() : null;
  const browser = await launch();
  try {
    const a = await device(browser, relay?.address ?? null);
    const b = await device(browser, relay?.address ?? null);

    // Alone, there is nobody to hand a copy to.
    await go(a.page, `${site}/${CARD}/`);
    await until('the control', async () => (await shown(a.page)).copies === 1);
    const offeredAlone = (await shown(a.page)).offers;

    await go(b.page, `${site}/${OTHER}/`);
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
      async () => (await connected(a.page)) && (await connected(b.page)),
    );
    await until('the button', async () => (await shown(a.page)).offers.includes(HAND));
    checks.check(
      'handover: the button to hand a copy over is there only with the opponent in the room',
      !offeredAlone.includes(HAND) && (await shown(a.page)).offers.includes(HAND),
      JSON.stringify({ alone: offeredAlone, inRoom: (await shown(a.page)).offers }),
    );

    // ── From the first player to the second ──

    await post(a.page, '/api/game/damage', `card=${CARD}&slot=1`);
    await go(a.page, `${site}/${CARD}/`);
    await until('the damage', async () => (await shown(a.page)).marked.startsWith('x'));
    await until(
      'back in the room',
      async () => (await connected(a.page)) && (await connected(b.page)),
    );
    const asked = await handOver(a.page);
    await until('it has left', async () => !(await shown(a.page)).marked.includes('x'));
    await until('taken', async () => !(await shown(a.page)).onItsWay);
    const gave = await shown(a.page);
    checks.check(
      'handover: it is asked about first, and then the copy has left the giver as it was',
      asked.startsWith('Hand this card to your opponent?') && gave.copies === 1 &&
        !gave.marked.includes('x') && !gave.guest,
      JSON.stringify({ asked, gave: gave.marked }),
    );

    await go(b.page, `${site}/${CARD}/`);
    await until('the control', async () => (await shown(b.page)).copies > 0);
    const got = await shown(b.page);
    checks.check(
      "handover: it is among the other player's cards, with its damage, and says where it came from",
      got.copies === 1 && got.marked.startsWith('x') && got.guest,
      JSON.stringify({ copies: got.copies, marked: got.marked, guest: got.guest }),
    );

    // ── And back ──

    await until(
      'back in the room',
      async () => (await connected(a.page)) && (await connected(b.page)),
    );
    // It takes more damage while it is away.
    await post(b.page, '/api/game/damage', `card=${CARD}&slot=2`);
    await go(b.page, `${site}/${CARD}/`);
    await until('the damage', async () => (await shown(b.page)).marked.startsWith('xx'));
    await until(
      'back in the room',
      async () => (await connected(a.page)) && (await connected(b.page)),
    );
    await handOver(b.page);
    await until('it is back', async () => (await shown(a.page)).marked.startsWith('xx'), 30000);
    const home = await shown(a.page);
    const left = await shown(b.page);
    checks.check(
      "handover: handed back, it is its owner's own again, as it now is",
      home.copies === 1 && home.marked.startsWith('xx') && !home.guest &&
        !left.marked.includes('x') &&
        !left.guest,
      JSON.stringify({ home: home.marked, guest: home.guest, left: left.marked }),
    );

    // It is stored on both: the next pages have it as it was left.
    await go(a.page, `${site}/${CARD}/`);
    await go(b.page, `${site}/${CARD}/`);
    await until(
      'the control',
      async () => (await shown(a.page)).copies > 0 && (await shown(b.page)).copies > 0,
    );
    checks.check(
      'handover: the next page on each device has it as it was left',
      (await shown(a.page)).marked.startsWith('xx') && !(await shown(b.page)).marked.includes('x'),
      JSON.stringify({ a: (await shown(a.page)).marked, b: (await shown(b.page)).marked }),
    );

    const problems = [...a.seen.problems, ...b.seen.problems];
    checks.check(
      'handover: no script errors',
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
