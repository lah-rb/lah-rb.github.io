//! The room's connection: this player's place in a room on the relay, and
//! what the two players say to each other through it.
//!
//! The relay forwards messages between the two players of a room without
//! reading them; the game stays in the players' engines. This module is
//! what the engine does with that:
//!
//! - what the player asks for in the room dialog: create a room, join one,
//!   leave ([`create`], [`join`], [`leave`]);
//! - what the relay says: the room was created or joined, the opponent came
//!   or went, we were refused (`relay_said`);
//! - what the opponent says: who they are, and the exchange that brings the
//!   two copies of the room's document together (`peer_said`);
//! - what the page shows of all this: the room dialog ([`dialog`]) and the
//!   status pill ([`face`]). Both are drawn by `routes::room_status`.
//!
//! The socket itself is a [`Link`], which keeps one socket and retries for
//! as long as the player is in a room. The engine's worker holds the real
//! socket and the clock (`crate::wire`).
//!
//! ## What travels
//!
//! To the relay, in the address: `action` (`create`, `join`, `rejoin`),
//! `code`, `name`, and `player`, this player's id. It goes in the address
//! and not in a first message because the relay hands each connection to its
//! room before any message is exchanged.
//!
//! From the relay: `room_created`, `room_joined`, `peer_joined` (with
//! `peerId`), `relay` (with `data`, what the opponent sent), `peer_away`,
//! `peer_left`, `superseded`, `error` (with `code` and `message`). To it:
//! `{"type":"relay","data":…}` and `{"type":"leave"}`.
//!
//! Between the players, as `data`: `hello` (with `proto` and `playerId`),
//! then `yrs_sv`, `yrs_sv_reply` and `yrs_update`, which carry the room
//! document's state vectors and updates.

use std::cell::RefCell;

use crate::game::{crdt, room};
use crate::link::{Link, Owner, State, Timer, Told};
use crate::routes::room as life;
use crate::wire::{self, Channel, SocketEvent};
use crate::{page, store};

/// The relay. Moving it to another host is a change to this line (and to the
/// matching route in signaling-server/wrangler.jsonc).
///
/// For local work, run the relay locally (`deno task dev` in
/// signaling-server/) and point at it without editing code:
/// `localStorage.setItem('kipukas_relay_url', 'ws://localhost:8787/ws')`.
pub const RELAY: &str = "wss://signal.kipukas.us/ws";

/// Both players' engines must speak the same room-document format. Each says
/// hello with this number when the opponent appears.
///
/// 3: a timer is kept as each player's dated version of it, so that the
/// later of two changes made apart wins (`game::crdt`).
pub const PROTOCOL: u64 = 3;

/// An opponent who has not said hello after this long is running an older
/// copy of the site, which never does.
const HELLO_TIMEOUT_MS: u32 = 3_000;

/// Every page change reconnects for a moment, so the pill only shows that
/// something is being waited for if the wait lasts this long.
const SHOW_WAITING_AFTER_MS: u32 = 1_500;

/// What the player just asked for and the relay has not yet answered.
#[derive(Debug, Clone, PartialEq)]
enum Want {
    Create,
    Join { code: String },
}

#[derive(Debug, Clone)]
struct Pending {
    want: Want,
    /// The room name the player typed.
    name: String,
    /// The id chosen for this player in the room being created or joined.
    player: String,
}

/// What the status pill shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    /// Not in a room: no pill.
    #[default]
    Off,
    /// Connected, opponent present.
    Connected,
    /// This player's own connection is down and being retried.
    Reconnecting,
    /// Connected to the relay; the opponent has not joined yet, or their
    /// connection has closed (a page change, a locked phone).
    WaitingForOpponent,
    /// The opponent is on an older copy of the site; timers and combat will
    /// not sync until they update.
    OpponentOutdated,
    /// This player's place was taken while they were away.
    Expired,
    /// This player has the room open in a newer tab, which holds the
    /// connection.
    Moved,
}

impl Face {
    /// Whether this is something being waited for, which is shown only if
    /// it lasts.
    fn waits(self) -> bool {
        matches!(self, Face::Reconnecting | Face::WaitingForOpponent | Face::OpponentOutdated)
    }
}

/// Where the player stands, for the room dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Not in a room: the dialog offers to create or join one.
    Out,
    /// A create or join is on its way to the relay.
    Connecting,
    /// In a room, the opponent not present.
    Waiting,
    /// In a room with the opponent.
    Connected,
}

/// What the room dialog shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub stage: Stage,
    pub name: String,
    pub code: String,
    /// Why the last thing asked for did not happen. Empty when it did.
    pub notice: String,
}

#[derive(Default)]
struct Conn {
    /// A relay address to use in place of [`RELAY`] (local work).
    relay: Option<String>,
    /// While this is `None`, a connection is a return to the room the
    /// player is in: it asks the relay to `rejoin` and never starts the
    /// room's shared state afresh.
    pending: Option<Pending>,
    /// The opponent is in the room and can be reached.
    peer_present: bool,
    /// The opponent speaks another protocol, or none.
    outdated: bool,
    /// The opponent has said hello in our protocol since they last appeared.
    /// The room's shared state is exchanged with them only then: an update
    /// in one format applied by a copy of the site that expects another
    /// would change what that player sees of their own timers.
    agreed: bool,
    /// The wait for the opponent's hello is running.
    hello_due: bool,
    /// The player's place in the room was taken. Shown until dismissed or
    /// until the connection next does anything.
    expired: bool,
    /// Another tab holds the room.
    moved: bool,
    notice: String,
    /// What the pill shows now.
    face: Face,
    /// A face that waits is wanted and the wait before showing it is running.
    face_due: bool,
    /// The dialog as it was when the page was last asked to draw it.
    drawn: Option<Dialog>,
}

thread_local! {
    static CONN: RefCell<Conn> = RefCell::new(Conn::default());
    static LINK: RefCell<Link> = RefCell::new(new_link());
}

fn new_link() -> Link {
    Link::new(Owner {
        url: Box::new(relay_address),
        // Keep retrying for as long as the player is in a room. A create or
        // join they just asked for is different: if the relay cannot be
        // reached, say so and stop.
        should_retry: Box::new(|| !is_pending() && room::in_room()),
        random: Box::new(fastrand::f64),
    })
}

