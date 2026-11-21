/**
 * A game moved from one device to another while it is being played against
 * someone: the player's place in the room goes with it.
 *
 * Three browsers with nothing in common: the player's two devices, and the
 * opponent's. The first device makes a room and the opponent joins it; the
 * game is then moved to the second device, which must end up in the room
 * as the player, with the opponent none the wiser, and the first device
 * out of it. Then a game is moved the other way, on to the device that is
 * in the room, which must leave the room and keep none of its timers.
 *
 * Against the built site a relay is run here; against the live site the
 * browsers use the live relay (see sync.ts).
 *
 *   deno run -A browser/seat.ts [site]
 */
import type { Page } from 'puppeteer-core';
import type { Checks } from './site.ts';
import { alone, go, launch, until } from './site.ts';
import { device, drawn, invite, post, press, says, section, startRelay } from './sync.ts';

const CARD = 'brox_the_defiant';

/** The room a page's engine is in: its code, and what the pill says. */
async function room(page: Page) {
  const dialog = await drawn(page, '/api/room/status');
  const pill = await drawn(page, '/api/room/pill');
  return {
    code: dialog.match(/Code: <strong>([A-Z0-9]{4})<\/strong>/)?.[1] ?? '',
    connected: pill.includes('>Connected<'),
    /** What the pill says when it says something, or nothing. */
    pill: pill.replace(/<[^>]+>/g, '').trim(),
  };
}

/** The names of the timers a page's engine lists: the room's, in a room. */
async function timers(page: Page) {
  const list = await drawn(page, '/api/game/turns?display=alarms');
  return ['siege', 'theirs', 'from the opponent', 'from the tablet', 'ambush']
    .filter((name) => list.includes(name));
}

/** Move the game on `from` to `to`, by the code typed. Returns what the two were asked. */
async function move(from: Page, to: Page) {
  await press(from, 'ask');
  await until('asked', async () => !!(await section(from)).question);
  const asked = (await section(from)).text;
  await press(from, 'close');
  await until('put away', async () => !(await section(from)).question);
  const shown = await invite(from);
  await to.type('input[name="invite"]', shown.code);
  await to.keyboard.press('Enter');
  await until('asked', async () => !!(await section(to)).question);
  const offered = (await section(to)).text;
  await press(to, 'receive');
  await says(to, 'This device now has the game from your other device.');
  await says(from, 'The other device now has this game.');
  return { asked, offered };
}

