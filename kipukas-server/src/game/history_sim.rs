//! Made-up histories, played through the engine, to see what a sync sum
//! says of them (`player_doc::take_over`, `Merge::distance`).
//!
//! The question it was written for (the owner's, November 2026): can a
//! sync sum tell an odd history from a normal one at a glance? Each
//! persona below is a way of using the site, normal or odd. It is played
//! with real documents: every page load is a new document restored from
//! the stored state, as in a browser, and every sync or import is a real
//! `take_over`. Each take-over is then put through every formula under
//! trial, and what each adds up to is printed, persona by persona.
//!
//! Nothing here checks the engine, and nothing in the engine uses it. It
//! runs only when asked for:
//!
//!   cargo test --release --lib history_sim -- --ignored --nocapture
//!
//! To try another formula, add it to `FORMULAS`. To try another way of
//! using the site, add it to `PERSONAS`.

use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use yrs::updates::decoder::Decode;
use yrs::StateVector;

use crate::clock;
use crate::game::player_doc as doc;

// ── What two histories had of each other when they met ─────────────

/// One take-over, as the two state vectors and the two records tell it.
#[derive(Clone, Copy, Debug)]
struct Met {
    /// Changes the state taken held that the device had not seen.
    gained: f64,
    /// Changes the device held that the state taken had not.
    held: f64,
    /// Changes both held.
    shared: f64,
    /// `2 H(A,B) - H(A) - H(B)` in bits, where a history's entropy is that
    /// of who wrote it: each page load writes under an id of its own.
    writers: f64,
    /// Days of play the state taken brought to the device's record.
    days_here: f64,
    /// Days of play the device added to the record it took.
    days_there: f64,
}

/// The smaller of two sizes over the larger, or nothing where one is nothing.
fn ratio(a: f64, b: f64) -> f64 {
    if a.max(b) == 0.0 {
        0.0
    } else {
        a.min(b) / a.max(b)
    }
}

impl Met {
    /// The size of the device's history, and of the one it took.
    fn sizes(&self) -> (f64, f64) {
        (self.held + self.shared, self.gained + self.shared)
    }

    /// What the engine keeps now: the changes only one side held, times the
    /// smaller of the two sides over the larger. In whole changes, as kept.
    fn built(&self) -> f64 {
        ((self.gained + self.held) * ratio(self.gained, self.held)).floor()
    }

    /// The owner's formula with a history's entropy counted in changes:
    /// `H(A)` is how many changes A holds and `H(A,B)` how many the two
    /// hold between them, so `2 H(A,B) - H(A) - H(B)` is `gained + held`.
    /// The ratio is of the two whole histories.
    fn counted(&self) -> f64 {
        let (mine, theirs) = self.sizes();
        (self.gained + self.held) * ratio(mine, theirs)
    }

    /// The owner's formula in bits: a history's entropy is that of one of
    /// its changes drawn at random, `log2` of how many it holds. The ratio
    /// is of the two whole histories.
    fn bits(&self) -> f64 {
        let (mine, theirs) = self.sizes();
        if mine == 0.0 || theirs == 0.0 {
            return 0.0;
        }
        let both = self.gained + self.held + self.shared;
        (2.0 * both.log2() - mine.log2() - theirs.log2()) * ratio(mine, theirs)
    }

    /// The owner's formula in bits, a history's entropy being that of who
    /// wrote it. The ratio is of the two whole histories.
    fn writer_bits(&self) -> f64 {
        let (mine, theirs) = self.sizes();
        self.writers * ratio(mine, theirs)
    }

    /// The built formula asked of the two records and not of the two
    /// histories: the days of play only one side had, times the smaller of
    /// the two over the larger.
    fn record(&self) -> f64 {
        (self.days_here + self.days_there) * ratio(self.days_here, self.days_there)
    }
}