fn is_pending() -> bool {
    CONN.with(|conn| conn.borrow().pending.is_some())
}

fn with_conn<R>(f: impl FnOnce(&mut Conn) -> R) -> R {
    CONN.with(|conn| f(&mut conn.borrow_mut()))
}

// ── The link ───────────────────────────────────────────────────────

/// Do something to the link, then pass on what it asks for and act on what
/// it has to tell. Nothing of the connection's own state may be borrowed
/// across this: the link asks its owner questions (`new_link`).
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
            wire::pass(Channel::Room, ask);
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

fn link_state() -> State {
    LINK.with(|link| link.borrow().state())
}

/// The relay address for one attempt, saying what this player wants: create
/// a room, join one, or return to the one they are in.
fn relay_address() -> String {
    let (pending, base) = CONN.with(|conn| {
        let conn = conn.borrow();
        (conn.pending.clone(), conn.relay.clone().unwrap_or_else(|| RELAY.to_string()))
    });
    let mut wants: Vec<(&str, String)> = Vec::new();
    let player = match pending {
        Some(Pending { want: Want::Create, name, player }) => {
            wants.push(("action", "create".to_string()));
            wants.push(("name", name));
            player
        }
        Some(Pending { want: Want::Join { code }, name, player }) => {
            wants.push(("action", "join".to_string()));
            wants.push(("code", code));
            wants.push(("name", name));
            player
        }
        None => room::with_room(|r| {
            if !r.room_code.is_empty() {
                wants.push(("action", "rejoin".to_string()));
                wants.push(("code", r.room_code.clone()));
                wants.push(("name", r.room_name.clone()));
            }
            r.player_id.clone()
        }),
    };
    wants.push(("player", player));
    with_query(&base, &wants)
}

/// The relay's address with `set` in its query. For a pairing of two
/// devices, which meets on the same relay (`game::device_sync`).
pub fn relay_address_with(set: &[(&str, String)]) -> String {
    let base = CONN.with(|conn| conn.borrow().relay.clone().unwrap_or_else(|| RELAY.to_string()));
    with_query(&base, set)
}

/// `address` with each of `set` in its query, in place of any value the
/// query already has for that name.
fn with_query(address: &str, set: &[(&str, String)]) -> String {
    let address = address.split('#').next().unwrap_or("");
    let (base, query) = address.split_once('?').unwrap_or((address, ""));
    let mut pairs: Vec<(String, String)> = crate::routes::util::parse_form_body(query)
        .into_iter()
        .filter(|(name, _)| !set.iter().any(|(set_name, _)| set_name == name))
        .collect();
    pairs.extend(set.iter().map(|(name, value)| (name.to_string(), value.clone())));
    let query: Vec<String> =
        pairs.iter().map(|(name, value)| format!("{}={}", form_encode(name), form_encode(value))).collect();
    format!("{}?{}", base, query.join("&"))
}

