//! What the engine asks of its worker: sockets and waits.
//!
//! The engine cannot open a socket or wait for anything: it only answers
//! requests. Its worker (kipukas-worker.js) can do both, and does them as
//! the engine asks and decides nothing. What the engine wants is queued
//! here and taken by the worker after every post, with the rest of the
//! handover (`crate::page`):
//!
//! ```text
//! {"open": "room:3", "url": "wss://…"}     open a socket, to be known by that name
//! {"send": "room:3", "text": "…"}          send on it, if it is open
//! {"close": "room:3"}                      close it; nothing more need be said of it
//! {"set": "room:retry", "ms": 1000}        report that name in that long,
//!                                          in place of any wait of the same name
//! {"clear": "room:retry"}                  forget that wait
//! ```
//!
//! The worker reports back with POST /api/wire (JSON):
//!
//! ```text
//! {"socket": "room:3", "event": "open"}
//! {"socket": "room:3", "event": "message", "data": "…"}
//! {"socket": "room:3", "event": "error"}
//! {"socket": "room:3", "event": "close"}
//! {"timer": "room:retry"}
//! ```
//!
//! A name starts with whose it is: `room` for the room's connection
//! (`game::connection`).

use std::cell::RefCell;

use serde::Deserialize;

use crate::game::connection;
use crate::link::{Ask, Timer};

thread_local! {
    static ASKED: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

/// Whose a socket or a wait is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Room,
}

impl Channel {
    fn name(self) -> &'static str {
        match self {
            Channel::Room => "room",
        }
    }

    fn named(name: &str) -> Option<Channel> {
        match name {
            "room" => Some(Channel::Room),
            _ => None,
        }
    }
}

fn timer_name(timer: Timer) -> &'static str {
    match timer {
        Timer::Retry => "retry",
        Timer::Connect => "connect",
        Timer::Ping => "ping",
        Timer::Pong => "pong",
    }
}

fn link_timer(name: &str) -> Option<Timer> {
    match name {
        "retry" => Some(Timer::Retry),
        "connect" => Some(Timer::Connect),
        "ping" => Some(Timer::Ping),
        "pong" => Some(Timer::Pong),
        _ => None,
    }
}

fn ask(what: serde_json::Value) {
    ASKED.with(|asked| asked.borrow_mut().push(what));
}

/// Pass on what a link asks for.
pub fn pass(channel: Channel, asked: Ask) {
    let socket = |id: u32| format!("{}:{}", channel.name(), id);
    match asked {
        Ask::Open { socket: id, url } => ask(serde_json::json!({ "open": socket(id), "url": url })),
        Ask::Close { socket: id } => ask(serde_json::json!({ "close": socket(id) })),
        Ask::Send { socket: id, text } => ask(serde_json::json!({ "send": socket(id), "text": text })),
        Ask::Set { timer, ms } => set(channel, timer_name(timer), ms),
        Ask::Clear { timer } => clear(channel, timer_name(timer)),
    }
}

/// Ask to be told `name` in `ms` milliseconds, in place of any earlier wait
/// of the same name. For waits that are not a link's own.
pub fn set(channel: Channel, name: &str, ms: u32) {
    ask(serde_json::json!({ "set": format!("{}:{}", channel.name(), name), "ms": ms }));
}

/// Forget a wait.
pub fn clear(channel: Channel, name: &str) {
    ask(serde_json::json!({ "clear": format!("{}:{}", channel.name(), name) }));
}

/// Take what has been asked of the worker. Taken once.
pub fn take() -> Vec<serde_json::Value> {
    ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()))
}

/// What a socket did.
#[derive(Debug, Clone, PartialEq)]
pub enum SocketEvent {
    Open,
    Message(String),
    Error,
    Close,
}

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    socket: Option<String>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    data: String,
    #[serde(default)]
    timer: Option<String>,
}

/// POST /api/wire — the worker reports what a socket did, or that a wait is
/// over. A report that makes no sense is ignored.
pub fn handle_wire_post(body: &str) -> String {
    let Ok(report) = serde_json::from_str::<Report>(body) else {
        return "ok".to_string();
    };

    if let Some(name) = report.socket {
        let Some((channel, id)) = split(&name) else {
            return "ok".to_string();
        };
        let Ok(id) = id.parse::<u32>() else {
            return "ok".to_string();
        };
        let event = match report.event.as_str() {
            "open" => SocketEvent::Open,
            "message" => SocketEvent::Message(report.data),
            "error" => SocketEvent::Error,
            "close" => SocketEvent::Close,
            _ => return "ok".to_string(),
        };
        match channel {
            Channel::Room => connection::socket_event(id, event),
        }
    } else if let Some(name) = report.timer {
        let Some((channel, wait)) = split(&name) else {
            return "ok".to_string();
        };
        match (channel, link_timer(wait)) {
            (Channel::Room, Some(timer)) => connection::link_timer(timer),
            (Channel::Room, None) => connection::wait_over(wait),
        }
    }
    "ok".to_string()
}

/// `room:3` → the room's, `3`.
fn split(name: &str) -> Option<(Channel, &str)> {
    let (channel, rest) = name.split_once(':')?;
    Some((Channel::named(channel)?, rest))
}

/// Whether the worker finds the socket `id` of `channel` open, given the
/// names of those it does (which it sends along with what the page reports).
pub fn is_alive(alive: &[String], channel: Channel, id: Option<u32>) -> bool {
    id.is_some_and(|id| alive.iter().any(|name| *name == format!("{}:{}", channel.name(), id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_link_asks_for_is_named_for_whose_it_is() {
        take();
        pass(Channel::Room, Ask::Open { socket: 3, url: "wss://relay/ws".to_string() });
        pass(Channel::Room, Ask::Send { socket: 3, text: "hi".to_string() });
        pass(Channel::Room, Ask::Close { socket: 1 });
        pass(Channel::Room, Ask::Set { timer: Timer::Retry, ms: 1000 });
        pass(Channel::Room, Ask::Clear { timer: Timer::Pong });
        set(Channel::Room, "hello", 3000);
        assert_eq!(
            take(),
            [
                serde_json::json!({ "open": "room:3", "url": "wss://relay/ws" }),
                serde_json::json!({ "send": "room:3", "text": "hi" }),
                serde_json::json!({ "close": "room:1" }),
                serde_json::json!({ "set": "room:retry", "ms": 1000 }),
                serde_json::json!({ "clear": "room:pong" }),
                serde_json::json!({ "set": "room:hello", "ms": 3000 }),
            ]
        );
        assert!(take().is_empty(), "taken once");
    }

    #[test]
    fn a_report_that_makes_no_sense_is_ignored() {
        for body in [
            "not json",
            "{}",
            r#"{"socket":"room"}"#,
            r#"{"socket":"room:x","event":"open"}"#,
            r#"{"socket":"elsewhere:1","event":"open"}"#,
            r#"{"socket":"room:1","event":"unheard-of"}"#,
            r#"{"timer":"nobody:retry"}"#,
            r#"{"timer":"retry"}"#,
        ] {
            assert_eq!(handle_wire_post(body), "ok", "{}", body);
        }
    }

    #[test]
    fn a_socket_is_alive_if_the_worker_names_it_among_those_open() {
        let alive = vec!["room:4".to_string(), "elsewhere:1".to_string()];
        assert!(is_alive(&alive, Channel::Room, Some(4)));
        assert!(!is_alive(&alive, Channel::Room, Some(1)));
        assert!(!is_alive(&alive, Channel::Room, None));
        assert!(!is_alive(&[], Channel::Room, Some(4)));
    }
}
