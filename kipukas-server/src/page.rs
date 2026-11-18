//! What the engine asks of the page.
//!
//! The engine only answers requests: it cannot reach the page. What it wants
//! done there is queued, and after every post the engine's worker takes what
//! is queued and hands it to the page (`handOver` in kipukas-worker.js):
//!
//! - updates of the room document for the other player, which the page sends
//!   because it holds the connection (`game::crdt::queue_for_peer`);
//! - views to draw again (`redraw`, below).

use std::cell::RefCell;

use crate::game::crdt;

thread_local! {
    static REDRAW: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Ask for the views of a route to be drawn again, whatever their query.
///
/// A tap in a view draws that view again by itself. This is for a change
/// that reaches a view some other way: the other player's move arriving, or
/// a lost round of fists marking damage on a card.
pub fn redraw(route: &'static str) {
    REDRAW.with(|queue| {
        let mut queue = queue.borrow_mut();
        if !queue.contains(&route) {
            queue.push(route);
        }
    });
}

/// Take the routes whose views are to be drawn again. They are taken once:
/// what is returned is no longer queued.
pub fn take_redraws() -> Vec<&'static str> {
    REDRAW.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
}

/// POST /api/handover — take everything the engine has queued for the page.
///
/// Returns JSON:
///   updates  room-document updates for the other player (base64), oldest first
///   redraw   routes whose views are to be drawn again
pub fn handle_handover_post(_body: &str) -> String {
    serde_json::json!({
        "updates": crdt::take_outbox(),
        "redraw": take_redraws(),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_is_queued_once_and_taken_once() {
        take_redraws();
        redraw("/api/card/panel");
        redraw("/api/room/fists");
        redraw("/api/card/panel");
        assert_eq!(take_redraws(), vec!["/api/card/panel", "/api/room/fists"]);
        assert!(take_redraws().is_empty());
    }

    #[test]
    fn the_handover_carries_both_queues_and_empties_them() {
        take_redraws();
        crdt::take_outbox();
        crdt::queue_for_peer("abc".to_string());
        redraw("/api/game/turns");

        let handed: serde_json::Value =
            serde_json::from_str(&handle_handover_post("")).unwrap();
        assert_eq!(handed["updates"], serde_json::json!(["abc"]));
        assert_eq!(handed["redraw"], serde_json::json!(["/api/game/turns"]));

        let again: serde_json::Value = serde_json::from_str(&handle_handover_post("")).unwrap();
        assert_eq!(again["updates"], serde_json::json!([]));
        assert_eq!(again["redraw"], serde_json::json!([]));
    }
}