type Formula = (&'static str, fn(&Met) -> f64);

const FORMULAS: [Formula; 5] = [
    ("built", Met::built),
    ("counted", Met::counted),
    ("bits", Met::bits),
    ("writer bits", Met::writer_bits),
    ("record", Met::record),
];

fn state_vector(encoded: &str) -> StateVector {
    StateVector::decode_v1(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap()
}

fn total(vector: &StateVector) -> f64 {
    vector.iter().map(|(_, clock)| *clock as f64).sum()
}

/// The entropy of who wrote a history, in bits.
fn entropy(vector: &StateVector) -> f64 {
    let all = total(vector);
    vector
        .iter()
        .filter(|(_, clock)| **clock > 0)
        .map(|(_, clock)| {
            let share = *clock as f64 / all;
            -share * share.log2()
        })
        .sum()
}

fn met(mine: &StateVector, theirs: &StateVector, taken: &doc::Taken) -> Met {
    let ahead = |a: &StateVector, b: &StateVector| -> f64 {
        a.iter().map(|(client, clock)| clock.saturating_sub(b.get(client)) as f64).sum()
    };
    let shared: f64 = mine.iter().map(|(client, clock)| (*clock).min(theirs.get(client)) as f64).sum();
    let mut both = mine.clone();
    for (client, clock) in theirs.iter() {
        both.set_max(*client, *clock);
    }
    let met = Met {
        gained: ahead(theirs, mine),
        held: ahead(mine, theirs),
        shared,
        writers: 2.0 * entropy(&both) - entropy(mine) - entropy(theirs),
        days_here: taken.days_new_here as f64,
        days_there: taken.days_new_there as f64,
    };
    assert_eq!((met.gained as u32, met.held as u32), (taken.gained, taken.held));
    met
}

// ── Devices, and what is done on them ──────────────────────────────

const DAY_MS: f64 = 86_400_000.0;
/// The first day of every history: 2026-01-01.
const DAY_0: f64 = 1_767_225_600_000.0;
/// How many cards there are to own.
const CARDS: usize = 80;
/// How many of them a player plays with.
const DECK: usize = 14;

/// One device: what its browser has stored, and every take-over known to
/// it, its own and those that came with a state it took. (The engine's
/// `merges` are kept the same way, but leave out a take-over that brought
/// nothing new. The formulas are asked of every one.)
#[derive(Clone)]
struct Device {
    state: String,
    ledger: BTreeMap<u64, Met>,
}

/// A device whose browser has never opened the site.
fn fresh() -> Device {
    doc::init_player_doc();
    Device { state: doc::encode_full_state(), ledger: BTreeMap::new() }
}

/// What makes a history up as it goes: the dice, the days, and a number for
/// each take-over.
struct World {
    dice: fastrand::Rng,
    day: u32,
    takeovers: u64,
}

impl World {
    fn new(seed: u64) -> World {
        World { dice: fastrand::Rng::with_seed(seed), day: 0, takeovers: 0 }
    }

    fn now(&self) -> f64 {
        DAY_0 + self.day as f64 * DAY_MS
    }

    fn chance(&mut self, of: f64) -> bool {
        self.dice.f64() < of
    }

    /// A player, by the cards they play with.
    fn player(&mut self) -> Vec<usize> {
        let mut cards: Vec<usize> = (0..CARDS).collect();
        self.dice.shuffle(&mut cards);
        cards.truncate(DECK);
        cards
    }

    /// A card this player picks up: one of their own, mostly.
    fn card(&mut self, deck: &[usize]) -> String {
        let card = if self.chance(0.85) { deck[self.dice.usize(..deck.len())] } else { self.dice.usize(..CARDS) };
        format!("card_{:02}", card)
    }

    /// An evening's play on a device, over a few pages. Each page is a new
    /// document, as each page load is in a browser.
    fn play(&mut self, deck: &[usize], device: &mut Device) {
        let (now, today) = (self.now(), clock::date_of(self.now()));
        for page in 0..self.dice.usize(2..=5) {
            doc::restore_from_state(&device.state).unwrap();
            if page == 0 && self.chance(0.3) {
                // New Game.
                doc::clear_all();
                doc::clear_affinity();
                doc::remove_all_alarms(now);
            }
            if page == 0 && self.chance(0.25) {
                let archetypes = doc::valid_archetypes();
                let _ = doc::declare_affinity(archetypes[self.dice.usize(..archetypes.len())], &today);
            }
            for _ in 0..self.dice.usize(1..=3) {
                let card = self.card(deck);
                doc::ensure_card_state(&card, 3);
                for _ in 0..self.dice.usize(0..=2) {
                    doc::toggle_slot(&card, self.dice.u8(1..=3));
                }
                // A round of fists in a room: the card is played today.
                if self.chance(0.2) {
                    let _ = doc::increment_loyalty(&card, &today);
                }
            }
            if self.chance(0.3) {
                doc::add_alarm(now, self.dice.i32(2..=6), "timer", "red");
            }
            if self.chance(0.5) {
                doc::tick_alarms(now);
            }
            device.state = doc::encode_full_state();
        }
    }

    /// Evenings of play, one a day.
    fn evenings(&mut self, deck: &[usize], device: &mut Device, count: usize) {
        for _ in 0..count {
            self.play(deck, device);
            self.day += 1;
        }
    }

    /// A device not being played on is used to look a card or two up.
    fn look_up(&mut self, device: &mut Device) {
        doc::restore_from_state(&device.state).unwrap();
        for _ in 0..self.dice.usize(1..=4) {
            let card = format!("card_{:02}", self.dice.usize(..CARDS));
            doc::ensure_card_state(&card, 3);
            if self.chance(0.15) {
                doc::toggle_slot(&card, 1);
            }
        }
        device.state = doc::encode_full_state();
    }

    /// The receiver takes a state over, and every formula is told of it.
    fn take(&mut self, state: &str, ledger: &BTreeMap<u64, Met>, receiver: &mut Device, by: &str) -> doc::Taken {
        doc::restore_from_state(&receiver.state).unwrap();
        let mine = state_vector(&doc::encode_state_vector());
        let taken = doc::take_over(state, self.now(), by).unwrap();
        let met = met(&mine, &state_vector(&taken.their_state_vector), &taken);
        receiver.state = doc::encode_full_state();
        receiver.ledger.extend(ledger.iter().map(|(id, met)| (*id, *met)));
        receiver.ledger.insert(self.takeovers, met);
        self.takeovers += 1;
        taken
    }

    /// A game is moved from one device to another: the receiver takes it,
    /// and the sender is sent what the receiver added to the record.
    fn sync(&mut self, sender: &mut Device, receiver: &mut Device) {
        let taken = self.take(&sender.state.clone(), &sender.ledger.clone(), receiver, "device");
        let back = doc::encode_diff(&taken.their_state_vector).unwrap();
        doc::restore_from_state(&sender.state).unwrap();
        doc::apply_update(&back).unwrap();
        sender.state = doc::encode_full_state();
        sender.ledger = receiver.ledger.clone();
    }

    /// A backup is imported.
    fn import(&mut self, backup: &Device, receiver: &mut Device) {
        self.take(&backup.state, &backup.ledger, receiver, "file");
    }
}

// ── Ways of using the site ─────────────────────────────────────────

/// One player gets a new phone, once.
fn a_new_phone(world: &mut World) -> Device {
    let me = world.player();
    let (mut old, mut new) = (fresh(), fresh());
    world.evenings(&me, &mut old, 80);
    world.sync(&mut old, &mut new);
    world.evenings(&me, &mut new, 80);
    new
}

/// One player, `count` devices, one played on at a time: the game is moved
/// on at every change. `looks_up` has the devices not in use look cards up
/// meanwhile.
fn devices_in_turn(world: &mut World, count: usize, looks_up: bool) -> Device {
    let me = world.player();
    let mut devices: Vec<Device> = (0..count).map(|_| fresh()).collect();
    let mut active = 0;
    world.evenings(&me, &mut devices[0], 20);
    for _ in 0..20 {
        let next = (active + 1) % count;
        if looks_up {
            for idle in (0..count).filter(|idle| *idle != active) {
                for _ in 0..world.dice.usize(1..=3) {
                    world.look_up(&mut devices[idle]);
                }
            }
        }
        let (mut from, mut to) = (devices[active].clone(), devices[next].clone());
        world.sync(&mut from, &mut to);
        (devices[active], devices[next]) = (from, to);
        active = next;
        let evenings = world.dice.usize(3..=7);
        world.evenings(&me, &mut devices[active], evenings);
    }
    devices[active].clone()
}

fn two_devices_in_turn(world: &mut World) -> Device {
    devices_in_turn(world, 2, false)
}

fn two_devices_and_looking_up(world: &mut World) -> Device {
    devices_in_turn(world, 2, true)
}

fn three_devices_in_turn(world: &mut World) -> Device {
    devices_in_turn(world, 3, true)
}

/// One player, careless with two devices: plays on either, sometimes both
/// in a day, syncs now and then, and one time in five the wrong way.
fn two_devices_carelessly(world: &mut World) -> Device {
    let me = world.player();
    let mut devices = [fresh(), fresh()];
    let (mut last, mut next_sync) = (0, 10);
    for day in 0..140 {
        let on = if world.chance(0.7) { 0 } else { 1 };
        world.play(&me, &mut devices[on]);
        last = on;
        if world.chance(0.15) {
            world.play(&me, &mut devices[1 - on]);
        }
        world.day += 1;
        if day == next_sync {
            let from = if world.chance(0.8) { last } else { 1 - last };
            let (mut sender, mut receiver) = (devices[from].clone(), devices[1 - from].clone());
            world.sync(&mut sender, &mut receiver);
            (devices[from], devices[1 - from]) = (sender, receiver);
            last = 1 - from;
            next_sync = day + world.dice.usize(7..=13);
        }
    }
    devices[last].clone()
}

/// One player, one device, a backup now and then; the browser's data is
/// cleared three times and the last backup imported, sometimes after an
/// evening or two on the emptied device.
fn backups_restored(world: &mut World) -> Device {
    let me = world.player();
    let mut device = fresh();
    let mut backup = device.clone();
    for evening in 0..160 {
        world.evenings(&me, &mut device, 1);
        if evening % 15 == 14 {
            backup = device.clone();
        }
        if [50, 100, 150].contains(&evening) {
            device = fresh();
            let before = world.dice.usize(0..=2);
            world.evenings(&me, &mut device, before);
            world.import(&backup, &mut device);
        }
    }
    device
}

/// Two players, each with a season behind them, put their records together.
fn two_records_pooled(world: &mut World) -> Device {
    let (me, them) = (world.player(), world.player());
    let (mut mine, mut theirs) = (fresh(), fresh());
    world.evenings(&me, &mut mine, 120);
    world.day = 0;
    world.evenings(&them, &mut theirs, 120);
    world.sync(&mut theirs, &mut mine);
    world.evenings(&me, &mut mine, 20);
    mine
}

/// A player of long standing takes in the record of one who has just begun.
fn a_newcomers_record_taken_in(world: &mut World) -> Device {
    let (me, them) = (world.player(), world.player());
    let (mut mine, mut theirs) = (fresh(), fresh());
    world.evenings(&me, &mut mine, 150);
    world.day = 135;
    world.evenings(&them, &mut theirs, 15);
    world.sync(&mut theirs, &mut mine);
    world.evenings(&me, &mut mine, 20);
    mine
}

/// One player takes in the records of six others, one after another.
fn a_collector(world: &mut World) -> Device {
    let me = world.player();
    let mut mine = fresh();
    world.evenings(&me, &mut mine, 120);
    for _ in 0..6 {
        let (them, mut theirs) = (world.player(), fresh());
        let (today, evenings) = (world.day, world.dice.usize(30..=60));
        world.day = today.saturating_sub(evenings as u32);
        world.evenings(&them, &mut theirs, evenings);
        world.day = today;
        world.sync(&mut theirs, &mut mine);
        world.evenings(&me, &mut mine, 5);
    }
    mine
}

/// Two players keep one account: each plays on their own device every
/// week, and they sync weekly, one way and then the other.
fn two_players_one_account(world: &mut World) -> Device {
    let (me, them) = (world.player(), world.player());
    let (mut mine, mut theirs) = (fresh(), fresh());
    world.evenings(&me, &mut mine, 20);
    world.sync(&mut mine, &mut theirs);
    for week in 0..20 {
        let monday = world.day;
        world.evenings(&me, &mut mine, 5);
        world.day = monday;
        world.evenings(&them, &mut theirs, 5);
        if week % 2 == 0 {
            world.sync(&mut mine, &mut theirs);
        } else {
            world.sync(&mut theirs, &mut mine);
        }
    }
    mine
}

/// One account played in five browsers at once, each with its own cards,
/// the four others sent to the first every week.
fn a_farm_of_browsers(world: &mut World) -> Device {
    let decks: Vec<Vec<usize>> = (0..5).map(|_| world.player()).collect();
    let mut main = fresh();
    world.evenings(&decks[0], &mut main, 10);
    let mut farm: Vec<Device> = (0..4).map(|_| fresh()).collect();
    for browser in farm.iter_mut() {
        world.sync(&mut main, browser);
    }
    for _ in 0..6 {
        let monday = world.day;
        world.evenings(&decks[0], &mut main, 5);
        for (browser, deck) in farm.iter_mut().zip(&decks[1..]) {
            world.day = monday;
            world.evenings(deck, browser, 5);
        }
        for browser in farm.iter_mut() {
            world.sync(browser, &mut main);
        }
    }
    main
}

/// Two records pooled, the one sent having first taken the empty game of
/// a browser that had never opened the site: its history is let go and its
/// record written anew, in far fewer changes.
fn a_record_washed_then_pooled(world: &mut World) -> Device {
    let (me, them) = (world.player(), world.player());
    let (mut mine, mut theirs) = (fresh(), fresh());
    world.evenings(&me, &mut mine, 120);
    world.day = 0;
    world.evenings(&them, &mut theirs, 120);
    world.sync(&mut fresh(), &mut theirs);
    world.sync(&mut theirs, &mut mine);
    world.evenings(&me, &mut mine, 20);
    mine
}

type Persona = (&'static str, bool, fn(&mut World) -> Device);

/// Each way of using the site, and whether it is an odd one.
const PERSONAS: [Persona; 12] = [
    ("a new phone, once", false, a_new_phone),
    ("two devices, in turn", false, two_devices_in_turn),
    ("two devices, the idle one looks cards up", false, two_devices_and_looking_up),
    ("three devices, in turn", false, three_devices_in_turn),
    ("two devices, carelessly", false, two_devices_carelessly),
    ("backups restored after clearing the browser", false, backups_restored),
    ("two records pooled", true, two_records_pooled),
    ("a newcomer's record taken in", true, a_newcomers_record_taken_in),
    ("a collector of six records", true, a_collector),
    ("two players, one account", true, two_players_one_account),
    ("a farm of five browsers", true, a_farm_of_browsers),
    ("a record washed, then pooled", true, a_record_washed_then_pooled),
];

// ── What comes of them ─────────────────────────────────────────────

/// How many histories of each persona are made up.
const HISTORIES: u64 = 24;

/// The middle of some numbers, and the least and the most.
fn spread(mut numbers: Vec<f64>) -> (f64, f64, f64) {
    numbers.sort_by(f64::total_cmp);
    (numbers[numbers.len() / 2], numbers[0], numbers[numbers.len() - 1])
}

fn shown(number: f64) -> String {
    if number >= 100.0 {
        format!("{:.0}", number)
    } else if number >= 10.0 {
        format!("{:.1}", number)
    } else {
        format!("{:.2}", number)
    }
}

#[test]
#[ignore = "a simulation, run when asked for: see the top of the file"]
fn made_up_histories_through_each_formula() {
    // persona → formula → what it added up to, history by history
    let mut sums: Vec<Vec<Vec<f64>>> = Vec::new();
    println!();
    println!("{} histories of each. The middle one, and [the least to the most].", HISTORIES);
    for (name, odd, persona) in PERSONAS {
        let mut by_formula: Vec<Vec<f64>> = vec![Vec::new(); FORMULAS.len()];
        let (mut takeovers, mut sizes, mut days) = (Vec::new(), Vec::new(), Vec::new());
        for seed in 0..HISTORIES {
            let device = persona(&mut World::new(seed));
            doc::restore_from_state(&device.state).unwrap();
            // The ledger is the engine's merges, with the take-overs that
            // brought nothing new besides: what the engine keeps agrees.
            let built: f64 = device.ledger.values().map(Met::built).sum();
            assert_eq!(built as u64, doc::sync_sum().distance, "{}", name);
            for (sums, (_, formula)) in by_formula.iter_mut().zip(FORMULAS) {
                sums.push(device.ledger.values().map(formula).sum());
            }
            takeovers.push(device.ledger.len() as f64);
            sizes.push(total(&state_vector(&doc::encode_state_vector())));
            days.push(doc::days_played() as f64);
        }
        println!();
        println!("{} {}", if odd { "ODD   " } else { "NORMAL" }, name);
        println!(
            "    take-overs {:.0}, changes held {:.0}, days in the record {:.0}",
            spread(takeovers).0,
            spread(sizes).0,
            spread(days).0
        );
        for (sums, (formula, _)) in by_formula.iter().zip(FORMULAS) {
            let (middle, least, most) = spread(sums.clone());
            println!("    {:12} {:>8}  [{} to {}]", formula, shown(middle), shown(least), shown(most));
        }
        sums.push(by_formula);
    }

    println!();
    println!("At a glance: the most any normal history came to, and the least any odd one did.");
    for (index, (formula, _)) in FORMULAS.iter().enumerate() {
        let of = |odd: bool| -> Vec<f64> {
            PERSONAS
                .iter()
                .zip(&sums)
                .filter(|((_, is_odd, _), _)| *is_odd == odd)
                .flat_map(|(_, by_formula)| by_formula[index].clone())
                .collect()
        };
        let (normal, strange) = (of(false), of(true));
        let most_normal = spread(normal).2;
        let least_odd = spread(strange.clone()).1;
        let missed = strange.iter().filter(|sum| **sum <= most_normal).count();
        println!(
            "    {:12} normal up to {:>8}, odd from {:>8}: {} of {} odd histories are no higher than a normal one",
            formula,
            shown(most_normal),
            shown(least_odd),
            missed,
            strange.len()
        );
    }
    doc::init_player_doc();
}
