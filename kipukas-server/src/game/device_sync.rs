//! Moving a game from one of the player's devices to another.
//!
//! One device (the sender) has the game: damage, timers, the affinity
//! declared. The other (the receiver) is to carry it on. The game goes one
//! way and takes the place of whatever game the receiver had. The player's
//! record (each card's loyalty) is the other kind of thing: each device adds
//! what it knows to what the other holds, and both end with the same.
//! `player_doc::take_over` is where that is done; this module gets the
//! sender's state to it and the receiver's additions back.
//!
//! ## How the two meet
//!
//! The sender draws a secret and makes a room on the relay the game's rooms
//! use. It then shows an invitation, the room's code and the secret, as a QR
//! code and as text. The receiver is handed it one of three ways: the
//! address in the QR code opens this site's Player Data page with the
//! invitation after the `#` (`routes::device_sync`, which reads it from the
//! page's address); the site's own scanner reads the code (`routes::qr`); or
//! the player types it. Nothing moves until the player on the receiver has
//! said so: an address opened by mistake, or sent by someone else, changes
//! nothing by being opened.
//!
//! The secret never goes to the relay. Everything the two say to each other
//! is sealed with a key made from it (`crypto::Pairing`), and the room's
//! name, which the relay asks of whoever joins, is made from it too. So the
//! relay reads nothing of the player's, and a device that was not shown the
//! invitation can neither join nor be understood.
//!
//! ## What travels
//!
//! To the relay, as for a game room: `action` (`create` or `join`), `code`,
//! `name` (the room name made from the secret), and `player`, an id made
//! for this page load.
//!
//! Between the devices, as the relay's `data`: `{"type":"sealed","box":…}`,
//! where the box opens to one of
//!
//! ```text
//! receiver → sender   {"say":"want"}                  the player here said yes
//! sender → receiver   {"say":"game","state":…}        the sender's whole state
//! receiver → sender   {"say":"record","update":…}     what the receiver added to it
//! sender → receiver   {"say":"done"}                  taken in; both hold the same
//! either              {"say":"failed"}                could not be read
//! ```
//!
//! ## A place in a room goes with the game
//!
//! A game being played against someone is in a room, and the player's
//! place there is part of it. So a sender that is in a room sends its place
//! with the game (`"seat"`, a `connection::Seat`), and the receiver takes it
//! up: it returns to the room as the player, which the relay and the
//! opponent see as the player coming back, as after any page load. The
//! receiver says whether it did (`"seat": true` with the record), and only
//! then does the sender forget the room. It does not leave it: the player
//! is still there.
//!
//! The room the receiver was in, if any, was part of the game it had. It
//! leaves that room when it takes the game.
//!
//! One thing can go wrong between the two messages: the receiver takes the
//! game and the place, and the sender never hears. The relay then closes
//! the sender's connection to the room, since the player has come back
//! elsewhere, and the sender takes that as the word it missed
//! ([`seat_taken_up`]).
//!
//! Then each gives up its place in the room, which ends it: an invitation
//! works once. One nobody has used ends after [`INVITE_LASTS_MS`]. The
//! socket is a [`Link`] of its own, which does not retry: a move that drops
//! is started again by hand.

use std::cell::RefCell;

use crate::clock;
use crate::game::crypto::{self, Pairing, Side, PAIRING_ALPHABET, PAIRING_SECRET_LENGTH};
use crate::game::{connection, player_doc};
use crate::link::{Link, Owner, State, Timer, Told};
use crate::page;
use crate::wire::{self, Channel, SocketEvent};

/// How long an invitation waits for the other device.
pub const INVITE_LASTS_MS: u32 = 10 * 60 * 1000;

/// The wait that ends an invitation nobody has used.
const EXPIRE: &str = "expire";

/// For this long after the sender sent its place in a room with its game,
/// the player coming back to that room from elsewhere is taken to be the
/// receiver taking the place up. (It does so at once, if it does.)
const SEAT_OUT_MS: f64 = 30_000.0;

/// Where the site is, for the address an invitation's QR code carries.
const SITE: &str = "https://www.kipukas.cards";

// ── An invitation ──────────────────────────────────────────────────

/// What the sender shows the receiver: the room's code and the secret.
#[derive(Debug, Clone, PartialEq)]
pub struct Invite {
    pub code: String,
    pub secret: String,
}

impl Invite {
    /// Read an invitation from what a device was handed: what follows the
    /// `#` of an address (`sync=ABCD-…`), the whole address, or the code as
    /// it was typed, with or without its dashes and in either case.
    pub fn read(handed: &str) -> Option<Invite> {
        let text = handed.rsplit("sync=").next().unwrap_or(handed).trim();
        let written: String = text.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ' ').collect();
        let plain: String =
            written.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
        if plain.len() != 4 + PAIRING_SECRET_LENGTH {
            return None;
        }
        let (code, secret) = plain.split_at(4);
        secret.bytes().all(|c| PAIRING_ALPHABET.contains(&c)).then(|| Invite { code: code.into(), secret: secret.into() })
    }

    /// As an address carries it: `ABCD-EFGHJKLMNPQRSTUVWXYZ`.
    pub fn compact(&self) -> String {
        format!("{}-{}", self.code, self.secret)
    }

    /// As it is shown to be typed, in groups: `ABCD-EFGHJ-KLMNP-QRSTU-VWXYZ`.
    pub fn spelled(&self) -> String {
        let groups: Vec<&str> =
            self.secret.as_bytes().chunks(5).map(|group| std::str::from_utf8(group).unwrap_or("")).collect();
        format!("{}-{}", self.code, groups.join("-"))
    }

    /// The address the QR code carries: this site's Player Data page, with
    /// the invitation after the `#`, which a browser sends to no server.
    pub fn address(&self) -> String {
        format!("{}/player_data#sync={}", SITE, self.compact())
    }
}

// ── What is held ───────────────────────────────────────────────────

/// How far a move has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    /// Nothing asked. (An invitation may be waiting for the player's answer.)
    #[default]
    Idle,
    /// The sender is asked whether to go on.
    Asked,
    /// The sender is making its room, then showing the invitation.
    Inviting,
    /// The receiver is on its way to the sender's room.
    Joining,
    /// The two are in the room and the game is on its way.
    Moving,
    /// The game has moved.
    Done,
}

/// What the page shows of a move.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dialog {
    pub stage: Stage,
    /// Which of the two this device is, once it is either.
    pub side: Option<Side>,
    /// The invitation the sender shows, once the relay has given the room
    /// its code.
    pub invite: Option<Invite>,
    /// An invitation this device was handed and the player has yet to answer.
    pub offered: Option<Invite>,
    /// A code that was typed and could not be read, to stay in its field.
    pub typed: String,
    pub error: String,
    /// The sender: the other device is in the room.
    pub peer_here: bool,
    /// The receiver: what came of taking the game over.
    pub taken: Option<player_doc::Taken>,
    /// The sender: how many days of play the other device added to the record.
    pub days_brought: u32,
    /// The receiver: the sender has said it took in what was sent back.
    pub confirmed: bool,
    /// The room whose place went with the game: the receiver has taken it
    /// up, the sender has let it go.
    pub room: Option<String>,
    /// The sender: the other device has the game, as its taking up the
    /// place in the room showed, but was not heard from again. What it
    /// added to the record has not arrived.
    pub unheard: bool,
}

/// What this device asked the relay for.
#[derive(Debug, Clone, PartialEq)]
enum Want {
    Create,
    Join { code: String },
}

