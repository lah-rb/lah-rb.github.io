//! Room state — what this player's engine knows about the multiplayer room it
//! is in: the room code and name, who this player and the opponent are, and
//! whether the opponent is currently reachable.
//!
//! Shared game state (timers, fists combat) does not live here. It lives in
//! the room document (`game::crdt`, `game::fists`), which is what is synced
//! between the two players and persisted across page loads. `RoomState.fists`
//! is only a cache of the current round for the result renderers.
//!
//! Uses its own `thread_local!` so it doesn't pollute localStorage persistence
//! of local game state.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;

/// Combat role in a fists matchup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatRole {
    Attacking,
    Defending,
}

/// A single player's fists combat submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FistsSubmission {
    /// Attacking or Defending
    pub role: CombatRole,
    /// Card slug (e.g., "brox_the_defiant")
    pub card: String,
    /// Which keal means to use (1-based index into the card's keal_means array)
    pub keal_idx: u8,
    /// The archetype this player had declared an affinity for when they
    /// locked in, if any. It goes with the submission so that both players'
    /// screens give the card the same roll bonus (`control::roll_bonus`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity: Option<String>,
}

/// Final Blows submission — used when a player's keal means are all exhausted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalBlowsSubmission {
    /// Card slug
    pub card: String,
    /// The archetype this player had declared an affinity for, as in
    /// [`FistsSubmission`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity: Option<String>,
}

/// One round of fists combat as this player sees it: their own submission
/// ("local") and the opponent's ("remote"). Derived from the room document
/// by `game::fists::view`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FistsCombat {
    /// This player's submission
    pub local: Option<FistsSubmission>,
    /// The opponent's submission
    pub remote: Option<FistsSubmission>,
    /// Local player's Final Blows submission (when keal means exhausted)
    pub local_final_blows: Option<FinalBlowsSubmission>,
    /// Remote player's Final Blows submission
    pub remote_final_blows: Option<FinalBlowsSubmission>,
}

impl FistsCombat {
    /// Both players have submitted their choices (regular fists or Final Blows).
    pub fn is_complete(&self) -> bool {
        let local_done = self.local.is_some() || self.local_final_blows.is_some();
        let remote_done = self.remote.is_some() || self.remote_final_blows.is_some();
        local_done && remote_done
    }

    /// Check if this is a Final Blows combat (at least one player has exhausted keal means).
    pub fn is_final_blows(&self) -> bool {
        self.local_final_blows.is_some() || self.remote_final_blows.is_some()
    }

    /// Clear for a new round.
    pub fn reset(&mut self) {
        self.local = None;
        self.remote = None;
        self.local_final_blows = None;
        self.remote_final_blows = None;
    }

    /// Get the attacker submission (from whichever player chose Attacking).
    pub fn attacker(&self) -> Option<&FistsSubmission> {
        if let Some(ref local) = self.local {
            if local.role == CombatRole::Attacking {
                return Some(local);
            }
        }
        if let Some(ref remote) = self.remote {
            if remote.role == CombatRole::Attacking {
                return Some(remote);
            }
        }
        None
    }

    /// Get the defender submission (from whichever player chose Defending).
    pub fn defender(&self) -> Option<&FistsSubmission> {
        if let Some(ref local) = self.local {
            if local.role == CombatRole::Defending {
                return Some(local);
            }
        }
        if let Some(ref remote) = self.remote {
            if remote.role == CombatRole::Defending {
                return Some(remote);
            }
        }
        None
    }

    /// Check if both players chose the same role.
    /// Returns `Some(role)` if there is a conflict, `None` if roles are valid.
    pub fn has_role_conflict(&self) -> Option<CombatRole> {
        match (&self.local, &self.remote) {
            (Some(l), Some(r)) if l.role == r.role => Some(l.role),
            _ => None,
        }
    }

    /// Get the card slug for the local player (from either regular or Final Blows submission).
    pub fn local_card(&self) -> Option<&str> {
        self.local.as_ref().map(|s| s.card.as_str())
            .or_else(|| self.local_final_blows.as_ref().map(|s| s.card.as_str()))
    }

    /// Get the card slug for the remote player (from either regular or Final Blows submission).
    pub fn remote_card(&self) -> Option<&str> {
        self.remote.as_ref().map(|s| s.card.as_str())
            .or_else(|| self.remote_final_blows.as_ref().map(|s| s.card.as_str()))
    }

    /// The archetype the local player had declared when they submitted.
    pub fn local_affinity(&self) -> Option<&str> {
        let regular = self.local.as_ref().and_then(|s| s.affinity.as_deref());
        regular.or_else(|| self.local_final_blows.as_ref().and_then(|s| s.affinity.as_deref()))
    }

    /// The archetype the remote player had declared when they submitted.
    pub fn remote_affinity(&self) -> Option<&str> {
        let regular = self.remote.as_ref().and_then(|s| s.affinity.as_deref());
        regular.or_else(|| self.remote_final_blows.as_ref().and_then(|s| s.affinity.as_deref()))
    }
}

/// What this player has picked in the fists form and not yet locked in.
///
/// It is this engine's alone: the opponent learns of a choice when it is
/// locked in. It is kept here and not on the page so that the form can be
/// drawn again at any moment (the opponent locking in draws it again) and
/// still show the picks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FistsChoice {
    /// The card the picks are for. Picks made for another card do not count.
    pub card: String,
    pub role: Option<CombatRole>,
    /// Which keal means (1-based, as in `FistsSubmission`).
    pub keal_idx: Option<u8>,
    /// Why the last attempt to lock in was refused, for the form to say.
    /// Empty when it was not.
    pub refusal: String,
}

