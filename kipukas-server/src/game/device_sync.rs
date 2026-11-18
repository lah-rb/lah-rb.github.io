//! Pairing two of a player's devices, to merge the player's own state
//! (damage, timers, affinity, loyalty) between them.
//!
//! The two devices meet in a room on the same relay the game's rooms use:
//! one creates it and shows its code, the other joins with the code. Before
//! anything is exchanged each proves to the other that it was given the same
//! passphrase (a keyed hash of the room code, `game::crypto`). Then each
//! sends what the other lacks, the same exchange of state vectors and
//! updates that keeps a room's document in step, here on the player's
//! document. While the pairing lasts, a change on either device is sent to
//! the other as it is made.
//!
//! The dialog is an engine-owned view (`routes::player`, `/api/player/sync`)
//! of what is held here. The socket is a [`Link`] of its own, which does
//! not retry: a pairing that drops is started again by hand.
//!
//! ## What travels
//!
//! To the relay, as for a game room: `action` (`create` or `join`), `code`,
//! an empty `name`, and `player`, an id made for this page load.
//!
//! Between the devices, as the relay's `data`: `sync_auth` (with `mac`),
//! then `player_sv`, `player_sv_reply` (with `sv`) and `player_update`
//! (with `update`).

use std::cell::RefCell;

use crate::game::{connection, crypto, player_doc};
use crate::link::{Link, Owner, State, Timer, Told};
use crate::page;
use crate::wire::{self, Channel, SocketEvent};

/// The shortest passphrase taken.
const SHORTEST_PASSPHRASE: usize = 4;

/// What the dialog is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// The passphrase, and the choice of creating a room or joining one.
    #[default]
    Menu,
    /// The other device's room code is asked for.
    Join,
    /// In a room (or on the way to one), with its code once it is known.
    Paired,
}

/// What this device asked the relay for.
#[derive(Debug, Clone, PartialEq)]
enum Want {
    Create,
    Join { code: String },
}

/// What the dialog shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dialog {
    pub screen: Screen,
    /// What was typed, so that it is still in its field after a refusal or
    /// a change of screen.
    pub passphrase: String,
    /// The room's code, once the relay has given or accepted it.
    pub code: String,
    pub status: String,
    pub error: String,
    /// The two devices are exchanging state.
    pub syncing: bool,
    /// They have exchanged it.
    pub done: bool,
}