#[derive(Default)]
struct Sync {
    dialog: Dialog,
    /// Set until the relay answers; while it is, a lost socket is a failure
    /// to connect.
    want: Option<Want>,
    /// This device's id in the room: one per page load.
    player: String,
    /// The sender's secret, until the room has its code and the invitation
    /// is whole.
    secret: String,
    pairing: Option<Pairing>,
    /// Invitations the player has answered on this page. The page's address
    /// goes on carrying the one it was opened with, which is not offered
    /// again.
    answered: Vec<String>,
    /// The sender: the room whose place it sent with the game, and when,
    /// until the receiver has said what it did with it.
    seat_out: Option<(String, f64)>,
    /// The sender: the relay has shown the receiver taking that place up.
    seat_went: bool,
    /// The dialog as it was when the page was last asked to draw it.
    drawn: Option<Dialog>,
}

thread_local! {
    static SYNC: RefCell<Sync> = RefCell::new(Sync::default());
    static LINK: RefCell<Link> = RefCell::new(new_link());
}

fn new_link() -> Link {
    Link::new(Owner {
        url: Box::new(relay_address),
        should_retry: Box::new(|| false),
        random: Box::new(fastrand::f64),
    })
}

fn with_sync<R>(f: impl FnOnce(&mut Sync) -> R) -> R {
    SYNC.with(|sync| f(&mut sync.borrow_mut()))
}

fn stage() -> Stage {
    with_sync(|sync| sync.dialog.stage)
}

fn side() -> Option<Side> {
    with_sync(|sync| sync.dialog.side)
}

// ── The link ───────────────────────────────────────────────────────

/// Do something to the link, then pass on what it asks for and act on what
/// it has to tell. Nothing held here may be borrowed across this: the link
/// asks for the relay address.
fn link<R>(f: impl FnOnce(&mut Link) -> R) -> R {
    let result = LINK.with(|link| f(&mut link.borrow_mut()));
    settle();
    result
}

fn settle() {
    loop {
        let (asks, told) = LINK.with(|link| {
            let mut link = link.borrow_mut();
            (link.take_asks(), link.take_told())
        });
        if asks.is_empty() && told.is_empty() {
            return;
        }
        for ask in asks {
            wire::pass(Channel::Sync, ask);
        }
        for told in told {
            match told {
                Told::State { next, prev } => link_state_changed(next, prev),
                Told::Opened => {}
                Told::Message(message) => relay_said(&message),
            }
        }
    }
}

fn relay_address() -> String {
    let (want, player, name) = SYNC.with(|sync| {
        let sync = sync.borrow();
        let name = sync.pairing.as_ref().map(|pairing| pairing.room_name.clone()).unwrap_or_default();
        (sync.want.clone(), sync.player.clone(), name)
    });
    let mut wants: Vec<(&str, String)> = Vec::new();
    match want {
        Some(Want::Create) => wants.push(("action", "create".to_string())),
        Some(Want::Join { code }) => {
            wants.push(("action", "join".to_string()));
            wants.push(("code", code));
        }
        None => {}
    }
    wants.push(("name", name));
    wants.push(("player", player));
    connection::relay_address_with(&wants)
}

/// Seal `message` and send it to the other device.
fn say(message: serde_json::Value) {
    let sealed = SYNC.with(|sync| {
        let sync = sync.borrow();
        let (pairing, side) = (sync.pairing.as_ref()?, sync.dialog.side?);
        pairing.seal(side, &message.to_string(), crypto::random_bytes().ok()?).ok()
    });
    if let Some(sealed) = sealed {
        let data = serde_json::json!({ "type": "relay", "data": { "type": "sealed", "box": sealed } });
        link(|link| link.send(&data.to_string()));
    }
}

// ── What the worker reports ────────────────────────────────────────

/// The link's socket did something.
pub fn socket_event(socket: u32, event: SocketEvent) {
    link(|link| match event {
        SocketEvent::Open => link.socket_opened(socket),
        SocketEvent::Message(text) => link.socket_message(socket, &text),
        SocketEvent::Error => link.socket_error(socket),
        SocketEvent::Close => link.socket_closed(socket),
    });
    show();
}

/// One of the link's waits is over.
pub fn link_timer(timer: Timer) {
    link(|link| link.timer_fired(timer));
    show();
}

/// A wait of this module's own is over.
pub fn wait_over(wait: &str) {
    if wait == EXPIRE && stage() == Stage::Inviting {
        fail("That code was not used within ten minutes and has ended. Start again to make another.");
    }
    show();
}

// ── What the player asks for ───────────────────────────────────────

/// The sender: the player wants to move this device's game to another.
/// They are asked first.
pub fn ask() {
    with_sync(|sync| {
        if sync.dialog.stage == Stage::Idle {
            sync.dialog = Dialog { stage: Stage::Asked, ..Dialog::default() };
        }
    });
    show();
}

/// The sender: the player said yes. Make the room and the invitation.
pub fn invite() {
    if stage() != Stage::Asked {
        return;
    }
    match crypto::new_pairing_secret() {
        Ok(secret) => {
            with_sync(|sync| {
                sync.pairing = Some(Pairing::new(&secret));
                sync.secret = secret;
                sync.dialog = Dialog { stage: Stage::Inviting, side: Some(Side::Sender), ..Dialog::default() };
                (sync.seat_out, sync.seat_went) = (None, false);
            });
            connect_for(Want::Create);
            wire::set(Channel::Sync, EXPIRE, INVITE_LASTS_MS);
        }
        Err(problem) => fail(&format!("A code could not be made: {}", problem)),
    }
    show();
}

/// The receiver was handed an invitation: typed, or read by the scanner.
/// The player is asked before anything is done with it.
pub fn offer(handed: &str) {
    with_sync(|sync| {
        if sync.dialog.stage != Stage::Idle {
            return;
        }
        match Invite::read(handed) {
            Some(invite) => sync.dialog = Dialog { offered: Some(invite), ..Dialog::default() },
            None => {
                sync.dialog = Dialog {
                    typed: handed.to_string(),
                    error: "That is not a sync code. It is four characters, then twenty more.".to_string(),
                    ..Dialog::default()
                }
            }
        }
    });
    show();
}

/// The receiver: the player said yes to an invitation. Go to the sender's
/// room.
pub fn receive(handed: &str) {
    if stage() != Stage::Idle {
        return;
    }
    let Some(invite) = Invite::read(handed) else {
        return offer(handed);
    };
    with_sync(|sync| {
        sync.answered.push(invite.compact());
        sync.pairing = Some(Pairing::new(&invite.secret));
        sync.dialog = Dialog { stage: Stage::Joining, side: Some(Side::Receiver), ..Dialog::default() };
        (sync.seat_out, sync.seat_went) = (None, false);
    });
    connect_for(Want::Join { code: invite.code });
    show();
}

/// The receiver: the player said no to an invitation.
pub fn decline(handed: &str) {
    with_sync(|sync| {
        if let Some(invite) = Invite::read(handed) {
            sync.answered.push(invite.compact());
        }
        if sync.dialog.stage == Stage::Idle {
            sync.dialog = Dialog::default();
        }
    });
    show();
}

/// Call a move off, or put away what came of one: back to how the page
/// opens.
pub fn close() {
    leave();
    with_sync(|sync| {
        sync.dialog = Dialog::default();
        (sync.seat_out, sync.seat_went) = (None, false);
    });
    show();
}

fn connect_for(want: Want) {
    link(|link| link.stop());
    with_sync(|sync| {
        if sync.player.is_empty() {
            sync.player = format!("{:016x}{:08x}", fastrand::u64(..), fastrand::u32(..));
        }
        sync.want = Some(want);
    });
    link(|link| link.start());
}