/// Room-level shared state for multiplayer sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoomState {
    /// Room code (e.g., "ABCD")
    pub room_code: String,
    /// Human-readable room name chosen by the creator
    pub room_name: String,
    /// Whether a peer is currently connected through the relay
    pub connected: bool,
    /// Whether this player created the room (and did not join it).
    #[serde(default)]
    pub creator: bool,
    /// This player's id in the room (chosen by the page when the room is
    /// created or joined). Keys this player's entries in the room document.
    #[serde(default)]
    pub player_id: String,
    /// The opponent's id, once their hello has arrived. Empty until then.
    #[serde(default)]
    pub peer_id: String,
    /// The current round's submissions as this player sees them. A cache of
    /// the room document (see `game::fists`), refreshed whenever it changes;
    /// the result renderers read it.
    pub fists: FistsCombat,
    /// This player's picks in the fists form, not yet locked in.
    #[serde(default)]
    pub choice: FistsChoice,
}

thread_local! {
    static ROOM: RefCell<RoomState> = RefCell::new(RoomState::default());
}

/// Execute a closure with read access to the room state.
pub fn with_room<F, R>(f: F) -> R
where
    F: FnOnce(&RoomState) -> R,
{
    ROOM.with(|r| f(&r.borrow()))
}

/// Execute a closure with mutable access to the room state.
pub fn with_room_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut RoomState) -> R,
{
    ROOM.with(|r| f(&mut r.borrow_mut()))
}

/// Replace the entire room state.
pub fn replace_room(new_state: RoomState) {
    ROOM.with(|r| {
        *r.borrow_mut() = new_state;
    });
}

/// Check if a peer is currently connected in the room.
pub fn is_peer_connected() -> bool {
    with_room(|r| r.connected)
}

/// Check whether this player is in a room, with or without the peer present.
///
/// Shared state (the room document) is used whenever this is true. The peer
/// being away — mid page-load, phone asleep, brief network loss — must not
/// drop the player back to solo mode: edits made meanwhile go to the shared
/// document and reach the peer on the next sync handshake.
pub fn in_room() -> bool {
    with_room(|r| !r.room_code.is_empty())
}

/// Reset room state to disconnected defaults.
pub fn reset_room() {
    replace_room(RoomState::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        reset_room();
    }

    #[test]
    fn default_room_is_disconnected() {
        reset();
        with_room(|r| {
            assert!(!r.connected);
            assert!(r.room_code.is_empty());
            assert!(r.room_name.is_empty());
            assert!(r.fists.local.is_none());
            assert!(r.fists.remote.is_none());
        });
    }

    #[test]
    fn fists_combat_complete_when_both_submitted() {
        let mut combat = FistsCombat::default();
        assert!(!combat.is_complete());

        combat.local = Some(FistsSubmission {
            role: CombatRole::Attacking,
            card: "brox_the_defiant".to_string(),
            keal_idx: 1,
            affinity: None,
        });
        assert!(!combat.is_complete());

        combat.remote = Some(FistsSubmission {
            role: CombatRole::Defending,
            card: "liliel_healing_fairy".to_string(),
            keal_idx: 1,
            affinity: None,
        });
        assert!(combat.is_complete());
    }

    #[test]
    fn fists_attacker_defender_lookup() {
        let combat = FistsCombat {
            local: Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "brox_the_defiant".to_string(),
                keal_idx: 2,
                affinity: None,
            }),
            remote: Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
                affinity: None,
            }),
            local_final_blows: None,
            remote_final_blows: None,
        };

        let atk = combat.attacker().unwrap();
        assert_eq!(atk.card, "brox_the_defiant");
        assert_eq!(atk.keal_idx, 2);

        let def = combat.defender().unwrap();
        assert_eq!(def.card, "liliel_healing_fairy");
        assert_eq!(def.keal_idx, 1);
    }

    #[test]
    fn fists_reset_clears_both() {
        let mut combat = FistsCombat {
            local: Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "test".to_string(),
                keal_idx: 1,
                affinity: None,
            }),
            remote: Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "test2".to_string(),
                keal_idx: 1,
                affinity: None,
            }),
            local_final_blows: None,
            remote_final_blows: None,
        };
        combat.reset();
        assert!(combat.local.is_none());
        assert!(combat.remote.is_none());
        assert!(combat.local_final_blows.is_none());
        assert!(combat.remote_final_blows.is_none());
    }

    #[test]
    fn room_state_roundtrip_json() {
        reset();
        with_room_mut(|r| {
            r.room_code = "ABCD".to_string();
            r.room_name = "Test Room".to_string();
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "brox_the_defiant".to_string(),
                keal_idx: 1,
                affinity: None,
            });
        });

        let json = with_room(|room| serde_json::to_string(room).unwrap());
        assert!(json.contains("ABCD"));
        assert!(json.contains("Test Room"));
        assert!(json.contains("brox_the_defiant"));

        let restored: RoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.room_code, "ABCD");
        assert!(restored.connected);
        assert!(restored.fists.local.is_some());

        reset();
    }
}