#[derive(Default)]
struct Sync {
    dialog: Dialog,
    /// Set until the relay answers; while it is, a lost socket is a failure
    /// to connect.
    want: Option<Want>,
    /// This device's id in the room: one per page load.
    player: String,
    peer_present: bool,
    /// The other device has proved it has the passphrase.
    authenticated: bool,
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
    let (want, player) = SYNC.with(|sync| {
        let sync = sync.borrow();
        (sync.want.clone(), sync.player.clone())
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
    wants.push(("name", String::new()));
    wants.push(("player", player));
    connection::relay_address_with(&wants)
}

fn send_to_peer(data: &str) {
    link(|link| link.send(&format!(r#"{{"type":"relay","data":{}}}"#, data)));
}

fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
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

// ── What the player asks for ───────────────────────────────────────

/// Keep the passphrase if it will do. Says why not otherwise.
fn take_passphrase(passphrase: &str) -> bool {
    with_sync(|sync| {
        sync.dialog.passphrase = passphrase.to_string();
        if passphrase.chars().count() < SHORTEST_PASSPHRASE {
            sync.dialog.error = "Passphrase must be at least 4 characters".to_string();
            false
        } else {
            sync.dialog.error.clear();
            true
        }
    })
}

/// Create a room for the other device to join.
pub fn create(passphrase: &str) {
    if take_passphrase(passphrase) {
        connect_for(Want::Create, "Creating sync room…");
    }
    show();
}

/// Go on to ask for the other device's room code.
pub fn ask_for_code(passphrase: &str) {
    if take_passphrase(passphrase) {
        with_sync(|sync| sync.dialog.screen = Screen::Join);
    }
    show();
}

/// Join the room the other device created.
pub fn join(code: &str) {
    let code = code.trim().to_uppercase();
    if code.chars().count() != 4 {
        with_sync(|sync| sync.dialog.error = "Enter a 4-character room code".to_string());
    } else {
        connect_for(Want::Join { code }, "Joining sync room…");
    }
    show();
}

/// Back to the first screen, keeping the passphrase.
pub fn back() {
    with_sync(|sync| {
        sync.dialog.screen = Screen::Menu;
        sync.dialog.error.clear();
    });
    show();
}

/// The dialog was closed, or the pairing called off: end it and forget
/// everything typed.
pub fn close() {
    disconnect();
    with_sync(|sync| sync.dialog = Dialog::default());
    show();
}

fn connect_for(want: Want, status: &str) {
    link(|link| link.stop());
    with_sync(|sync| {
        if sync.player.is_empty() {
            sync.player = format!("{:016x}{:08x}", fastrand::u64(..), fastrand::u32(..));
        }
        sync.want = Some(want);
        sync.peer_present = false;
        sync.authenticated = false;
        sync.dialog.screen = Screen::Paired;
        sync.dialog.code.clear();
        sync.dialog.status = status.to_string();
        sync.dialog.error.clear();
        sync.dialog.syncing = false;
        sync.dialog.done = false;
    });
    link(|link| link.start());
}

/// End the pairing here.
fn disconnect() {
    with_sync(|sync| {
        sync.want = None;
        sync.peer_present = false;
        sync.authenticated = false;
    });
    link(|link| link.stop());
}

/// Back to the first screen to say what went wrong.
fn fail(error: &str) {
    disconnect();
    with_sync(|sync| {
        sync.dialog.screen = Screen::Menu;
        sync.dialog.status.clear();
        sync.dialog.error = error.to_string();
        sync.dialog.syncing = false;
    });
}

// ── What the link tells ────────────────────────────────────────────

fn link_state_changed(next: State, prev: State) {
    if next != State::Idle {
        return;
    }
    // The relay was never reached, or never answered.
    if with_sync(|sync| sync.want.take()).is_some() {
        fail("Could not connect to signaling server");
        return;
    }
    // The socket of a pairing closed.
    if prev == State::Open {
        with_sync(|sync| {
            sync.peer_present = false;
            sync.authenticated = false;
            if sync.dialog.syncing {
                sync.dialog.status = "Disconnected during sync".to_string();
                sync.dialog.syncing = false;
            } else if sync.dialog.screen == Screen::Paired && !sync.dialog.done {
                sync.dialog.status = "Connection lost. Close and start again.".to_string();
            }
        });
    }
}

fn relay_said(message: &serde_json::Value) {
    let text = |name: &str| message[name].as_str().unwrap_or("");
    match text("type") {
        "room_created" => with_sync(|sync| {
            sync.want = None;
            sync.dialog.code = text("code").to_string();
            sync.dialog.status = "Room created! Share this code with your other device:".to_string();
        }),
        "room_joined" => with_sync(|sync| {
            sync.want = None;
            sync.dialog.code = text("code").to_string();
            sync.dialog.status = "Joined room. Waiting for authentication…".to_string();
        }),
        "peer_joined" => {
            with_sync(|sync| {
                sync.peer_present = true;
                sync.dialog.status = "Peer device connected. Authenticating…".to_string();
            });
            // Prove to the other device that this one has the passphrase.
            let (passphrase, code) =
                SYNC.with(|sync| (sync.borrow().dialog.passphrase.clone(), sync.borrow().dialog.code.clone()));
            if let Ok(mac) = crypto::sign_export(&passphrase, &code) {
                send_to_peer(&format!(r#"{{"type":"sync_auth","mac":{}}}"#, quoted(&mac)));
            }
        }
        "relay" => peer_said(&message["data"]),
        "peer_away" | "peer_left" => with_sync(|sync| {
            sync.peer_present = false;
            sync.authenticated = false;
            if !sync.dialog.done {
                sync.dialog.status = "Peer device disconnected.".to_string();
                sync.dialog.syncing = false;
            }
        }),
        "error" => {
            let said = text("message");
            fail(if said.is_empty() { "Connection error" } else { said });
        }
        _ => {}
    }
}

/// A message from the other device.
fn peer_said(data: &serde_json::Value) {
    let text = |name: &str| data[name].as_str().unwrap_or("");
    let kind = text("type");

    // Nothing of the player's state is exchanged with a device that has not
    // proved it has the passphrase.
    if kind != "sync_auth" && !SYNC.with(|sync| sync.borrow().authenticated) {
        return;
    }

    match kind {
        // The other device's proof.
        "sync_auth" => {
            let (passphrase, code) =
                SYNC.with(|sync| (sync.borrow().dialog.passphrase.clone(), sync.borrow().dialog.code.clone()));
            if crypto::verify_export(&passphrase, &code, text("mac")) == Ok(true) {
                with_sync(|sync| {
                    sync.authenticated = true;
                    sync.dialog.status = "Authenticated! Syncing player data…".to_string();
                    sync.dialog.syncing = true;
                });
                send_state_vector("player_sv");
            } else {
                fail("Wrong passphrase on the other device");
            }
        }
        // Their state vector: send what they lack, then our own state
        // vector so that they can send what we lack.
        "player_sv" => {
            send_what_they_lack(text("sv"));
            send_state_vector("player_sv_reply");
        }
        // Their state vector, in answer to ours.
        "player_sv_reply" => send_what_they_lack(text("sv")),
        // What we lacked.
        "player_update" => {
            if player_doc::apply_update(text("update")).is_ok() {
                with_sync(|sync| {
                    sync.dialog.status = "✓ Sync complete! Player data merged.".to_string();
                    sync.dialog.syncing = false;
                    sync.dialog.done = true;
                });
                // Whatever on the page shows the player's state shows the
                // merged state.
                for route in ["/api/card/panel", "/api/game/turns", "/api/player/affinity"] {
                    page::redraw(route);
                }
            }
        }
        _ => {}
    }
}

fn send_state_vector(kind: &str) {
    send_to_peer(&format!(r#"{{"type":{},"sv":{}}}"#, quoted(kind), quoted(&player_doc::encode_state_vector())));
}

fn send_what_they_lack(their_state_vector: &str) {
    if let Ok(diff) = player_doc::encode_diff(their_state_vector) {
        if !diff.is_empty() {
            send_to_peer(&format!(r#"{{"type":"player_update","update":{}}}"#, quoted(&diff)));
        }
    }
}

/// The player's state changed on this device. While a pairing lasts, the
/// other device is offered it: our state vector goes out, and the exchange
/// that follows brings both up to date.
pub fn player_changed() {
    let paired = SYNC.with(|sync| sync.borrow().authenticated && sync.borrow().peer_present);
    if paired {
        send_state_vector("player_sv");
    }
}

// ── What the page shows ────────────────────────────────────────────

/// What the dialog shows now.
pub fn dialog() -> Dialog {
    SYNC.with(|sync| sync.borrow().dialog.clone())
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
    use crate::game::{crdt, room};
    use crate::store;

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

    /// The socket the engine last asked for (its id and address), and what
    /// has been sent on the sync channel since this was last called.
    fn asked() -> (Option<(u32, String)>, Vec<String>) {
        let mut opened = None;
        let mut sent = Vec::new();
        for op in wire::take() {
            if let Some(name) = op["open"].as_str() {
                let id = name.strip_prefix("sync:").expect("a socket of the sync channel").parse().unwrap();
                opened = Some((id, op["url"].as_str().unwrap().to_string()));
            }
            if op["send"].as_str().is_some_and(|name| name.starts_with("sync:")) {
                sent.push(op["text"].as_str().unwrap().to_string());
            }
        }
        (opened, sent)
    }

    fn relay(socket: u32, message: serde_json::Value) {
        socket_event(socket, SocketEvent::Message(message.to_string()));
    }

    fn from_peer(socket: u32, data: serde_json::Value) {
        relay(socket, serde_json::json!({ "type": "relay", "data": data }));
    }

    /// The `data` of each message sent to the other device.
    fn data(sent: &[String]) -> Vec<serde_json::Value> {
        sent.iter().map(|text| serde_json::from_str::<serde_json::Value>(text).unwrap()["data"].clone()).collect()
    }

    /// Create a pairing room and have the relay answer. Returns the socket.
    fn in_a_room() -> u32 {
        create("correct horse");
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "room_created", "code": "ABCD", "v": 2 }));
        asked();
        socket
    }

    /// As `in_a_room`, with the other device there and proved.
    fn paired() -> u32 {
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        let mac = crypto::sign_export("correct horse", "ABCD").unwrap();
        from_peer(socket, serde_json::json!({ "type": "sync_auth", "mac": mac }));
        asked();
        socket
    }

    #[test]
    fn a_short_passphrase_is_refused_and_stays_in_its_field() {
        fresh();
        create("abc");
        let shown = dialog();
        assert_eq!(shown.screen, Screen::Menu);
        assert_eq!(shown.error, "Passphrase must be at least 4 characters");
        assert_eq!(shown.passphrase, "abc");
        assert!(asked().0.is_none(), "nothing was connected");

        ask_for_code("abc");
        assert_eq!(dialog().screen, Screen::Menu);
        fresh();
    }

    #[test]
    fn creating_asks_the_relay_for_a_room_with_an_id_of_this_page_load() {
        fresh();
        create("correct horse");
        let (_, url) = asked().0.unwrap();
        let player = with_sync(|sync| sync.player.clone());
        assert_eq!(url, format!("wss://signal.kipukas.us/ws?action=create&name=&player={}", player));
        assert_eq!(player.len(), 24);
        let shown = dialog();
        assert_eq!(shown.screen, Screen::Paired);
        assert_eq!(shown.status, "Creating sync room…");
        fresh();
    }

    #[test]
    fn joining_asks_for_the_code_first_and_sends_it_in_capitals() {
        fresh();
        ask_for_code("correct horse");
        assert_eq!(dialog().screen, Screen::Join);

        join("abc");
        assert_eq!(dialog().error, "Enter a 4-character room code");
        assert_eq!(dialog().screen, Screen::Join);

        join(" abcd ");
        let (_, url) = asked().0.unwrap();
        assert!(url.starts_with("wss://signal.kipukas.us/ws?action=join&code=ABCD&name=&player="), "{}", url);
        assert_eq!(dialog().status, "Joining sync room…");

        back();
        assert_eq!(dialog().screen, Screen::Menu);
        assert_eq!(dialog().passphrase, "correct horse");
        fresh();
    }

    #[test]
    fn the_relay_given_for_local_work_is_used_here_too() {
        fresh();
        connection::use_relay("ws://localhost:8792/ws?who=A");
        create("correct horse");
        assert!(asked().0.unwrap().1.starts_with("ws://localhost:8792/ws?who=A&action=create&name=&player="));
        fresh();
    }

    #[test]
    fn the_room_code_is_shown_once_the_relay_gives_it() {
        fresh();
        in_a_room();
        let shown = dialog();
        assert_eq!(shown.code, "ABCD");
        assert_eq!(shown.status, "Room created! Share this code with your other device:");
        fresh();
    }

    #[test]
    fn when_the_other_device_arrives_each_proves_it_has_the_passphrase() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        let sent = data(&asked().1);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["type"], "sync_auth");
        assert_eq!(sent[0]["mac"], crypto::sign_export("correct horse", "ABCD").unwrap());
        assert_eq!(dialog().status, "Peer device connected. Authenticating…");

        // Their proof is right: the exchange starts with our state vector.
        let mac = crypto::sign_export("correct horse", "ABCD").unwrap();
        from_peer(socket, serde_json::json!({ "type": "sync_auth", "mac": mac }));
        let sent = data(&asked().1);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["type"], "player_sv");
        assert_eq!(sent[0]["sv"], player_doc::encode_state_vector());
        assert!(dialog().syncing);
        fresh();
    }

    #[test]
    fn a_wrong_passphrase_on_the_other_device_ends_the_pairing() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        asked();
        let wrong = crypto::sign_export("another phrase", "ABCD").unwrap();
        from_peer(socket, serde_json::json!({ "type": "sync_auth", "mac": wrong }));

        let shown = dialog();
        assert_eq!(shown.screen, Screen::Menu);
        assert_eq!(shown.error, "Wrong passphrase on the other device");
        assert_eq!(LINK.with(|link| link.borrow().state()), State::Idle);
        assert!(asked().1.is_empty(), "nothing of the player's state was sent");
        fresh();
    }

    #[test]
    fn a_proof_made_for_another_room_is_no_proof() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        asked();
        let elsewhere = crypto::sign_export("correct horse", "WXYZ").unwrap();
        from_peer(socket, serde_json::json!({ "type": "sync_auth", "mac": elsewhere }));
        assert_eq!(dialog().error, "Wrong passphrase on the other device");
        from_peer(socket, serde_json::json!({ "type": "sync_auth" }));
        assert!(!dialog().syncing);
        fresh();
    }

    #[test]
    fn nothing_is_exchanged_with_a_device_that_has_not_proved_itself() {
        fresh();
        let socket = in_a_room();
        relay(socket, serde_json::json!({ "type": "peer_joined", "peerId": "other" }));
        asked();
        player_doc::add_alarm(3, "mine", "red");

        // It asks for our state, and offers its own, without a proof.
        from_peer(socket, serde_json::json!({ "type": "player_sv", "sv": "AA" }));
        from_peer(socket, serde_json::json!({ "type": "player_update", "update": "AAA" }));
        assert!(asked().1.is_empty());
        assert!(!dialog().done);
        fresh();
    }

    #[test]
    fn the_two_devices_each_send_what_the_other_lacks() {
        fresh();
        // The other device's state: one timer of its own.
        player_doc::add_alarm(5, "theirs", "blue");
        let their_state = player_doc::encode_full_state();
        player_doc::init_player_doc();
        let empty_sv = player_doc::encode_state_vector();

        player_doc::add_alarm(3, "ours", "red");
        let socket = paired();

        // Their state vector (they have nothing of ours): our update, then our own state vector.
        from_peer(socket, serde_json::json!({ "type": "player_sv", "sv": empty_sv }));
        let sent = data(&asked().1);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["type"], "player_update");
        assert_eq!(sent[1]["type"], "player_sv_reply");

        // What we lacked arrives: merged, said so, and the views drawn again.
        page::take_redraws();
        from_peer(socket, serde_json::json!({ "type": "player_update", "update": their_state }));
        let names: Vec<String> = player_doc::get_alarms().into_iter().map(|alarm| alarm.name).collect();
        assert!(names.contains(&"ours".to_string()) && names.contains(&"theirs".to_string()), "{:?}", names);
        let shown = dialog();
        assert!(shown.done && !shown.syncing);
        assert_eq!(shown.status, "✓ Sync complete! Player data merged.");
        let redraws = page::take_redraws();
        for route in ["/api/card/panel", "/api/game/turns", "/api/player/affinity", "/api/player/sync"] {
            assert!(redraws.contains(&route), "{} in {:?}", route, redraws);
        }

        // In answer to our state vector they send theirs: only an update goes back.
        from_peer(socket, serde_json::json!({ "type": "player_sv_reply", "sv": empty_sv }));
        let sent = data(&asked().1);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["type"], "player_update");
        fresh();
    }

    #[test]
    fn a_change_made_while_paired_is_offered_to_the_other_device() {
        fresh();
        player_changed();
        assert!(asked().1.is_empty(), "not paired: nothing is sent");

        paired();
        player_changed();
        let sent = data(&asked().1);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["type"], "player_sv");
        fresh();
    }