/// Give up this device's place in the room, which ends the room once both
/// have, and let go of the socket.
fn leave() {
    wire::clear(Channel::Sync, EXPIRE);
    with_sync(|sync| {
        sync.want = None;
        sync.pairing = None;
        sync.secret.clear();
    });
    link(|link| {
        link.send(r#"{"type":"leave"}"#);
        link.stop();
    });
}

/// The receiver has taken the game over.
fn has_the_game() -> bool {
    with_sync(|sync| sync.dialog.taken.is_some())
}

/// The other device, or the way to it, is lost while a move is on. Before
/// the game has arrived the move is off, and `error` says so. After, the
/// receiver has the game: the move is done, though the sender never said it
/// took in what was sent back, or the receiver what it added.
fn lost(error: &str) {
    if has_the_game() {
        with_sync(|sync| sync.dialog.stage = Stage::Done);
        leave();
    } else if with_sync(|sync| sync.seat_went) {
        moved_unheard();
    } else {
        fail(error);
    }
}

/// The sender: the other device has the game, as its taking up the place
/// in the room showed, though it was not heard from again.
fn moved_unheard() {
    leave();
    with_sync(|sync| {
        let room = sync.seat_out.take().map(|(code, _)| code);
        sync.seat_went = false;
        sync.dialog =
            Dialog { stage: Stage::Done, side: Some(Side::Sender), room, unheard: true, ..Dialog::default() };
    });
}

/// The relay says this player has come back to their room from elsewhere.
/// Whether that is the other device taking up the place this one sent with
/// its game: it is, if one was sent a moment ago. The other device then has
/// the game, since it takes the place up only once it does.
pub fn seat_taken_up() -> bool {
    let taken_up = with_sync(|sync| {
        let out = sync.seat_out.as_ref().is_some_and(|(_, sent)| clock::now_ms() - sent <= SEAT_OUT_MS);
        sync.seat_went |= out;
        out
    });
    // A move given up as lost was not: the game arrived.
    if taken_up && !matches!(stage(), Stage::Moving | Stage::Done) {
        moved_unheard();
    }
    if taken_up {
        show();
    }
    taken_up
}

/// Back to how the page opens, saying what went wrong.
fn fail(error: &str) {
    leave();
    with_sync(|sync| sync.dialog = Dialog { error: error.to_string(), ..Dialog::default() });
}

// ── What the link tells ────────────────────────────────────────────

fn link_state_changed(next: State, prev: State) {
    if next != State::Idle {
        return;
    }
    // The relay was never reached, or never answered.
    if with_sync(|sync| sync.want.take()).is_some() {
        fail("The relay could not be reached. Check the connection and try again.");
        return;
    }
    // The socket of a move closed under it.
    if prev == State::Open && matches!(stage(), Stage::Inviting | Stage::Joining | Stage::Moving) {
        lost("The connection was lost before the game had moved. Nothing was changed. Start again.");
    }
}

fn relay_said(message: &serde_json::Value) {
    let text = |name: &str| message[name].as_str().unwrap_or("");
    match text("type") {
        "room_created" => with_sync(|sync| {
            sync.want = None;
            sync.dialog.invite = Some(Invite { code: text("code").to_string(), secret: std::mem::take(&mut sync.secret) });
        }),
        "room_joined" => {
            with_sync(|sync| sync.want = None);
            // The room is still on the relay, but the device that made it
            // is no longer in it.
            if text("peer") != "present" {
                fail("The other device is no longer showing that code. Start again there.");
            }
        }
        // Both devices are in the room. The receiver, whose player has said
        // yes, asks for the game.
        "peer_joined" => match side() {
            Some(Side::Receiver) if stage() == Stage::Joining => {
                with_sync(|sync| sync.dialog.stage = Stage::Moving);
                say(serde_json::json!({ "say": "want" }));
            }
            Some(Side::Sender) => with_sync(|sync| sync.dialog.peer_here = true),
            _ => {}
        },
        "relay" => peer_said(&message["data"]),
        "peer_away" | "peer_left" => match (side(), stage()) {
            // It may come back: the invitation stands until it ends.
            (Some(Side::Sender), Stage::Inviting) => with_sync(|sync| sync.dialog.peer_here = false),
            (_, Stage::Joining | Stage::Moving) => {
                lost("The other device went away before the game had moved. Start again.");
            }
            _ => {}
        },
        "error" => fail(match text("code") {
            "not_found" => "That code has ended or has been used. Start again on the other device.",
            "name_mismatch" => "That is not the code the other device is showing. Check it and try again.",
            "full" => "That code is in use by another device. Start again on the other device.",
            _ if !text("message").is_empty() => text("message"),
            _ => "The relay refused the connection.",
        }),
        _ => {}
    }
}

/// A message from the other device.
fn peer_said(data: &serde_json::Value) {
    if data["type"] != "sealed" {
        return;
    }
    let opened = SYNC.with(|sync| {
        let sync = sync.borrow();
        let (pairing, side) = (sync.pairing.as_ref()?, sync.dialog.side?);
        pairing.open(side.other(), data["box"].as_str()?)
    });
    // What does not open was not sealed by a device that was shown the
    // invitation, or was altered on its way.
    let Some(said) = opened.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()) else {
        if matches!(stage(), Stage::Inviting | Stage::Joining | Stage::Moving) {
            lost("What the other device sent could not be read. Nothing was changed. Start again.");
        }
        return;
    };
    let text = |name: &str| said[name].as_str().unwrap_or("");

    match (side(), stage(), text("say")) {
        // The player on the other device said yes: send the game.
        (Some(Side::Sender), Stage::Inviting, "want") => {
            wire::clear(Channel::Sync, EXPIRE);
            // This device's place in a room, if it holds one, goes with it.
            let seat = connection::seat();
            with_sync(|sync| {
                sync.dialog.stage = Stage::Moving;
                sync.seat_out = seat.as_ref().map(|seat| (seat.code.clone(), clock::now_ms()));
                sync.seat_went = false;
            });
            let mut game = serde_json::json!({ "say": "game", "state": player_doc::encode_full_state() });
            if let Some(seat) = seat {
                game["seat"] = serde_json::to_value(seat).unwrap_or_default();
            }
            say(game);
        }
        // The game. It takes the place of the one here, this device's record
        // is added to the one it brings, and what was added goes back.
        (Some(Side::Receiver), Stage::Moving, "game") if !has_the_game() => {
            match player_doc::take_over(text("state"), clock::now_ms(), "device") {
                Ok(taken) => {
                    // What goes back is what this device added to the
                    // record: worked out before the room is touched, which
                    // can mark damage from a round that was waiting.
                    let update = player_doc::encode_diff(&taken.their_state_vector).unwrap_or_default();
                    // The room this device was in went with the game it
                    // had. The sender's place in one comes with the game.
                    let seat: Option<connection::Seat> = serde_json::from_value(said["seat"].clone()).ok();
                    let sat = connection::game_taken(seat.as_ref());
                    say(serde_json::json!({ "say": "record", "update": update, "seat": sat }));
                    // The move is done here. It is shown as done once the
                    // sender has said it took that in, or is lost.
                    with_sync(|sync| {
                        sync.dialog.taken = Some(taken);
                        sync.dialog.room = seat.filter(|_| sat).map(|seat| seat.code.to_uppercase());
                    });
                    // Everything on the page that shows the player's state
                    // shows the game taken over.
                    page::refresh();
                }
                Err(_) => {
                    say(serde_json::json!({ "say": "failed" }));
                    fail("The game the other device sent could not be read. Nothing here was changed.");
                }
            }
        }
        // What the other device added to the record.
        (Some(Side::Sender), Stage::Moving, "record") => {
            // The place sent with the game is the other device's once it
            // says it has taken it up. The room is forgotten here.
            let room = with_sync(|sync| {
                sync.seat_went = false;
                sync.seat_out.take().map(|(code, _)| code)
            })
            .filter(|_| said["seat"].as_bool() == Some(true));
            if room.is_some() {
                connection::seat_handed_over();
            }
            let before = player_doc::days_played();
            if player_doc::apply_update(text("update")).is_err() {
                say(serde_json::json!({ "say": "failed" }));
                fail("The other device has the game, but what it sent back could not be read.");
                return;
            }
            say(serde_json::json!({ "say": "done" }));
            with_sync(|sync| {
                sync.dialog.stage = Stage::Done;
                sync.dialog.days_brought = player_doc::days_played().saturating_sub(before);
                sync.dialog.room = room;
            });
            leave();
            page::refresh();
        }
        (Some(Side::Receiver), Stage::Moving, "done") if has_the_game() => {
            with_sync(|sync| {
                sync.dialog.stage = Stage::Done;
                sync.dialog.confirmed = true;
            });
            leave();
        }
        (_, Stage::Moving, "failed") => {
            lost("The other device could not read what was sent. Start again.");
        }
        _ => {}
    }
}