export async function run(site: string, checks: Checks) {
  const relay = /^https?:\/\/(localhost|127\.0\.0\.1)/.test(site) ? await startRelay() : null;
  const browser = await launch();
  try {
    const phone = await device(browser, relay?.address ?? null);
    const tablet = await device(browser, relay?.address ?? null);
    const opponent = await device(browser, relay?.address ?? null);

    // The phone makes a room, the opponent joins it, and the game begins:
    // damage on the card's second slot, and a timer the room holds.
    await go(phone.page, `${site}/${CARD}/`);
    await go(opponent.page, `${site}/${CARD}/`);
    await post(phone.page, '/api/room/status', 'action=create&name=Table');
    await until('a room', async () => !!(await room(phone.page)).code);
    const code = (await room(phone.page)).code;
    await post(opponent.page, '/api/room/status', `action=join&code=${code}&name=Table`);
    await until(
      'both in the room',
      async () => (await room(phone.page)).connected && (await room(opponent.page)).connected,
    );
    await post(phone.page, '/api/game/damage', `card=${CARD}&slot=2`);
    await post(phone.page, '/api/game/turns', 'action=add&turns=5&name=siege&color_set=red');
    await until('the timer', async () => (await timers(opponent.page)).includes('siege'));
    // The tablet has a game of its own, alone.
    await go(tablet.page, `${site}/${CARD}/`);
    await post(tablet.page, '/api/game/turns', 'action=add&turns=2&name=theirs&color_set=blue');
    checks.check(
      'seat: the phone and the opponent are in a room, and share a timer',
      /^[A-Z0-9]{4}$/.test(code) && (await timers(phone.page)).join() === 'siege',
      code.replace(/./g, 'X'),
    );

    // ── The game goes to the tablet, and the place in the room with it ──

    await go(phone.page, `${site}/player_data`);
    await go(tablet.page, `${site}/player_data`);
    await until('back in the room', async () => (await room(phone.page)).connected);
    const first = await move(phone.page, tablet.page);
    checks.check(
      'seat: the sender is told its place in the room goes with the game',
      first.asked.includes(`Your place in room ${code} goes with the game`),
      first.asked.slice(0, 260),
    );
    const told = [(await section(tablet.page)).text, (await section(phone.page)).text];
    checks.check(
      'seat: both say where the place went',
      told[0].includes(`It has taken your place in room ${code}.`) &&
        told[1].includes(`It has your place in room ${code}, and this device has left the room.`),
      told.join(' | ').slice(0, 300),
    );
    await until('the tablet in the room', async () => (await room(tablet.page)).connected);
    const after = {
      tablet: await room(tablet.page),
      phone: await room(phone.page),
      opponent: await room(opponent.page),
    };
    checks.check(
      'seat: the tablet is in the room with the opponent, and the phone is in none',
      after.tablet.code === code && after.tablet.connected && after.opponent.connected &&
        !after.phone.code && !after.phone.pill,
      JSON.stringify({
        tablet: after.tablet.connected,
        phone: after.phone.pill,
        opponent: after.opponent.pill,
      }),
    );
    checks.check(
      "seat: the tablet has the phone's game and the room's timer, and none of its own",
      (await timers(tablet.page)).join() === 'siege' &&
        (await drawn(tablet.page, `/api/game/damage?card=${CARD}`)).match(/aria-pressed="true"/g)
            ?.length === 1,
      (await timers(tablet.page)).join(),
    );

    // The room goes on between the tablet and the opponent, both ways.
    await post(
      opponent.page,
      '/api/game/turns',
      'action=add&turns=4&name=from the opponent&color_set=green',
    );
    await until(
      "the opponent's timer",
      async () => (await timers(tablet.page)).includes('from the opponent'),
    );
    await post(
      tablet.page,
      '/api/game/turns',
      'action=add&turns=3&name=from the tablet&color_set=blue',
    );
    await until(
      "the tablet's timer",
      async () => (await timers(opponent.page)).includes('from the tablet'),
    );
    checks.check(
      'seat: a timer either makes reaches the other, and none reaches the phone',
      (await timers(phone.page)).join() === 'siege',
      (await timers(phone.page)).join(),
    );

    // It is stored: the next page on each device finds it as it was left.
    await go(tablet.page, `${site}/${CARD}/`);
    await go(phone.page, `${site}/${CARD}/`);
    await until('the tablet back in the room', async () => (await room(tablet.page)).connected);
    checks.check(
      'seat: on the next page the tablet is still in the room and the phone still out of it',
      (await room(tablet.page)).code === code && !(await room(phone.page)).code &&
        (await timers(tablet.page)).length === 3,
      (await timers(tablet.page)).join(),
    );

    // ── A game taken by a device that is in a room ──

    // The phone plays on alone, and its game is moved to the tablet.
    await post(phone.page, '/api/game/turns', 'action=add&turns=3&name=ambush&color_set=green');
    await go(phone.page, `${site}/player_data`);
    await go(tablet.page, `${site}/player_data`);
    await until('the tablet back in the room', async () => (await room(tablet.page)).connected);
    const second = await move(phone.page, tablet.page);
    checks.check(
      'seat: a device in a room is told it will leave it, and a sender in none says nothing of rooms',
      second.offered.includes(`It will leave room ${code}.`) && !second.asked.includes('room'),
      second.offered.slice(-200),
    );
    await until('the opponent alone', async () => !(await room(opponent.page)).connected);
    const alone = { tablet: await room(tablet.page), opponent: await room(opponent.page) };
    checks.check(
      'seat: the tablet has left the room, and the opponent is told',
      !alone.tablet.code && !alone.tablet.pill && alone.opponent.code === code,
      JSON.stringify({ tablet: alone.tablet.pill, opponent: alone.opponent.pill }),
    );
    checks.check(
      "seat: the tablet's timers are the phone's, and none of the room's",
      (await timers(tablet.page)).join() === 'siege,ambush',
      (await timers(tablet.page)).join(),
    );
    await go(tablet.page, `${site}/${CARD}/`);
    checks.check(
      'seat: and still are on the next page',
      (await timers(tablet.page)).join() === 'siege,ambush' && !(await room(tablet.page)).code,
      (await timers(tablet.page)).join(),
    );

    const problems = [...phone.seen.problems, ...tablet.seen.problems, ...opponent.seen.problems];
    checks.check('seat: no script errors', problems.length === 0, problems.slice(0, 3).join(' | '));
    // The opponent leaves, which ends the room on the relay.
    await post(opponent.page, '/api/room/status', 'action=leave');
    await phone.close();
    await tablet.close();
    await opponent.close();
  } finally {
    await browser.close();
    if (relay) await relay.stop();
  }
}

if (import.meta.main) await alone(run);