    #[test]
    fn a_relay_that_cannot_be_reached_or_refuses_is_said_on_the_first_screen() {
        fresh();
        create("correct horse");
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Error);
        socket_event(socket, SocketEvent::Close);
        let shown = dialog();
        assert_eq!(shown.screen, Screen::Menu);
        assert_eq!(shown.error, "Could not connect to signaling server");
        assert_eq!(shown.passphrase, "correct horse", "what was typed is kept");

        ask_for_code("correct horse");
        join("ZZZZ");
        let (socket, _) = asked().0.unwrap();
        socket_event(socket, SocketEvent::Open);
        relay(socket, serde_json::json!({ "type": "error", "code": "not_found", "message": "Room not found" }));
        let shown = dialog();
        assert_eq!(shown.screen, Screen::Menu);
        assert_eq!(shown.error, "Room not found");
        fresh();
    }

    #[test]
    fn the_other_device_leaving_or_the_socket_closing_is_said() {
        fresh();
        let socket = paired();
        assert!(dialog().syncing);
        relay(socket, serde_json::json!({ "type": "peer_left" }));
        assert_eq!(dialog().status, "Peer device disconnected.");
        assert!(!dialog().syncing);

        fresh();
        let socket = paired();
        socket_event(socket, SocketEvent::Close);
        assert_eq!(dialog().status, "Disconnected during sync");
        assert_eq!(LINK.with(|link| link.borrow().state()), State::Idle, "a pairing is not retried");

        // Before anything was being exchanged.
        fresh();
        let socket = in_a_room();
        socket_event(socket, SocketEvent::Close);
        assert_eq!(dialog().status, "Connection lost. Close and start again.");
        fresh();
    }

    #[test]
    fn closing_ends_the_pairing_and_forgets_what_was_typed() {
        fresh();
        let socket = paired();
        close();
        assert_eq!(dialog(), Dialog::default());
        let asked = wire::take();
        assert!(asked.contains(&serde_json::json!({ "close": format!("sync:{}", socket) })), "{:?}", asked);

        // Messages still on their way from the other device change nothing.
        from_peer(socket, serde_json::json!({ "type": "player_update", "update": "AAA" }));
        assert_eq!(dialog(), Dialog::default());
        fresh();
    }

    #[test]
    fn the_dialog_is_drawn_again_when_what_it_shows_changes_and_not_otherwise() {
        fresh();
        show();
        page::take_redraws();
        show();
        assert!(page::take_redraws().is_empty());
        create("abc");
        assert_eq!(page::take_redraws(), ["/api/player/sync"]);
        fresh();
    }
}