// ── What the page shows ────────────────────────────────────────────

/// What the page shows now. `address` is what follows the `#` of the page's
/// address: an invitation there that the player has not answered is offered.
pub fn dialog_for(address: &str) -> Dialog {
    SYNC.with(|sync| {
        let sync = sync.borrow();
        let mut dialog = sync.dialog.clone();
        if dialog == Dialog::default() && address.contains("sync=") {
            dialog.offered = Invite::read(address).filter(|invite| !sync.answered.contains(&invite.compact()));
        }
        dialog
    })
}

/// What is held now, whatever the page's address.
pub fn dialog() -> Dialog {
    dialog_for("")
}

/// Have the page draw the dialog again if what it shows has changed.
pub fn show() {
    let now = dialog();
    if with_sync(|sync| sync.drawn.replace(now.clone())) != Some(now) {
        page::redraw("/api/player/sync");
    }
}

/// Forget everything, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    SYNC.with(|sync| *sync.borrow_mut() = Sync::default());
    LINK.with(|link| *link.borrow_mut() = new_link());
    wire::take();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;
    use crate::game::{crdt, room};
    use crate::store;

    const SECRET: &str = "EFGHJKLMNPQRSTUVWXYZ";
    const HANDED: &str = "ABCD-EFGHJKLMNPQRSTUVWXYZ";

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        page::reset();
        connection::reset();
        reset();
    }

    /// The socket the engine last asked for (its id and address), what has
    /// been sent on the sync channel, and everything else asked of the
    /// worker, since this was last called.
    fn asked() -> (Option<(u32, String)>, Vec<String>, Vec<serde_json::Value>) {
        let mut opened = None;
        let mut sent = Vec::new();
        let mut other = Vec::new();
        for op in wire::take() {
            if let Some(id) = op["open"].as_str().and_then(|name| name.strip_prefix("sync:")) {
                opened = Some((id.parse().unwrap(), op["url"].as_str().unwrap().to_string()));
            } else if op["send"].as_str().is_some_and(|name| name.starts_with("sync:")) {
                sent.push(op["text"].as_str().unwrap().to_string());
            } else {
                other.push(op);
            }
        }
        (opened, sent, other)
    }

    fn relay(socket: u32, message: serde_json::Value) {
        socket_event(socket, SocketEvent::Message(message.to_string()));
    }

    /// What the other device, which is `from`, says, sealed as it would be.
    fn from_peer(socket: u32, from: Side, said: serde_json::Value) {
        let sealed = Pairing::new(SECRET).seal(from, &said.to_string(), crypto::random_bytes().unwrap()).unwrap();
        relay(socket, serde_json::json!({ "type": "relay", "data": { "type": "sealed", "box": sealed } }));
    }

    /// What this device, which is `from`, sent the other: each message opened.
    fn said(sent: &[String], from: Side) -> Vec<serde_json::Value> {
        sent.iter()
            .filter_map(|text| {
                let message: serde_json::Value = serde_json::from_str(text).unwrap();
                let sealed = message["data"]["box"].as_str()?;
                assert_eq!(message["data"]["type"], "sealed");
                let opened = Pairing::new(SECRET).open(from, sealed).expect("sealed by this device");
                Some(serde_json::from_str(&opened).unwrap())
            })
            .collect()
    }

    /// The sender, showing its invitation. The secret is put in place of
    /// the one it drew, so that the test can read what it seals. Returns the
    /// socket.
    fn inviting() -> u32 {
        ask();
        invite();
        with_sync(|sync| {
            sync.secret = SECRET.to_string();
            sync.pairing = Some(Pairing::new(SECRET));
        });
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_created", "code": "ABCD", "v": 2, "peer": "none" }));
        asked();
        socket
    }

    /// The receiver, in the sender's room, having asked for the game.
    /// Returns the socket.
    fn receiving() -> u32 {
        receive(HANDED);
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_joined", "code": "ABCD", "v": 2, "peer": "present" }));
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        socket
    }

    /// The state of a device with a game in progress and a record.
    fn a_game_elsewhere() -> String {
        player_doc::init_player_doc();
        player_doc::ensure_card_state("brox_the_defiant", 3);
        player_doc::toggle_slot("brox_the_defiant", 2);
        player_doc::add_alarm(NOW, 5, "siege", "red");
        player_doc::increment_loyalty("brox_the_defiant", "2026-11-18").unwrap();
        let state = player_doc::encode_full_state();
        player_doc::init_player_doc();
        state
    }

    // ── An invitation ──

    #[test]
    fn an_invitation_is_read_however_it_is_handed_over() {
        let invite = Invite { code: "ABCD".to_string(), secret: SECRET.to_string() };
        assert_eq!(invite.compact(), HANDED);
        assert_eq!(invite.spelled(), "ABCD-EFGHJ-KLMNP-QRSTU-VWXYZ");
        assert_eq!(invite.address(), "https://www.kipukas.cards/player_data#sync=ABCD-EFGHJKLMNPQRSTUVWXYZ");
        for handed in [
            HANDED.to_string(),
            invite.spelled(),
            invite.address(),
            format!("sync={}", HANDED),
            " abcd efghj klmnp qrstu vwxyz ".to_string(),
            "abcdefghjklmnpqrstuvwxyz".to_string(),
            format!("sync={}&x=1", HANDED),
        ] {
            assert_eq!(Invite::read(&handed), Some(invite.clone()), "{}", handed);
        }
    }

    #[test]
    fn what_is_not_an_invitation_is_read_as_none() {
        for handed in [
            "",
            "ABCD",
            "sync=",
            "ABCD-EFGHJKLMNPQRSTUVWXY",   // a character short
            "ABCD-EFGHJKLMNPQRSTUVWXYZ2", // one too many
            "ABCD-EFGHJKLMNPQRSTUVWXY0",  // 0 is not in the alphabet
            "ABCD-EFGHIKLMNPQRSTUVWXYZ",  // nor is I
            "brox_the_defiant",
        ] {
            assert_eq!(Invite::read(handed), None, "{}", handed);
        }
    }

    // ── The sender ──

    #[test]
    fn the_sender_is_asked_before_anything_is_made() {
        fresh();
        ask();
        assert_eq!(dialog().stage, Stage::Asked);
        assert!(asked().0.is_none(), "nothing is connected until the player says yes");

        close();
        assert_eq!(dialog(), Dialog::default());
        // Yes without having been asked does nothing.
        invite();
        assert_eq!(dialog().stage, Stage::Idle);
        assert!(asked().0.is_none());
        fresh();
    }

    #[test]
    fn saying_yes_makes_a_room_under_a_name_made_from_a_secret_the_relay_is_not_sent() {
        fresh();
        ask();
        invite();
        let (secret, name) = with_sync(|sync| (sync.secret.clone(), sync.pairing.as_ref().unwrap().room_name.clone()));
        assert_eq!(secret.len(), 20);
        let (opened, _, other) = asked();
        let (_, url) = opened.unwrap();
        let player = with_sync(|sync| sync.player.clone());
        assert_eq!(url, format!("wss://signal.kipukas.us/ws?action=create&name={}&player={}", name, player));
        assert!(!url.contains(&secret));
        assert!(other.contains(&serde_json::json!({ "set": "sync:expire", "ms": INVITE_LASTS_MS })), "{:?}", other);

        let shown = dialog();
        assert_eq!((shown.stage, shown.side), (Stage::Inviting, Some(Side::Sender)));
        assert_eq!(shown.invite, None, "the room has no code yet");
        fresh();
    }

    #[test]
    fn the_invitation_is_the_rooms_code_and_the_secret() {
        fresh();
        inviting();
        let shown = dialog();
        assert_eq!(shown.invite, Some(Invite { code: "ABCD".to_string(), secret: SECRET.to_string() }));
        assert!(!shown.peer_here);
        fresh();
    }

    #[test]
    fn the_game_is_sent_only_when_the_other_device_asks_under_the_key() {
        fresh();
        player_doc::add_alarm(NOW, 3, "ours", "red");
        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        assert!(dialog().peer_here);
        assert!(asked().1.is_empty(), "being in the room is not asking");

        // Asked in plain words, and by a device that seals with another secret.
        relay(socket, serde_json::json!({ "type": "relay", "data": { "say": "want" } }));
        assert!(asked().1.is_empty());
        let stranger = Pairing::new("22222222222222222222").seal(Side::Receiver, r#"{"say":"want"}"#, [1; 12]).unwrap();
        relay(socket, serde_json::json!({ "type": "relay", "data": { "type": "sealed", "box": stranger } }));
        let (_, sent, _) = asked();
        assert!(said(&sent, Side::Sender).is_empty(), "nothing of the player's was sent");
        assert_eq!(dialog().stage, Stage::Idle);
        assert_eq!(dialog().error, "What the other device sent could not be read. Nothing was changed. Start again.");
        fresh();
    }

    #[test]
    fn the_sender_sends_its_whole_state_sealed_and_takes_in_what_comes_back() {
        fresh();
        // What the receiver will send back: a day of play this device lacks.
        player_doc::add_alarm(NOW, 3, "ours", "red");
        let ours = player_doc::encode_full_state();
        let ours_sv = player_doc::encode_state_vector();
        player_doc::increment_loyalty("liliel", "2026-11-17").unwrap();
        let back = player_doc::encode_diff(&ours_sv).unwrap();
        player_doc::restore_from_state(&ours).unwrap();

        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        let (_, sent, other) = asked();
        assert!(sent.iter().all(|text| !text.contains(&ours)), "the state is not sent in the clear");
        let sent = said(&sent, Side::Sender);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["say"], "game");
        assert_eq!(sent[0]["state"], ours.as_str());
        assert!(other.contains(&serde_json::json!({ "clear": "sync:expire" })), "the invitation no longer ends");
        assert_eq!(dialog().stage, Stage::Moving);

        page::take_refresh();
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "record", "update": back }));
        assert_eq!(player_doc::get_loyalty("liliel").unwrap().0, 1);
        assert_eq!(player_doc::get_alarms()[0].name, "ours", "its own game is as it was");
        let shown = dialog();
        assert_eq!((shown.stage, shown.days_brought), (Stage::Done, 1));
        assert!(page::take_refresh());

        // It says so, gives up its place in the room, and lets the socket go.
        let (_, sent, other) = asked();
        assert_eq!(said(&sent, Side::Sender), [serde_json::json!({ "say": "done" })]);
        assert_eq!(sent.last().unwrap(), r#"{"type":"leave"}"#);
        assert!(other.contains(&serde_json::json!({ "close": format!("sync:{}", socket) })), "{:?}", other);
        fresh();
    }

    #[test]
    fn an_invitation_nobody_uses_ends() {
        fresh();
        let socket = inviting();
        wait_over("expire");
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Idle);
        assert!(shown.error.starts_with("That code was not used within ten minutes"), "{}", shown.error);
        let (_, sent, other) = asked();
        assert_eq!(sent, [r#"{"type":"leave"}"#]);
        assert!(other.contains(&serde_json::json!({ "close": format!("sync:{}", socket) })));

        // Once the game is moving there is nothing left to end.
        fresh();
        let socket = inviting();
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        wait_over("expire");
        assert_eq!(dialog().stage, Stage::Moving);
        fresh();
    }

    #[test]
    fn the_other_device_coming_and_going_leaves_the_invitation_standing() {
        fresh();
        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        relay(socket, serde_json::json!({ "type": "peer_away" }));
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Inviting);
        assert!(!shown.peer_here && shown.invite.is_some());

        // Gone once the game is on its way, the move is off.
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        relay(socket, serde_json::json!({ "type": "peer_left" }));
        assert_eq!(dialog().error, "The other device went away before the game had moved. Start again.");
        fresh();
    }

    // ── The receiver ──

    #[test]
    fn an_invitation_in_the_pages_address_is_offered_and_changes_nothing() {
        fresh();
        let offered = dialog_for("sync=ABCD-EFGHJKLMNPQRSTUVWXYZ").offered.expect("offered");
        assert_eq!(offered.compact(), HANDED);
        assert_eq!(dialog(), Dialog::default(), "nothing is held of it");
        assert!(asked().0.is_none(), "and nothing is connected");
        // Whatever else follows the # is no invitation.
        assert_eq!(dialog_for("ABCD-EFGHJKLMNPQRSTUVWXYZ").offered, None);
        assert_eq!(dialog_for("sync=nonsense").offered, None);

        // Answered, it is not offered again by the same address.
        decline(HANDED);
        assert_eq!(dialog_for("sync=ABCD-EFGHJKLMNPQRSTUVWXYZ").offered, None);
        // Another is.
        assert!(dialog_for("sync=WXYZ-EFGHJKLMNPQRSTUVWXYZ").offered.is_some());
        fresh();
    }

    #[test]
    fn an_invitation_typed_or_scanned_is_offered_and_one_that_cannot_be_read_is_said() {
        fresh();
        offer("abcd-efghj-klmnp-qrstu-vwxyz");
        assert_eq!(dialog().offered.unwrap().compact(), HANDED);
        assert!(asked().0.is_none());

        close();
        offer("ABCD-123");
        let shown = dialog();
        assert_eq!(shown.offered, None);
        assert_eq!(shown.typed, "ABCD-123");
        assert_eq!(shown.error, "That is not a sync code. It is four characters, then twenty more.");
        fresh();
    }

    #[test]
    fn saying_yes_joins_the_senders_room_under_the_name_made_from_the_secret() {
        fresh();
        receive(HANDED);
        let (_, url) = asked().0.unwrap();
        let name = Pairing::new(SECRET).room_name;
        assert!(
            url.starts_with(&format!("wss://signal.kipukas.us/ws?action=join&code=ABCD&name={}&player=", name)),
            "{}",
            url
        );
        assert!(!url.contains(SECRET));
        let shown = dialog();
        assert_eq!((shown.stage, shown.side), (Stage::Joining, Some(Side::Receiver)));
        fresh();
    }

    #[test]
    fn the_receiver_asks_for_the_game_when_both_are_in_the_room() {
        fresh();
        receive(HANDED);
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_joined", "code": "ABCD", "v": 2, "peer": "present" }));
        assert!(asked().1.is_empty());
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        assert_eq!(said(&asked().1, Side::Receiver), [serde_json::json!({ "say": "want" })]);
        assert_eq!(dialog().stage, Stage::Moving);
        fresh();
    }

    #[test]
    fn the_game_takes_the_place_of_the_one_here_and_the_record_here_goes_back() {
        fresh();
        let theirs = a_game_elsewhere();
        // This device: a game of its own, and a day of play of its own.
        player_doc::add_alarm(NOW, 2, "mine", "blue");
        player_doc::increment_loyalty("liliel", "2026-11-17").unwrap();

        let socket = receiving();
        asked();
        page::take_refresh();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));

        let timers: Vec<String> = player_doc::get_alarms().into_iter().map(|alarm| alarm.name).collect();
        assert_eq!(timers, ["siege"]);
        assert!(player_doc::get_slot("brox_the_defiant", 2));
        assert_eq!(player_doc::get_loyalty("brox_the_defiant").unwrap().0, 1);
        assert_eq!(player_doc::get_loyalty("liliel").unwrap().0, 1);
        assert!(page::take_refresh(), "every view is drawn again");

        // Shown as done only once the sender has said it has what goes back.
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Moving);
        let taken = shown.taken.unwrap();
        assert_eq!((taken.days_new_here, taken.days_new_there), (1, 1));

        // What it added goes back, sealed: applied to the sender's state, it
        // gives the sender Liliel's day.
        let sent = said(&asked().1, Side::Receiver);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["say"], "record");
        let here = player_doc::encode_full_state();
        player_doc::restore_from_state(&theirs).unwrap();
        player_doc::apply_update(sent[0]["update"].as_str().unwrap()).unwrap();
        assert_eq!(player_doc::get_loyalty("liliel").unwrap().0, 1);
        player_doc::restore_from_state(&here).unwrap();

        // The sender says it has it: the receiver leaves the room.
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "done" }));
        assert_eq!((dialog().stage, dialog().confirmed), (Stage::Done, true));
        let (_, sent, other) = asked();
        assert_eq!(sent, [r#"{"type":"leave"}"#]);
        assert!(other.contains(&serde_json::json!({ "close": format!("sync:{}", socket) })));
        fresh();
    }

    #[test]
    fn a_game_that_cannot_be_read_changes_nothing_and_the_sender_is_told() {
        fresh();
        player_doc::add_alarm(NOW, 2, "mine", "blue");
        let socket = receiving();
        asked();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": "!!!" }));
        assert_eq!(player_doc::get_alarms()[0].name, "mine");
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Idle);
        assert_eq!(shown.error, "The game the other device sent could not be read. Nothing here was changed.");
        assert_eq!(said(&asked().1, Side::Receiver), [serde_json::json!({ "say": "failed" })]);
        fresh();
    }

    #[test]
    fn nothing_is_taken_from_a_device_that_does_not_seal_with_the_key() {
        fresh();
        let theirs = a_game_elsewhere();
        player_doc::add_alarm(NOW, 2, "mine", "blue");
        let socket = receiving();
        // In plain words: not heard. The move still waits for the real one.
        relay(socket, serde_json::json!({ "type": "relay", "data": { "say": "game", "state": theirs } }));
        assert_eq!(dialog().stage, Stage::Moving);
        // Sealed as the receiver itself would, handed back to it.
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "game", "state": theirs }));
        assert_eq!(player_doc::get_alarms()[0].name, "mine");
        assert_eq!(dialog().error, "What the other device sent could not be read. Nothing was changed. Start again.");
        fresh();
    }

    #[test]
    fn a_game_is_taken_once_and_only_while_one_was_asked_for() {
        fresh();
        let theirs = a_game_elsewhere();
        // Nothing was asked for.
        from_peer(1, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));
        assert!(player_doc::get_alarms().is_empty());

        let socket = receiving();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));
        assert_eq!(player_doc::merges().len(), 1);
        // The same message again, as a relay could send it.
        player_doc::add_alarm(NOW, 2, "since", "blue");
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));
        assert_eq!(player_doc::get_alarms().len(), 2, "the game here was not replaced again");
        fresh();
    }

    #[test]
    fn a_code_the_relay_does_not_know_or_that_is_not_the_one_shown_is_said() {
        for (code, why) in [
            ("not_found", "That code has ended or has been used. Start again on the other device."),
            ("name_mismatch", "That is not the code the other device is showing. Check it and try again."),
            ("full", "That code is in use by another device. Start again on the other device."),
        ] {
            fresh();
            receive(HANDED);
            let (socket, _) = asked().0.unwrap();
            socket_event(socket, SocketEvent::Open);
            relay(socket, serde_json::json!({ "type": "error", "code": code, "message": "whatever the relay says" }));
            let shown = dialog();
            assert_eq!((shown.stage, shown.error.as_str()), (Stage::Idle, why));
        }
        fresh();
    }

    #[test]
    fn a_room_the_sender_has_left_is_not_waited_in() {
        fresh();
        receive(HANDED);
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_joined", "code": "ABCD", "v": 2, "peer": "away" }));
        assert_eq!(dialog().error, "The other device is no longer showing that code. Start again there.");
        assert_eq!(asked().1, [r#"{"type":"leave"}"#]);
        fresh();
    }

    #[test]
    fn the_sender_going_away_after_the_game_arrived_leaves_it_here() {
        fresh();
        let theirs = a_game_elsewhere();
        let socket = receiving();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));
        relay(socket, serde_json::json!({ "type": "peer_away" }));
        socket_event(socket, SocketEvent::Close);
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Done);
        assert!(!shown.confirmed && shown.error.is_empty());
        assert_eq!(player_doc::get_alarms()[0].name, "siege");

        // The same if it is the socket that goes.
        fresh();
        let theirs = a_game_elsewhere();
        let socket = receiving();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));
        socket_event(socket, SocketEvent::Close);
        assert_eq!((dialog().stage, dialog().confirmed), (Stage::Done, false));
        assert!(dialog().error.is_empty());
        fresh();
    }

    // ── Either ──

    #[test]
    fn a_relay_that_cannot_be_reached_is_said() {
        fresh();
        ask();
        invite();
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Error);
        socket_event(socket, SocketEvent::Close);
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Idle);
        assert_eq!(shown.error, "The relay could not be reached. Check the connection and try again.");
        fresh();
    }

    #[test]
    fn a_socket_lost_before_the_game_has_moved_ends_the_move_and_is_not_retried() {
        fresh();
        let socket = inviting();
        socket_event(socket, SocketEvent::Close);
        assert_eq!(dialog().error, "The connection was lost before the game had moved. Nothing was changed. Start again.");
        assert_eq!(LINK.with(|link| link.borrow().state()), State::Idle);
        assert!(asked().0.is_none());
        fresh();
    }

    #[test]
    fn closing_ends_a_move_and_forgets_it() {
        fresh();
        let socket = inviting();
        close();
        assert_eq!(dialog(), Dialog::default());
        let (_, sent, other) = asked();
        assert_eq!(sent, [r#"{"type":"leave"}"#]);
        assert!(other.contains(&serde_json::json!({ "close": format!("sync:{}", socket) })), "{:?}", other);
        assert!(other.contains(&serde_json::json!({ "clear": "sync:expire" })));

        // Messages still on their way from the other device change nothing.
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        assert_eq!(dialog(), Dialog::default());
        assert!(asked().1.is_empty());
        fresh();
    }

    // ── A place in a room goes with the game ──

    const ME: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const THEM: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    /// What was asked of the worker for the room's own socket (`open`,
    /// `send` or `close`), among what `asked` returned last: each address,
    /// or text.
    fn of_room(other: &[serde_json::Value], what: &str) -> Vec<String> {
        other
            .iter()
            .filter(|op| op[what].as_str().is_some_and(|name| name.starts_with("room:")))
            .map(|op| op["url"].as_str().or(op["text"].as_str()).unwrap_or("").to_string())
            .collect()
    }

    /// This device in room `code` as `me`, connected to the relay, the
    /// room holding one timer. Returns the room's socket.
    fn in_a_room(code: &str, me: &str) -> u32 {
        crate::routes::room::resume(code, "Table", me, THEM, true, "").unwrap();
        store::claim_room();
        connection::resumed();
        let socket: u32 = wire::take()
            .iter()
            .find_map(|op| op["open"].as_str()?.strip_prefix("room:")?.parse().ok())
            .expect("the room's socket");
        connection::socket_event(socket, SocketEvent::Open);
        let joined = serde_json::json!({ "type": "room_joined", "code": code, "name": "Table", "v": 2, "peer": "none" });
        connection::socket_event(socket, SocketEvent::Message(joined.to_string()));
        crdt::add_alarm(NOW, 2, "room timer", "blue");
        crdt::export_to_local();
        crdt::take_outbox();
        store::take_saves();
        wire::take();
        socket
    }

    /// The place another device holds in room WXYZ, as it would send it.
    fn a_seat_elsewhere() -> connection::Seat {
        crdt::init_doc();
        crdt::add_alarm(NOW, 5, "siege", "red");
        let doc = crdt::encode_full_state();
        crdt::reset_doc();
        crdt::take_outbox();
        connection::Seat {
            code: "WXYZ".to_string(),
            name: "Table".to_string(),
            player: ME.to_string(),
            peer: THEM.to_string(),
            creator: true,
            doc,
        }
    }

    /// An update that brings nothing: what a receiver with no record sends back.
    fn nothing_back() -> String {
        player_doc::encode_diff(&player_doc::encode_state_vector()).unwrap()
    }

    fn timers() -> Vec<String> {
        player_doc::get_alarms().into_iter().map(|alarm| alarm.name).collect()
    }

    /// The sender, in room WXYZ, having sent its game. Returns the sync
    /// socket, the room's, and the game as it was sent.
    fn sent_from_a_room() -> (u32, u32, serde_json::Value) {
        let room_socket = in_a_room("WXYZ", ME);
        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        let game = said(&asked().1, Side::Sender).remove(0);
        (socket, room_socket, game)
    }

    #[test]
    fn a_sender_that_is_in_no_room_sends_no_place() {
        fresh();
        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "want" }));
        let game = said(&asked().1, Side::Sender).remove(0);
        assert_eq!(game["say"], "game");
        assert!(game.get("seat").is_none(), "{}", game);
        assert!(!seat_taken_up());
        fresh();
    }

    #[test]
    fn a_sender_in_a_room_sends_its_place_and_forgets_the_room_once_it_is_taken_up() {
        fresh();
        let (socket, room_socket, game) = sent_from_a_room();
        let seat: connection::Seat = serde_json::from_value(game["seat"].clone()).unwrap();
        assert_eq!((seat.code.as_str(), seat.name.as_str(), seat.creator), ("WXYZ", "Table", true));
        assert_eq!((seat.player.as_str(), seat.peer.as_str()), (ME, THEM));
        assert_eq!(seat.doc, crdt::encode_full_state(), "the room's document goes whole");
        assert!(room::in_room(), "the room is its own until the other device says it has the place");

        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "record", "update": nothing_back(), "seat": true }));
        assert!(!room::in_room());
        assert_eq!(connection::face(), connection::Face::Off);
        let shown = dialog();
        assert_eq!((shown.stage, shown.room.as_deref(), shown.unheard), (Stage::Done, Some("WXYZ"), false));
        // The relay is not told the player left: they are in the room on
        // the other device. The room's socket is let go.
        let (_, _, other) = asked();
        assert!(of_room(&other, "send").is_empty(), "{:?}", other);
        assert!(other.contains(&serde_json::json!({ "close": format!("room:{}", room_socket) })), "{:?}", other);
        // Its game is as it was, the room's timer its own again, and the
        // room is gone from what is stored.
        assert_eq!(timers(), ["room timer"]);
        assert_eq!(store::take_saves().get(store::ROOM), Some(&None));
        fresh();
    }

    #[test]
    fn a_sender_keeps_its_place_if_the_other_device_does_not_say_it_took_it_up() {
        // A device on an earlier copy of the site knows nothing of places.
        for answer in [
            serde_json::json!({ "say": "record", "update": nothing_back() }),
            serde_json::json!({ "say": "record", "update": nothing_back(), "seat": false }),
        ] {
            fresh();
            let (socket, _, _) = sent_from_a_room();
            from_peer(socket, Side::Receiver, answer);
            assert!(room::in_room());
            assert_eq!((dialog().stage, dialog().room), (Stage::Done, None));
            assert!(!seat_taken_up(), "the place is no longer on its way");
        }
        fresh();
    }

    #[test]
    fn a_receiver_takes_up_the_place_that_comes_with_the_game() {
        fresh();
        let (theirs, seat) = (a_game_elsewhere(), a_seat_elsewhere());
        let socket = receiving();
        asked();
        let game = serde_json::json!({ "say": "game", "state": theirs, "seat": seat });
        from_peer(socket, Side::Sender, game);

        // It is in the room as the player, with the room's document, and
        // returns to it on the relay the way a page load does.
        assert!(room::in_room());
        room::with_room(|r| {
            assert_eq!((r.room_code.as_str(), r.room_name.as_str(), r.creator), ("WXYZ", "Table", true));
            assert_eq!((r.player_id.as_str(), r.peer_id.as_str()), (ME, THEM));
        });
        assert_eq!(crdt::get_alarms()[0].name, "siege");
        let (_, sent, other) = asked();
        let addresses = of_room(&other, "open");
        assert_eq!(addresses.len(), 1);
        assert!(
            addresses[0].ends_with(&format!("?action=rejoin&code=WXYZ&name=Table&player={}", ME)),
            "{}",
            addresses[0]
        );
        // It says so with the record, and shows it.
        let sent = said(&sent, Side::Receiver);
        assert_eq!((sent[0]["say"].as_str(), sent[0]["seat"].as_bool()), (Some("record"), Some(true)));
        assert_eq!(dialog().room.as_deref(), Some("WXYZ"));
        // The room is stored, to be returned to from the next page.
        let saves = store::take_saves();
        assert!(saves.get(store::ROOM).unwrap().as_ref().unwrap().contains(r#""code":"WXYZ""#));
        assert_eq!(saves.get(store::ROOM_DOC).unwrap().as_deref(), Some(seat.doc.as_str()));
        fresh();
    }

    #[test]
    fn a_receiver_in_a_room_leaves_it_and_keeps_none_of_its_timers() {
        fresh();
        let theirs = a_game_elsewhere();
        let room_socket = in_a_room("HERE", "cccccccccccccccccccccccc");
        assert_eq!(timers(), ["room timer"]);
        let socket = receiving();
        asked();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs }));

        // The room went with the game this device had: the relay is told
        // the player left it, and the opponent with it.
        assert!(!room::in_room());
        let (_, sent, other) = asked();
        assert_eq!(of_room(&other, "send"), [r#"{"type":"leave"}"#]);
        assert!(other.contains(&serde_json::json!({ "close": format!("room:{}", room_socket) })), "{:?}", other);
        // The game taken is whole: its timers, and none of the room's.
        assert_eq!(timers(), ["siege"]);
        let sent = said(&sent, Side::Receiver);
        assert_eq!(sent[0]["seat"].as_bool(), Some(false));
        assert_eq!(dialog().room, None);
        assert_eq!(store::take_saves().get(store::ROOM), Some(&None));
        fresh();
    }

    #[test]
    fn a_receiver_in_a_room_leaves_it_for_the_place_that_comes_with_the_game() {
        fresh();
        let (theirs, seat) = (a_game_elsewhere(), a_seat_elsewhere());
        in_a_room("HERE", "cccccccccccccccccccccccc");
        let socket = receiving();
        asked();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs, "seat": seat }));

        let (_, _, other) = asked();
        assert_eq!(of_room(&other, "send"), [r#"{"type":"leave"}"#], "it leaves the room it was in");
        assert!(of_room(&other, "open")[0].contains("action=rejoin&code=WXYZ"), "and returns to the other");
        assert_eq!(room::with_room(|r| r.room_code.clone()), "WXYZ");
        assert_eq!(timers(), ["siege"]);
        let room_timers: Vec<String> = crdt::get_alarms().into_iter().map(|alarm| alarm.name).collect();
        assert_eq!(room_timers, ["siege"], "the room's document is the one that came");
        fresh();
    }

    #[test]
    fn a_receiver_that_already_holds_the_same_place_does_not_leave_the_room() {
        fresh();
        let (theirs, seat) = (a_game_elsewhere(), a_seat_elsewhere());
        in_a_room("WXYZ", ME);
        let socket = receiving();
        asked();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs, "seat": seat }));
        let (_, _, other) = asked();
        assert!(of_room(&other, "send").is_empty(), "{:?}", other);
        assert_eq!(room::with_room(|r| (r.room_code.clone(), r.player_id.clone())), ("WXYZ".to_string(), ME.to_string()));
        assert_eq!(timers(), ["siege"]);
        fresh();
    }

    #[test]
    fn a_place_that_cannot_be_one_is_not_taken_up() {
        for (code, player) in [("WXYZ", "not an id"), ("", ME), ("TOOLONG", ME)] {
            fresh();
            let theirs = a_game_elsewhere();
            let mut seat = a_seat_elsewhere();
            (seat.code, seat.player) = (code.to_string(), player.to_string());
            let socket = receiving();
            asked();
            from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": theirs, "seat": seat }));
            assert!(!room::in_room(), "{} {}", code, player);
            let (_, sent, other) = asked();
            assert!(of_room(&other, "open").is_empty());
            assert_eq!(said(&sent, Side::Receiver)[0]["seat"].as_bool(), Some(false));
            assert_eq!(timers(), ["siege"], "the game is taken all the same");
        }
        fresh();
    }

    #[test]
    fn a_game_that_cannot_be_read_leaves_the_receiver_in_its_room() {
        fresh();
        in_a_room("HERE", "cccccccccccccccccccccccc");
        let socket = receiving();
        asked();
        from_peer(socket, Side::Sender, serde_json::json!({ "say": "game", "state": "!!!", "seat": a_seat_elsewhere() }));
        assert_eq!(room::with_room(|r| r.room_code.clone()), "HERE");
        assert_eq!(timers(), ["room timer"]);
        assert!(of_room(&asked().2, "send").is_empty());
        fresh();
    }

    #[test]
    fn the_relay_showing_the_place_taken_up_is_enough_for_the_sender_to_forget_the_room() {
        fresh();
        let (socket, room_socket, _) = sent_from_a_room();
        // The receiver has returned to the room as the player. The relay
        // closes this device's connection, before the receiver's word arrives.
        let superseded = serde_json::json!({ "type": "superseded" });
        connection::socket_event(room_socket, SocketEvent::Message(superseded.to_string()));
        assert!(!room::in_room());
        assert_eq!(connection::face(), connection::Face::Off, "not shown as open in another tab");
        assert_eq!(dialog().stage, Stage::Moving, "the record is still to come");

        from_peer(socket, Side::Receiver, serde_json::json!({ "say": "record", "update": nothing_back(), "seat": true }));
        let shown = dialog();
        assert_eq!((shown.stage, shown.room.as_deref(), shown.unheard), (Stage::Done, Some("WXYZ"), false));
        fresh();
    }

    #[test]
    fn a_sender_that_never_hears_back_learns_from_the_relay_that_the_game_arrived() {
        let superseded = serde_json::json!({ "type": "superseded" }).to_string();
        // The connection to the other device is lost, then the relay shows
        // the place taken up; and the other way about.
        for lost_first in [true, false] {
            fresh();
            let (socket, room_socket, _) = sent_from_a_room();
            if lost_first {
                socket_event(socket, SocketEvent::Close);
                assert!(dialog().error.starts_with("The connection was lost"), "{:?}", dialog());
                connection::socket_event(room_socket, SocketEvent::Message(superseded.clone()));
            } else {
                connection::socket_event(room_socket, SocketEvent::Message(superseded.clone()));
                socket_event(socket, SocketEvent::Close);
            }
            let shown = dialog();
            assert_eq!((shown.stage, shown.side), (Stage::Done, Some(Side::Sender)), "{}", lost_first);
            assert_eq!((shown.room.as_deref(), shown.unheard, shown.error.as_str()), (Some("WXYZ"), true, ""));
            assert!(!room::in_room());
            assert_eq!(timers(), ["room timer"], "its own game is as it was");
        }
        fresh();
    }

    #[test]
    fn coming_back_elsewhere_is_another_tab_unless_a_place_was_sent_a_moment_ago() {
        let superseded = serde_json::json!({ "type": "superseded" }).to_string();
        // No move at all.
        fresh();
        let room_socket = in_a_room("WXYZ", ME);
        connection::socket_event(room_socket, SocketEvent::Message(superseded.clone()));
        assert!(room::in_room());
        assert_eq!(connection::face(), connection::Face::Moved);

        // A move that was lost a while ago.
        fresh();
        let (socket, room_socket, _) = sent_from_a_room();
        socket_event(socket, SocketEvent::Close);
        with_sync(|sync| sync.seat_out = Some(("WXYZ".to_string(), clock::now_ms() - SEAT_OUT_MS - 1_000.0)));
        connection::socket_event(room_socket, SocketEvent::Message(superseded.clone()));
        assert!(room::in_room());
        assert_eq!(connection::face(), connection::Face::Moved);
        assert!(dialog().error.starts_with("The connection was lost"));

        // A move called off, or put away, and another begun.
        fresh();
        let (_, room_socket, _) = sent_from_a_room();
        close();
        connection::socket_event(room_socket, SocketEvent::Message(superseded));
        assert!(room::in_room());
        fresh();
    }

    #[test]
    fn a_player_changing_their_state_sends_nothing_to_the_other_device() {
        fresh();
        let socket = inviting();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        asked();
        player_doc::add_alarm(NOW, 3, "since", "red");
        // The change is stored, and nothing is asked of the worker.
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert!(handed["save"][store::PLAYER_DOC].is_string(), "{}", handed);
        assert!(handed["wire"].is_null(), "{}", handed);
        fresh();
    }

    #[test]
    fn the_relay_given_for_local_work_is_used_here_too() {
        fresh();
        connection::use_relay("ws://localhost:8792/ws?who=A");
        ask();
        invite();
        assert!(asked().0.unwrap().1.starts_with("ws://localhost:8792/ws?who=A&action=create&name="));
        fresh();
    }

    #[test]
    fn the_dialog_is_drawn_again_when_what_it_shows_changes_and_not_otherwise() {
        fresh();
        show();
        page::take_redraws();
        show();
        assert!(page::take_redraws().is_empty());
        ask();
        assert_eq!(page::take_redraws(), ["/api/player/sync"]);
        fresh();
    }
}