/// Encode a value for a query the way a browser's `URLSearchParams` does.
fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// A string as JSON.
fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// Send a message to the opponent through the relay. One that cannot be sent
/// just now is made up for by the exchange of state when the two are next
/// connected.
fn send_to_peer(data: &str) {
    link(|link| link.send(&format!(r#"{{"type":"relay","data":{}}}"#, data)));
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

/// One of the connection's own waits is over.
pub fn wait_over(name: &str) {
    match name {
        "hello" => {
            // No hello from the opponent: they need to update.
            if with_conn(|conn| std::mem::take(&mut conn.hello_due)) {
                with_conn(|conn| conn.outdated = true);
            }
        }
        "face" => {
            // What was being waited for has lasted: show it.
            if with_conn(|conn| std::mem::take(&mut conn.face_due)) {
                let wanted = face_wanted();
                with_conn(|conn| conn.face = wanted);
                page::redraw("/api/room/pill");
            }
        }
        _ => {}
    }
    show();
}

// ── What the page reports ──────────────────────────────────────────

/// The engine was started in the room it was in on the last page: all that
/// is missing is the socket.
pub fn resumed() {
    if room::in_room() {
        link(|link| link.start());
    }
    show();
}

/// Use this relay address in place of [`RELAY`], if it is one.
pub fn use_relay(address: &str) {
    if address.starts_with("ws://") || address.starts_with("wss://") {
        with_conn(|conn| conn.relay = Some(address.to_string()));
    }
}

/// Something suggests the network is back: the page is in view again, or
/// the browser says it is online. Retry now rather than waiting out the
/// backoff. `alive` names the sockets the worker finds open.
pub fn came_back(alive: &[String]) {
    let alive = wire::is_alive(alive, Channel::Room, LINK.with(|link| link.borrow().socket()));
    link(|link| link.kick(alive));
    show();
}

/// This player's connection now belongs to another tab (a card opened from
/// the phone's camera app, for example): stop, and do not reconnect. Two
/// tabs that both kept reconnecting would take the connection from each
/// other for ever. The page is loaded again the next time it is shown, and
/// takes the room back.
pub fn go_quiet() {
    if !room::in_room() {
        return;
    }
    with_conn(|conn| {
        conn.peer_present = false;
        conn.agreed = false;
    });
    clear_hello();
    life::peer_gone();
    link(|link| link.stop());
    with_conn(|conn| conn.moved = true);
    show();
}

// ── What the player asks for ───────────────────────────────────────

/// Create a room of this name.
pub fn create(name: &str) {
    if room::in_room() {
        return;
    }
    connect_for(Pending { want: Want::Create, name: name.trim().to_string(), player: new_player_id() });
    show();
}

/// Join the room of this code, giving its name.
pub fn join(code: &str, name: &str) {
    if room::in_room() {
        return;
    }
    let code = code.trim().to_uppercase();
    if code.chars().count() != 4 {
        with_conn(|conn| conn.notice = "Please enter a 4-character room code.".to_string());
    } else {
        connect_for(Pending { want: Want::Join { code }, name: name.trim().to_string(), player: new_player_id() });
    }
    show();
}

/// Leave the room for good: give up this player's place, so that the
/// opponent is told and the place is free for someone else. (Being away
/// keeps the place; leaving does not.) Also how a create or join that is
/// still on its way is called off.
pub fn leave() {
    link(|link| link.send(r#"{"type":"leave"}"#));
    leave_room();
    with_conn(|conn| conn.notice.clear());
    show();
}

/// The player closed the room dialog: what it had to say has been read.
pub fn dialog_closed() {
    with_conn(|conn| conn.notice.clear());
    show();
}

/// The player dismissed the pill's "no longer in the room".
pub fn dismiss_expired() {
    with_conn(|conn| conn.expired = false);
    show();
}

/// A new random id for this player in a room: 24 hex characters.
fn new_player_id() -> String {
    format!("{:016x}{:08x}", fastrand::u64(..), fastrand::u32(..))
}

/// Connect to the relay to carry out a create or join the player asked for.
fn connect_for(pending: Pending) {
    // Stop first: stopping with something pending reads as "could not
    // connect".
    link(|link| link.stop());
    with_conn(|conn| {
        conn.notice.clear();
        conn.pending = Some(pending);
    });
    link(|link| link.start());
}

/// Leave the room here: the engine forgets it and goes back to playing alone.
fn leave_room() {
    with_conn(|conn| {
        conn.peer_present = false;
        conn.pending = None;
        conn.outdated = false;
        conn.agreed = false;
    });
    clear_hello();
    link(|link| link.stop());
    with_conn(|conn| {
        conn.expired = false;
        conn.moved = false;
    });
    // Only a room can be left: leaving copies the room's timers over the
    // player's own.
    if room::in_room() {
        life::leave();
    }
}

// ── What the link tells ────────────────────────────────────────────

fn link_state_changed(next: State, prev: State) {
    // "No longer in the room" and "open in another tab" give way to
    // whatever the connection does next.
    let (peer_present, asked) = with_conn(|conn| {
        conn.expired = false;
        conn.moved = false;
        (conn.peer_present, next == State::Idle && conn.pending.take().is_some())
    });

    // Socket lost while in a room: the opponent cannot reach us until we
    // are back.
    if prev == State::Open && next != State::Open && peer_present {
        peer_is_gone();
    }

    // A create or join the player asked for never reached the relay.
    if asked {
        with_conn(|conn| conn.notice = "Could not connect to signaling server. Try again.".to_string());
    }
}

/// The opponent is no longer reachable (their socket or ours is gone).
fn peer_is_gone() {
    with_conn(|conn| {
        conn.peer_present = false;
        conn.agreed = false;
    });
    clear_hello();
    life::peer_gone();
}

fn clear_hello() {
    if with_conn(|conn| std::mem::take(&mut conn.hello_due)) {
        wire::clear(Channel::Room, "hello");
    }
}

/// A message from the relay.
fn relay_said(message: &serde_json::Value) {
    let text = |name: &str| message[name].as_str().unwrap_or("");
    match text("type") {
        "room_created" => entered(text("code"), text("name"), true),
        "room_joined" => entered(text("code"), text("name"), false),
        // Both players are connected. Sent on every (re)connect of either.
        "peer_joined" => {
            with_conn(|conn| conn.peer_present = true);
            set_peer_id(text("peerId"));
            peer_connected();
        }
        // What the opponent sent, passed on by the relay.
        "relay" => peer_said(&message["data"]),
        // The opponent's connection closed (a page change, a locked phone);
        // they still have their place in the room. Or they left for good.
        "peer_away" | "peer_left" => peer_is_gone(),
        // This player connected again elsewhere (another tab, which the
        // storage event did not reach in time). The relay is closing this
        // socket.
        "superseded" => go_quiet(),
        "error" => refused(text("code"), text("message")),
        _ => {}
    }
}

/// The relay created the room, or let this player into it.
fn entered(code: &str, name: &str, created: bool) {
    link(|link| link.settled());
    match with_conn(|conn| conn.pending.take()) {
        // Only a create or join the player asked for starts the room's
        // shared state. A return to the room (a page load, a reconnect)
        // must not: that would start a fresh shared document and seed it
        // again, which duplicated timers and wiped combat.
        Some(pending) => {
            let name = if created || !name.is_empty() { name } else { pending.name.as_str() };
            life::enter(code, name, &pending.player, created);
        }
        None if room::in_room() => {
            room::with_room_mut(|r| {
                if !code.is_empty() {
                    r.room_code = code.to_uppercase();
                }
                if !name.is_empty() {
                    r.room_name = name.to_string();
                }
            });
            store::claim_room();
        }
        None => {}
    }
}

/// Record who the opponent is. Only their entries count as the opponent's
/// in combat.
fn set_peer_id(id: &str) {
    if id.is_empty() || room::with_room(|r| r.peer_id == id) {
        return;
    }
    life::set_peer(id);
    store::claim_room();
}

/// Both players are in the room and can exchange messages.
fn peer_connected() {
    life::peer_arrived();
    // Whoever this is must say hello before anything is exchanged with
    // them. Both players are told of every (re)connect of either, and both
    // say hello on it, so a hello is always on its way.
    with_conn(|conn| conn.agreed = false);

    // Say who we are and which room-document format we speak. An opponent
    // on an older copy of the site never answers; the player is told after
    // a moment.
    let me = room::with_room(|r| r.player_id.clone());
    send_to_peer(&format!(r#"{{"type":"hello","proto":{},"playerId":{}}}"#, PROTOCOL, quoted(&me)));
    with_conn(|conn| conn.hello_due = true);
    wire::set(Channel::Room, "hello", HELLO_TIMEOUT_MS);

    // Exchange state vectors so that both room documents converge after
    // every (re)connect. This is what repairs anything missed while either
    // player was away: timers and fists combat both live in the room
    // document.
    send_to_peer(&format!(r#"{{"type":"yrs_sv","sv":{}}}"#, quoted(&crdt::encode_state_vector())));
}

/// A message from the opponent.
fn peer_said(data: &serde_json::Value) {
    match data["type"].as_str().unwrap_or("") {
        // The opponent's id and protocol.
        "hello" => {
            clear_hello();
            let same = data["proto"].as_u64() == Some(PROTOCOL);
            with_conn(|conn| {
                conn.outdated = !same;
                conn.agreed = same;
            });
            set_peer_id(data["playerId"].as_str().unwrap_or(""));
        }
        // Nothing of the room's state is taken from, or given to, an
        // opponent who has not said hello in our protocol. (Theirs comes
        // before anything else they send on a connection, so nothing is
        // missed by this; and whatever was not exchanged meanwhile is, by
        // the handshake that follows their next hello.)
        "yrs_sv" | "yrs_sv_reply" | "yrs_update" if !with_conn(|conn| conn.agreed) => {}
        // The opponent's state vector: send what they lack, then our own
        // state vector so that they can send what we lack.
        "yrs_sv" => {
            send_what_they_lack(data["sv"].as_str().unwrap_or(""));
            send_to_peer(&format!(r#"{{"type":"yrs_sv_reply","sv":{}}}"#, quoted(&crdt::encode_state_vector())));
        }
        // Their state vector, in answer to ours.
        "yrs_sv_reply" => send_what_they_lack(data["sv"].as_str().unwrap_or("")),
        // An update of the room document (timers, fists combat, or both).
        // The views that show the room are drawn again, and anything to
        // send back is queued, by `apply_peer_update`.
        "yrs_update" => {
            let _ = life::apply_peer_update(data["update"].as_str().unwrap_or(""));
        }
        _ => {}
    }
}

fn send_what_they_lack(their_state_vector: &str) {
    if let Ok(diff) = crdt::encode_diff(their_state_vector) {
        if !diff.is_empty() {
            send_update(&diff);
        }
    }
}

fn send_update(update: &str) {
    send_to_peer(&format!(r#"{{"type":"yrs_update","update":{}}}"#, quoted(update)));
}

/// Send the opponent the changes this player has made to the room's shared
/// state (a turn timer advanced, a choice locked in for a round of fists),
/// which the engine queued as it made them. Called before every handover.
pub fn send_updates() {
    // To an opponent on another protocol nothing is sent: the changes stay
    // in this player's room document and reach the opponent by the
    // handshake once both speak the same.
    let agreed = with_conn(|conn| conn.agreed);
    for update in crdt::take_outbox() {
        if agreed {
            send_update(&update);
        }
    }
    show();
}

/// The relay refused us.
fn refused(code: &str, message: &str) {
    let said = if message.is_empty() { "The relay refused the connection." } else { message };

    if with_conn(|conn| conn.pending.take()).is_some() {
        // A create or join the player asked for was refused (wrong code or
        // name, room full). What was pending is cleared first, so that
        // stopping is not read as a failure to connect.
        link(|link| link.stop());
        with_conn(|conn| conn.notice = said.to_string());
        return;
    }

    // Refused on returning to a room: this player no longer has a place in
    // it (someone joined in their stead while they were away). Leave; the
    // room's timers are kept as the player's own.
    leave_room();
    with_conn(|conn| {
        conn.expired = true;
        conn.notice = if code == "full" {
            "Your place in this room was taken while you were away. Create or join a room to keep playing."
                .to_string()
        } else {
            said.to_string()
        };
    });
}

// ── What the page shows ────────────────────────────────────────────

/// What the room dialog shows now.
pub fn dialog() -> Dialog {
    let (pending, notice) = CONN.with(|conn| {
        let conn = conn.borrow();
        (conn.pending.is_some(), conn.notice.clone())
    });
    room::with_room(|r| Dialog {
        stage: if r.room_code.is_empty() {
            if pending { Stage::Connecting } else { Stage::Out }
        } else if r.connected {
            Stage::Connected
        } else {
            Stage::Waiting
        },
        name: r.room_name.clone(),
        code: r.room_code.clone(),
        notice,
    })
}

/// What the status pill shows now.
pub fn face() -> Face {
    CONN.with(|conn| conn.borrow().face)
}

/// What the pill should show, going by where things stand.
fn face_wanted() -> Face {
    let in_room = room::in_room();
    let open = link_state() == State::Open;
    CONN.with(|conn| {
        let conn = conn.borrow();
        if conn.expired {
            Face::Expired
        } else if conn.moved {
            Face::Moved
        } else if !in_room {
            Face::Off
        } else if !open {
            Face::Reconnecting
        } else if !conn.peer_present {
            Face::WaitingForOpponent
        } else if conn.outdated {
            Face::OpponentOutdated
        } else {
            Face::Connected
        }
    })
}

/// Have the page draw again whatever of the connection it shows that has
/// changed. Called at the end of everything that can change it.
pub fn show() {
    // The pill. Something being waited for is shown only if it lasts.
    let wanted = face_wanted();
    let (shown, due) = CONN.with(|conn| {
        let conn = conn.borrow();
        (conn.face, conn.face_due)
    });
    if wanted.waits() && wanted != shown {
        if !due {
            with_conn(|conn| conn.face_due = true);
            wire::set(Channel::Room, "face", SHOW_WAITING_AFTER_MS);
        }
    } else {
        if due {
            with_conn(|conn| conn.face_due = false);
            wire::clear(Channel::Room, "face");
        }
        if wanted != shown {
            with_conn(|conn| conn.face = wanted);
            page::redraw("/api/room/pill");
        }
    }

    // The dialog.
    let now = dialog();
    if with_conn(|conn| conn.drawn.replace(now.clone())) != Some(now) {
        page::redraw("/api/room/status");
    }
}

/// Forget everything, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    CONN.with(|conn| *conn.borrow_mut() = Conn::default());
    LINK.with(|link| *link.borrow_mut() = new_link());
    wire::take();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;
    use crate::game::{fists, player_doc};
    use crate::routes::room_status;

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        page::reset();
        reset();
    }

    /// What the engine has asked of its worker since this was last called,
    /// as (what, name) with the text or address after: `open room:1 <url>`.
    fn asked() -> Vec<String> {
        wire::take()
            .into_iter()
            .map(|op| {
                let op = op.as_object().unwrap();
                let (what, name) = op.iter().find(|(key, _)| !matches!(key.as_str(), "url" | "text" | "ms")).unwrap();
                let rest = ["url", "text", "ms"].iter().find_map(|key| op.get(*key));
                match rest {
                    Some(serde_json::Value::String(text)) => format!("{} {} {}", what, name.as_str().unwrap(), text),
                    Some(other) => format!("{} {} {}", what, name.as_str().unwrap(), other),
                    None => format!("{} {}", what, name.as_str().unwrap()),
                }
            })
            .collect()
    }

    /// The id of the socket the engine last asked for, and its address.
    fn opened(asked: &[String]) -> (u32, String) {
        let open = asked.iter().rev().find(|line| line.starts_with("open room:")).expect("a socket was asked for");
        let mut parts = open.splitn(3, ' ');
        let id = parts.nth(1).unwrap().trim_start_matches("room:").parse().unwrap();
        (id, parts.next().unwrap().to_string())
    }

    fn sent(asked: &[String]) -> Vec<String> {
        asked
            .iter()
            .filter(|line| line.starts_with("send room:"))
            .map(|line| line.splitn(3, ' ').nth(2).unwrap().to_string())
            .collect()
    }

    fn relay(socket: u32, message: serde_json::Value) {
        socket_event(socket, SocketEvent::Message(message.to_string()));
    }

    fn from_peer(socket: u32, data: serde_json::Value) {
        relay(socket, serde_json::json!({ "type": "relay", "data": data }));
    }

    /// Create a room and have the relay answer. Returns the socket.
    fn in_a_room() -> u32 {
        create("Table");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_created", "code": "ABCD", "name": "Table", "v": 2 }));
        asked();
        socket
    }

    const THEM: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    /// As `in_a_room`, with the opponent there and known.
    fn with_the_opponent() -> u32 {
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        asked();
        page::take_redraws();
        socket
    }

    // ── The address ────────────────────────────────────────────────

    #[test]
    fn the_address_says_what_the_player_wants_the_way_a_browser_would_write_it() {
        fresh();
        create("  My Room & Co  ");
        let (_, url) = opened(&asked());
        let player = with_conn(|conn| conn.pending.clone().unwrap().player);
        assert_eq!(url, format!("wss://signal.kipukas.us/ws?action=create&name=My+Room+%26+Co&player={}", player));
        assert_eq!(player.len(), 24);
        assert!(fists::is_valid_player_id(&player));
        fresh();
    }

    #[test]
    fn joining_sends_the_code_in_capitals_and_the_name() {
        fresh();
        join(" abcd ", "Table");
        let (_, url) = opened(&asked());
        assert!(url.starts_with("wss://signal.kipukas.us/ws?action=join&code=ABCD&name=Table&player="), "{}", url);
        fresh();
    }

    #[test]
    fn a_relay_address_given_for_local_work_is_used_and_keeps_its_own_query() {
        fresh();
        use_relay("ws://localhost:8792/ws?who=A");
        create("Table");
        let (_, url) = opened(&asked());
        assert!(url.starts_with("ws://localhost:8792/ws?who=A&action=create&name=Table&player="), "{}", url);

        // Anything that is not a socket address is not taken for one.
        fresh();
        use_relay("javascript:alert(1)");
        create("Table");
        assert!(opened(&asked()).1.starts_with("wss://signal.kipukas.us/ws?"));
        fresh();
    }

    #[test]
    fn a_name_in_another_script_is_encoded_byte_by_byte() {
        assert_eq!(form_encode("Café 東"), "Caf%C3%A9+%E6%9D%B1");
        assert_eq!(form_encode("a*b-c.d_e~f"), "a*b-c.d_e%7Ef");
    }

    // ── Creating and joining ───────────────────────────────────────

    #[test]
    fn creating_a_room_enters_it_when_the_relay_answers_and_not_before() {
        fresh();
        player_doc::add_alarm(NOW, 3, "mine", "red");
        create("Table");
        assert_eq!(dialog().stage, Stage::Connecting);
        assert!(!room::in_room());

        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        assert!(!room::in_room());
        relay(socket, serde_json::json!({ "type": "room_created", "code": "abcd", "name": "Table", "v": 2 }));

        assert!(room::in_room());
        let shown = dialog();
        assert_eq!((shown.stage, shown.code.as_str(), shown.name.as_str()), (Stage::Waiting, "ABCD", "Table"));
        room::with_room(|r| {
            assert!(r.creator);
            assert_eq!(r.player_id.len(), 24);
        });
        // The player's own timer is now the room's.
        assert_eq!(crdt::get_alarms().len(), 1);
        assert!(store::is_owner());
        fresh();
    }

    #[test]
    fn joining_takes_the_rooms_name_from_the_relay_or_else_what_was_typed() {
        fresh();
        join("ABCD", "typed");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_joined", "code": "ABCD" }));
        room::with_room(|r| {
            assert_eq!(r.room_name, "typed");
            assert!(!r.creator);
        });

        fresh();
        join("ABCD", "typed");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_joined", "code": "ABCD", "name": "Table" }));
        room::with_room(|r| assert_eq!(r.room_name, "Table"));
        fresh();
    }

    #[test]
    fn a_code_that_is_not_four_characters_is_refused_without_connecting() {
        fresh();
        join("ABC", "Table");
        assert!(asked().is_empty());
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Out);
        assert_eq!(shown.notice, "Please enter a 4-character room code.");
        dialog_closed();
        assert_eq!(dialog().notice, "");
        fresh();
    }

    #[test]
    fn a_create_that_cannot_reach_the_relay_says_so_and_does_not_retry() {
        fresh();
        create("Table");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Error);
        socket_event(socket, SocketEvent::Close);

        let shown = dialog();
        assert_eq!(shown.stage, Stage::Out);
        assert_eq!(shown.notice, "Could not connect to signaling server. Try again.");
        assert!(!asked().iter().any(|line| line.starts_with("set room:retry")), "no retry");
        assert_eq!(link_state(), State::Idle);

        // Trying again clears what was said.
        create("Table");
        assert_eq!(dialog().notice, "");
        assert_eq!(dialog().stage, Stage::Connecting);
        fresh();
    }

    #[test]
    fn a_join_the_relay_refuses_shows_what_the_relay_said() {
        fresh();
        join("ABCD", "Wrong");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "error", "code": "not_found", "message": "Room not found" }));

        let shown = dialog();
        assert_eq!(shown.stage, Stage::Out);
        assert_eq!(shown.notice, "Room not found");
        assert_eq!(link_state(), State::Idle);
        assert!(asked().iter().any(|line| *line == format!("close room:{}", socket)));
        assert_eq!(face(), Face::Off);
        fresh();
    }

    #[test]
    fn a_create_on_its_way_can_be_called_off_and_the_players_timers_are_left_alone() {
        fresh();
        player_doc::add_alarm(NOW, 3, "mine", "red");
        create("Table");
        leave();
        assert_eq!(dialog().stage, Stage::Out);
        assert_eq!(dialog().notice, "");
        assert_eq!(link_state(), State::Idle);
        assert_eq!(player_doc::get_alarms().len(), 1);
        fresh();
    }

    #[test]
    fn nothing_is_created_or_joined_from_inside_a_room() {
        fresh();
        let socket = in_a_room();
        create("Other");
        join("WXYZ", "Other");
        assert!(asked().is_empty());
        room::with_room(|r| assert_eq!(r.room_code, "ABCD"));
        let _ = socket;
        fresh();
    }

    // ── The opponent ───────────────────────────────────────────────

    #[test]
    fn when_the_opponent_arrives_we_say_hello_and_send_our_state_vector() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));

        let me = room::with_room(|r| r.player_id.clone());
        let asked = asked();
        assert_eq!(
            sent(&asked),
            [
                format!(r#"{{"type":"relay","data":{{"type":"hello","proto":3,"playerId":"{}"}}}}"#, me),
                format!(r#"{{"type":"relay","data":{{"type":"yrs_sv","sv":"{}"}}}}"#, crdt::encode_state_vector()),
            ]
        );
        assert!(asked.contains(&"set room:hello 3000".to_string()));
        assert_eq!(dialog().stage, Stage::Connected);
        room::with_room(|r| assert_eq!(r.peer_id, THEM));
        fresh();
    }

    #[test]
    fn an_opponent_who_never_says_hello_needs_to_update_and_one_who_does_is_fine() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined" }));
        wait_over("hello");
        wait_over("face");
        assert_eq!(face(), Face::OpponentOutdated);

        // Their hello arrives after all (a slow phone): all is well.
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        assert_eq!(face(), Face::Connected);
        room::with_room(|r| assert_eq!(r.peer_id, THEM));

        // One speaking another protocol is not.
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL - 1, "playerId": THEM }));
        wait_over("face");
        assert_eq!(face(), Face::OpponentOutdated);
        fresh();
    }

    #[test]
    fn a_hello_in_time_calls_off_the_wait_for_it() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        asked();
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        assert!(asked().contains(&"clear room:hello".to_string()));
        // A wait reported late all the same changes nothing.
        wait_over("hello");
        assert_eq!(face(), Face::Connected);
        fresh();
    }

    #[test]
    fn the_two_documents_are_brought_together_by_exchanging_state_vectors() {
        fresh();
        let socket = with_the_opponent();
        crdt::add_alarm(NOW, 4, "ours", "blue");
        crdt::take_outbox();

        // Their state vector, from a document that has nothing of ours.
        let theirs = yrs::Doc::new();
        let their_sv = {
            use base64::Engine;
            use yrs::updates::encoder::Encode;
            use yrs::{ReadTxn, Transact};
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(theirs.transact().state_vector().encode_v1())
        };
        from_peer(socket, serde_json::json!({ "type": "yrs_sv", "sv": their_sv }));
        let sent = sent(&asked());
        assert_eq!(sent.len(), 2, "{:?}", sent);
        assert!(sent[0].starts_with(r#"{"type":"relay","data":{"type":"yrs_update","update":""#), "{}", sent[0]);
        assert_eq!(
            sent[1],
            format!(r#"{{"type":"relay","data":{{"type":"yrs_sv_reply","sv":"{}"}}}}"#, crdt::encode_state_vector())
        );

        // In answer to our own state vector they send theirs: only the update goes back.
        from_peer(socket, serde_json::json!({ "type": "yrs_sv_reply", "sv": their_sv }));
        let sent = super::tests::sent(&asked());
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains(r#""type":"yrs_update""#));

        // A state vector that cannot be read gets no update, and still our own.
        from_peer(socket, serde_json::json!({ "type": "yrs_sv", "sv": "!!" }));
        let sent = super::tests::sent(&asked());
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains(r#""type":"yrs_sv_reply""#));
        fresh();
    }

    #[test]
    fn an_update_from_the_opponent_is_applied_and_the_rooms_views_are_drawn_again() {
        fresh();
        let socket = with_the_opponent();

        // Build an update as the opponent's engine would.
        let before = crdt::encode_full_state();
        let update = crdt::add_alarm(NOW, 6, "from them", "green");
        crdt::reset_doc();
        crdt::restore_from_state(&before).unwrap();
        crdt::take_outbox();
        assert!(crdt::get_alarms().is_empty());

        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert_eq!(crdt::get_alarms().len(), 1);
        let redraws = page::take_redraws();
        assert!(redraws.contains(&"/api/game/turns"));
        assert!(redraws.contains(&"/api/room/fists"));
        fresh();
    }

    /// An update as the opponent's engine would make it (a timer of
    /// theirs), and their state vector before any of ours reached them.
    fn made_by_the_opponent() -> (String, String) {
        use base64::Engine;
        use yrs::updates::encoder::Encode;
        use yrs::{ReadTxn, Transact};
        let ours = crdt::encode_full_state();
        crdt::reset_doc();
        let update = crdt::add_alarm(NOW, 6, "theirs", "green");
        crdt::reset_doc();
        crdt::restore_from_state(&ours).unwrap();
        crdt::take_outbox();
        let empty = yrs::Doc::new();
        let sv = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(empty.transact().state_vector().encode_v1());
        (update, sv)
    }

    fn timer_names() -> Vec<String> {
        crdt::get_alarms().into_iter().map(|alarm| alarm.name).collect()
    }

    #[test]
    fn nothing_of_the_rooms_state_is_exchanged_with_an_opponent_on_another_protocol() {
        // One phone has taken an update of the site and the other has not.
        // Each is told so, and each keeps its own timers as they are.
        fresh();
        let socket = in_a_room();
        crdt::add_alarm(NOW, 4, "ours", "blue");
        crdt::take_outbox();
        let (update, their_sv) = made_by_the_opponent();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL - 1, "playerId": THEM }));
        asked();
        page::take_redraws();

        // Their state vectors get no answer and their update is not applied.
        from_peer(socket, serde_json::json!({ "type": "yrs_sv", "sv": their_sv }));
        from_peer(socket, serde_json::json!({ "type": "yrs_sv_reply", "sv": their_sv }));
        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert!(sent(&asked()).is_empty());
        assert_eq!(timer_names(), ["ours"]);

        // What this player changes is not sent either.
        crdt::queue_for_peer(crdt::tick_alarms(NOW + 1_000.0));
        send_updates();
        assert!(sent(&asked()).is_empty());
        assert!(crdt::take_outbox().is_empty(), "and is not kept to be sent later as it is");

        // They take the update and come back. Their hello is in our protocol
        // now, and the handshake gives each what the other has, the change
        // made meanwhile included.
        relay(socket, serde_json::json!({ "type": "peer_away" }));
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        asked();
        from_peer(socket, serde_json::json!({ "type": "yrs_sv", "sv": their_sv }));
        let answer = sent(&asked());
        assert_eq!(answer.len(), 2, "{:?}", answer);
        assert!(answer[0].contains(r#""type":"yrs_update""#), "{}", answer[0]);
        assert!(answer[1].contains(r#""type":"yrs_sv_reply""#), "{}", answer[1]);
        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert_eq!(timer_names(), ["ours", "theirs"]);
        assert_eq!(crdt::get_alarms()[0].remaining, 3, "our change made meanwhile is still ours");
        assert_eq!(face(), Face::Connected);
        fresh();
    }

    #[test]
    fn nothing_is_taken_from_an_opponent_until_they_have_said_hello_each_time_they_appear() {
        fresh();
        let socket = in_a_room();
        let (update, _) = made_by_the_opponent();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));

        // No hello yet (a copy of the site from before there was one never
        // sends it).
        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert!(timer_names().is_empty());

        // A hello, then they go and someone appears again: that one has to
        // say hello too.
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        relay(socket, serde_json::json!({ "type": "peer_away" }));
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert!(timer_names().is_empty());
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "yrs_update", "update": update }));
        assert_eq!(timer_names(), ["theirs"]);
        fresh();
    }

    #[test]
    fn a_change_this_player_makes_is_sent_before_the_handover_and_only_once() {
        fresh();
        let socket = with_the_opponent();
        let update = crdt::add_alarm(NOW, 2, "ours", "red");
        crdt::queue_for_peer(update.clone());

        send_updates();
        assert_eq!(
            asked(),
            [format!(r#"send room:{} {{"type":"relay","data":{{"type":"yrs_update","update":"{}"}}}}"#, socket, update)]
        );
        send_updates();
        assert!(asked().is_empty());
        fresh();
    }

    #[test]
    fn a_change_made_while_the_connection_is_down_is_not_sent_and_not_kept() {
        fresh();
        let socket = with_the_opponent();
        socket_event(socket, SocketEvent::Close);
        asked();
        crdt::queue_for_peer("abc".to_string());
        send_updates();
        assert!(sent(&asked()).is_empty());
        assert!(crdt::take_outbox().is_empty(), "the exchange of state on reconnecting makes up for it");
        fresh();
    }

    #[test]
    fn the_opponent_going_away_keeps_the_room_and_shows_waiting() {
        fresh();
        let socket = with_the_opponent();
        assert_eq!(face(), Face::Connected);
        for gone in ["peer_away", "peer_left"] {
            relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
            from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
            relay(socket, serde_json::json!({ "type": gone }));
            assert_eq!(dialog().stage, Stage::Waiting, "{}", gone);
            assert!(room::in_room());
            wait_over("face");
            assert_eq!(face(), Face::WaitingForOpponent);
        }
        fresh();
    }

    // ── Losing the connection and getting it back ──────────────────

    #[test]
    fn a_lost_socket_in_a_room_is_retried_as_a_return_that_starts_nothing_afresh() {
        fresh();
        let socket = with_the_opponent();
        crdt::add_alarm(NOW, 4, "shared", "blue");
        crdt::take_outbox();
        let me = room::with_room(|r| r.player_id.clone());

        socket_event(socket, SocketEvent::Close);
        assert_eq!(link_state(), State::Waiting);
        assert_eq!(dialog().stage, Stage::Waiting, "the opponent cannot reach us");
        let asked_now = asked();
        assert!(asked_now.iter().any(|line| line.starts_with("set room:retry ")), "{:?}", asked_now);

        link_timer(Timer::Retry);
        let (second, url) = opened(&asked());
        assert_ne!(second, socket);
        assert_eq!(url, format!("wss://signal.kipukas.us/ws?action=rejoin&code=ABCD&name=Table&player={}", me));

        socket_event(second, SocketEvent::Open);
        relay(second, serde_json::json!({ "type": "room_joined", "code": "ABCD", "name": "Table", "v": 2 }));
        relay(second, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        assert_eq!(dialog().stage, Stage::Connected);
        // The room's document was not started afresh.
        assert_eq!(crdt::get_alarms().len(), 1);
        room::with_room(|r| {
            assert_eq!(r.player_id, me);
            assert_eq!(r.peer_id, THEM);
        });
        fresh();
    }

    #[test]
    fn a_relay_that_recreates_the_room_on_return_does_not_start_it_afresh_either() {
        fresh();
        let socket = with_the_opponent();
        crdt::add_alarm(NOW, 4, "shared", "blue");
        socket_event(socket, SocketEvent::Close);
        link_timer(Timer::Retry);
        let (second, _) = opened(&asked());
        socket_event(second, SocketEvent::Open);
        relay(second, serde_json::json!({ "type": "room_created", "code": "ABCD", "name": "Table", "v": 2 }));
        assert_eq!(crdt::get_alarms().len(), 1);
        room::with_room(|r| assert_eq!(r.peer_id, THEM));
        fresh();
    }

    #[test]
    fn a_page_load_in_a_room_reconnects_as_a_return() {
        fresh();
        let stored = serde_json::json!({
            store::ROOM: serde_json::json!({
                "v": 2, "code": "ABCD", "name": "Table", "creator": false,
                "playerId": "aaaaaaaaaaaaaaaaaaaaaaaa", "peerId": THEM, "ts": crate::clock::now_ms(),
            }).to_string(),
        });
        assert_eq!(store::handle_start_post(&stored.to_string()), "ok");
        let (_, url) = opened(&asked());
        assert_eq!(
            url,
            "wss://signal.kipukas.us/ws?action=rejoin&code=ABCD&name=Table&player=aaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(dialog().stage, Stage::Waiting);
        fresh();
    }

    #[test]
    fn coming_back_into_view_retries_at_once_or_checks_the_socket() {
        fresh();
        let socket = with_the_opponent();

        // Open, and the worker finds it open: a ping.
        came_back(&[format!("room:{}", socket)]);
        assert_eq!(sent(&asked()), [r#"{"type":"ping"}"#]);

        // Open as far as the engine knows, but the worker finds it is not.
        came_back(&[]);
        assert_eq!(link_state(), State::Waiting);
        asked();

        // Waiting: retry now.
        came_back(&[]);
        assert_eq!(link_state(), State::Connecting);
        fresh();
    }

    // ── Leaving, and being put out ─────────────────────────────────

    #[test]
    fn leaving_tells_the_relay_closes_the_socket_and_keeps_the_rooms_timers() {
        fresh();
        let socket = with_the_opponent();
        crdt::add_alarm(NOW, 4, "shared", "blue");
        crdt::take_outbox();

        leave();
        assert_eq!(
            asked().iter().filter(|line| !line.starts_with("clear ")).collect::<Vec<_>>(),
            [&format!(r#"send room:{} {{"type":"leave"}}"#, socket), &format!("close room:{}", socket)]
        );
        assert!(!room::in_room());
        assert_eq!(dialog().stage, Stage::Out);
        assert_eq!(face(), Face::Off);
        assert_eq!(player_doc::get_alarms().len(), 1, "the room's timer is now the player's own");
        fresh();
    }

    #[test]
    fn a_place_taken_while_away_puts_the_player_out_and_says_why() {
        fresh();
        let socket = with_the_opponent();
        socket_event(socket, SocketEvent::Close);
        link_timer(Timer::Retry);
        let (second, _) = opened(&asked());
        socket_event(second, SocketEvent::Open);
        relay(second, serde_json::json!({ "type": "error", "code": "full", "message": "Room is full" }));

        assert!(!room::in_room());
        assert_eq!(face(), Face::Expired);
        let shown = dialog();
        assert_eq!(shown.stage, Stage::Out);
        assert!(shown.notice.starts_with("Your place in this room was taken"), "{}", shown.notice);
        assert_eq!(link_state(), State::Idle);

        dismiss_expired();
        assert_eq!(face(), Face::Off);
        fresh();
    }

    #[test]
    fn another_refusal_on_return_shows_what_the_relay_said_and_a_new_room_clears_it() {
        fresh();
        let socket = with_the_opponent();
        relay(socket, serde_json::json!({ "type": "error", "message": "Room closed" }));
        assert_eq!(face(), Face::Expired);
        assert_eq!(dialog().notice, "Room closed");

        create("Again");
        assert_eq!(face(), Face::Off, "the pill gives way to what the connection does next");
        assert_eq!(dialog().notice, "");
        fresh();
    }

    #[test]
    fn another_tab_taking_the_room_quiets_this_one_for_good() {
        fresh();
        let socket = with_the_opponent();
        go_quiet();
        assert_eq!(face(), Face::Moved);
        assert_eq!(link_state(), State::Idle);
        assert!(asked().contains(&format!("close room:{}", socket)));
        assert!(room::in_room(), "the room is not left: the other tab is in it");

        came_back(&[]);
        assert_eq!(link_state(), State::Idle, "it does not reconnect");
        assert!(asked().is_empty());
        fresh();
    }

    #[test]
    fn the_relay_saying_we_connected_elsewhere_quiets_this_tab_too() {
        fresh();
        let socket = with_the_opponent();
        relay(socket, serde_json::json!({ "type": "superseded" }));
        assert_eq!(face(), Face::Moved);
        assert_eq!(link_state(), State::Idle);
        fresh();
    }

    // ── The pill ───────────────────────────────────────────────────

    #[test]
    fn a_wait_shows_on_the_pill_only_if_it_lasts() {
        fresh();
        create("Table");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        assert_eq!(face(), Face::Off, "not in a room yet");
        relay(socket, serde_json::json!({ "type": "room_created", "code": "ABCD", "name": "Table" }));

        // Waiting for the opponent is wanted, and due in a moment.
        assert_eq!(face(), Face::Off);
        assert!(asked().contains(&"set room:face 1500".to_string()));

        // The opponent arrives first: the wait is called off, the pill goes green.
        page::take_redraws();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": THEM }));
        from_peer(socket, serde_json::json!({ "type": "hello", "proto": PROTOCOL, "playerId": THEM }));
        assert_eq!(face(), Face::Connected);
        assert!(asked().contains(&"clear room:face".to_string()));
        assert!(page::take_redraws().contains(&"/api/room/pill"));

        // They go away and stay away: shown once the wait is over.
        relay(socket, serde_json::json!({ "type": "peer_away" }));
        assert_eq!(face(), Face::Connected);
        wait_over("face");
        assert_eq!(face(), Face::WaitingForOpponent);
        assert!(page::take_redraws().contains(&"/api/room/pill"));

        // What is waited for changes while one wait is already shown: the
        // new one is given the same moment before it shows.
        socket_event(socket, SocketEvent::Close);
        assert_eq!(face(), Face::WaitingForOpponent);
        wait_over("face");
        assert_eq!(face(), Face::Reconnecting);
        fresh();
    }

    #[test]
    fn the_dialog_is_drawn_again_when_what_it_shows_changes_and_not_otherwise() {
        fresh();
        show();
        page::take_redraws();

        create("Table");
        assert!(page::take_redraws().contains(&"/api/room/status"), "connecting");
        let (socket, _) = opened(&asked());
        socket_event(socket, SocketEvent::Open);
        assert!(!page::take_redraws().contains(&"/api/room/status"), "still connecting");
        relay(socket, serde_json::json!({ "type": "room_created", "code": "ABCD", "name": "Table" }));
        assert!(page::take_redraws().contains(&"/api/room/status"), "in the room");
        link_timer(Timer::Ping);
        assert!(!page::take_redraws().contains(&"/api/room/status"), "a ping changes nothing");
        fresh();
    }

    #[test]
    fn the_dialog_and_pill_draw_what_the_connection_holds() {
        fresh();
        assert_eq!(room_status::handle_pill_get(""), "");
        let socket = with_the_opponent();
        let pill = room_status::handle_pill_get("");
        assert!(pill.contains(r#"data-tone="ok""#), "{}", pill);
        let status = room_status::handle_status_get("");
        assert!(status.contains("Connected"));
        assert!(status.contains("ABCD"));
        let _ = socket;
        fresh();
    }
}
