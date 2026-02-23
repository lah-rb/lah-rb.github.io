//! Room (global) state — shared state for multiplayer sessions.
//!
//! Separate from the local `GameState` which tracks per-user damage/turns.
//! Room state is synchronized across connected peers via WebRTC data channel.
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
}

/// The fists combat state shared between both players.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FistsCombat {
    /// Local player's submission (set via POST /api/room/fists)
    pub local: Option<FistsSubmission>,
    /// Remote player's submission (set via POST /api/room/fists/sync)
    pub remote: Option<FistsSubmission>,
    /// Local player's final-blows state (card slug when fully exhausted)
    pub final_blows_local: Option<String>,
    /// Remote player's final-blows state (card slug when fully exhausted)
    pub final_blows_remote: Option<String>,
}

impl FistsCombat {
    /// Both players have submitted their choices.
    pub fn is_complete(&self) -> bool {
        self.local.is_some() && self.remote.is_some()
    }

    /// Clear for a new round.
    pub fn reset(&mut self) {
        self.local = None;
        self.remote = None;
        self.final_blows_local = None;
        self.final_blows_remote = None;
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
}

/// Room-level shared state for multiplayer sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoomState {
    /// Room code (e.g., "ABCD")
    pub room_code: String,
    /// Human-readable room name chosen by the creator
    pub room_name: String,
    /// Whether a peer is currently connected via WebRTC
    pub connected: bool,
    /// Active fists combat session (if any)
    pub fists: FistsCombat,
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

/// Reset room state to disconnected defaults.
pub fn reset_room() {
    replace_room(RoomState::default());
}

/// Export room state as JSON (for WebRTC sync).
pub fn export_room_json() -> String {
    with_room(|room| serde_json::to_string(room).unwrap_or_else(|_| "{}".to_string()))
}

/// Export just the fists combat state as JSON (for WebRTC data channel).
pub fn export_fists_json() -> String {
    with_room(|room| {
        if let Some(ref local) = room.fists.local {
            serde_json::to_string(local).unwrap_or_else(|_| "{}".to_string())
        } else {
            "null".to_string()
        }
    })
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
        });
        assert!(!combat.is_complete());

        combat.remote = Some(FistsSubmission {
            role: CombatRole::Defending,
            card: "liliel_healing_fairy".to_string(),
            keal_idx: 1,
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
            }),
            remote: Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
            }),
            final_blows_local: None,
            final_blows_remote: None,
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
            }),
            remote: Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "test2".to_string(),
                keal_idx: 1,
            }),
            final_blows_local: None,
            final_blows_remote: None,
        };
        combat.reset();
        assert!(combat.local.is_none());
        assert!(combat.remote.is_none());
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
            });
        });

        let json = export_room_json();
        assert!(json.contains("ABCD"));
        assert!(json.contains("Test Room"));
        assert!(json.contains("brox_the_defiant"));

        let restored: RoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.room_code, "ABCD");
        assert!(restored.connected);
        assert!(restored.fists.local.is_some());

        reset();
    }

    #[test]
    fn export_fists_json_when_none() {
        reset();
        let json = export_fists_json();
        assert_eq!(json, "null");
        reset();
    }

    #[test]
    fn export_fists_json_when_submitted() {
        reset();
        with_room_mut(|r| {
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "test_card".to_string(),
                keal_idx: 2,
            });
        });
        let json = export_fists_json();
        assert!(json.contains("test_card"));
        assert!(json.contains("Defending"));
        reset();
    }
}
