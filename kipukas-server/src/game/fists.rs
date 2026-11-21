//! Fists combat in the room document.
//!
//! Combat used to live only in each player's memory and travel as one-off
//! relay messages. A page change wiped one side, a message sent while the
//! other player was reconnecting was lost, and a round could wait for the
//! opponent forever. It now lives in the shared room document next to the
//! timers, so it is persisted with the document, restored on page load and
//! repaired by the same state-vector handshake on every reconnect.
//!
//! ## Doc structure
//!
//! ```text
//! yrs::Doc
//! └── "fists" (MapRef)
//!     ├── "<round>:<playerId>" → JSON string Entry
//!     └── ...
//! ```
//!
//! One entry per round per player, written only by that player:
//!
//! - **The current round** is the highest round number present. Only entries
//!   at that round count as this round's submissions.
//! - **A new round** is started by writing an empty entry at `round + 1`.
//!   That makes the other player's entry stale without touching it, so two
//!   players starting a new round at once, or one starting it while the other
//!   is still submitting, converge without either needing the other's
//!   permission.
//! - **Earlier rounds are kept.** A player who was away when a round finished
//!   still finds its outcome and can apply it (see [`unsettled`]) even if the
//!   opponent has already moved on to the next round.
//!
//! Everything here is a pure function over a `&Doc`, so tests can run two
//! documents and sync between them. The routes apply these to the room doc.

use serde::{Deserialize, Serialize};
use yrs::{Any, Doc, Map, MapRef, ReadTxn, Transact};

use crate::game::room::{CombatRole, FinalBlowsSubmission, FistsCombat, FistsSubmission};

/// Root map holding combat entries.
pub const FISTS: &str = "fists";

/// What one player has put into one round.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entry {
    /// Regular submission (role, card, keal means).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<FistsSubmission>,
    /// Final Blows submission (card whose keal means are all exhausted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fb: Option<FinalBlowsSubmission>,
    /// This player's answer to "Did you win?", stored as `attacker_won`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<bool>,
    /// This player has applied the round's outcome on their own device.
    /// Guards against applying it twice (reload, reconnect, both players
    /// answering).
    #[serde(default)]
    pub settled: bool,
    /// Applying it marked damage (or the wasted flag) on this player's card.
    #[serde(default)]
    pub marked: bool,
}

impl Entry {
    pub fn has_submission(&self) -> bool {
        self.sub.is_some() || self.fb.is_some()
    }

    /// The role this player fights in. A Final Blows card always defends.
    pub fn role(&self) -> Option<CombatRole> {
        self.sub
            .as_ref()
            .map(|s| s.role)
            .or_else(|| self.fb.as_ref().map(|_| CombatRole::Defending))
    }

    pub fn card(&self) -> &str {
        self.sub
            .as_ref()
            .map(|s| s.card.as_str())
            .or_else(|| self.fb.as_ref().map(|f| f.card.as_str()))
            .unwrap_or("")
    }

    pub fn keal_idx(&self) -> u8 {
        self.sub.as_ref().map_or(0, |s| s.keal_idx)
    }

    /// Which copy of the card this player fights with (`game::copies`).
    pub fn copy(&self) -> u8 {
        self.sub.as_ref().map(|s| s.copy).or_else(|| self.fb.as_ref().map(|f| f.copy)).unwrap_or(1)
    }
}

/// How a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    AttackerWon,
    DefenderWon,
    /// The two players answered "Did you win?" differently. Nothing is
    /// applied automatically; they settle it at the table.
    Disputed,
}

/// Which screen this player should see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing submitted by this player this round.
    Form,
    /// This player submitted; the opponent has not.
    Waiting,
    /// Both submitted: the matchup and "Did you win?".
    Result,
    /// The round has an outcome.
    Outcome,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::Form => "form",
            Phase::Waiting => "waiting",
            Phase::Result => "result",
            Phase::Outcome => "outcome",
        }
    }
}

/// One player's view of the current round.
#[derive(Debug, Clone, Default)]
pub struct View {
    pub round: u32,
    /// The round's submissions, from this player's side (local / remote).
    pub combat: FistsCombat,
    pub outcome: Option<Outcome>,
    /// This player's entry for the round, if any.
    pub mine: Option<Entry>,
}

impl View {
    pub fn phase(&self) -> Phase {
        if self.outcome.is_some() {
            Phase::Outcome
        } else if self.combat.is_complete() {
            Phase::Result
        } else if self.mine.as_ref().is_some_and(|e| e.has_submission()) {
            Phase::Waiting
        } else {
            Phase::Form
        }
    }

    /// The opponent has submitted and is waiting on this player.
    pub fn remote_ready(&self) -> bool {
        self.combat.remote.is_some() || self.combat.remote_final_blows.is_some()
    }
}

/// A finished round this player has not applied yet.
#[derive(Debug, Clone)]
pub struct Pending {
    pub round: u32,
    pub mine: Entry,
    pub outcome: Outcome,
    /// Either player stated Final Blows this round.
    pub final_blows: bool,
}

// ── Keys and ids ───────────────────────────────────────────────────

/// Whether `id` is acceptable as a player id. Ids arrive from the other
/// player, so only a conservative character set is accepted.
pub fn is_valid_player_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn key(round: u32, player: &str) -> String {
    format!("{}:{}", round, player)
}

fn parse_key(key: &str) -> Option<(u32, &str)> {
    let (round, player) = key.split_once(':')?;
    let round: u32 = round.parse().ok()?;
    if round == 0 || !is_valid_player_id(player) {
        return None;
    }
    Some((round, player))
}

// ── Reading ────────────────────────────────────────────────────────

/// Every well-formed entry as (round, player, entry). Anything malformed is
/// skipped rather than trusted: the other player writes to this map too.
fn read_all<T: ReadTxn>(map: &MapRef, txn: &T) -> Vec<(u32, String, Entry)> {
    let mut out = Vec::new();
    for (k, value) in map.iter(txn) {
        let Some((round, player)) = parse_key(k) else {
            continue;
        };
        if let yrs::Out::Any(Any::String(json)) = value {
            if let Ok(entry) = serde_json::from_str::<Entry>(&json) {
                out.push((round, player.to_string(), entry));
            }
        }
    }
    out
}

fn all_entries(doc: &Doc) -> Vec<(u32, String, Entry)> {
    let map = doc.get_or_insert_map(FISTS);
    let txn = doc.transact();
    read_all(&map, &txn)
}

fn current_round(all: &[(u32, String, Entry)]) -> u32 {
    all.iter().map(|(r, _, _)| *r).max().unwrap_or(0)
}

fn find<'a>(all: &'a [(u32, String, Entry)], round: u32, player: &str) -> Option<&'a Entry> {
    all.iter()
        .find(|(r, p, _)| *r == round && p == player)
        .map(|(_, _, e)| e)
}

/// The opponent's entry for a round.
///
/// With the opponent's id known, only their entry counts. Before it is known
/// (their hello has not arrived yet) the other entry at that round is used;
/// a room holds two players, so there is at most one. If a stale id lingers,
/// the smallest id is taken so both devices pick the same one.
fn find_peer<'a>(
    all: &'a [(u32, String, Entry)],
    round: u32,
    me: &str,
    peer: &str,
) -> Option<&'a Entry> {
    if !peer.is_empty() {
        return find(all, round, peer);
    }
    all.iter()
        .filter(|(r, p, _)| *r == round && p != me)
        .min_by(|a, b| a.1.cmp(&b.1))
        .map(|(_, _, e)| e)
}

fn outcome_of(mine: Option<&Entry>, theirs: Option<&Entry>) -> Option<Outcome> {
    let as_outcome = |won: bool| {
        if won {
            Outcome::AttackerWon
        } else {
            Outcome::DefenderWon
        }
    };
    match (mine.and_then(|e| e.outcome), theirs.and_then(|e| e.outcome)) {
        (Some(a), Some(b)) if a != b => Some(Outcome::Disputed),
        (Some(a), _) | (None, Some(a)) => Some(as_outcome(a)),
        (None, None) => None,
    }
}

/// This player's view of the current round.
pub fn view(doc: &Doc, me: &str, peer: &str) -> View {
    let all = all_entries(doc);
    let round = current_round(&all);
    let mine = find(&all, round, me);
    let theirs = find_peer(&all, round, me, peer);

    View {
        round,
        combat: FistsCombat {
            local: mine.and_then(|e| e.sub.clone()),
            remote: theirs.and_then(|e| e.sub.clone()),
            local_final_blows: mine.and_then(|e| e.fb.clone()),
            remote_final_blows: theirs.and_then(|e| e.fb.clone()),
        },
        outcome: outcome_of(mine, theirs),
        mine: mine.cloned(),
    }
}

/// Finished rounds this player took part in and has not applied yet, oldest
/// first. Looks at every round, not only the current one: the opponent may
/// have started the next round before this player's device saw the outcome.
pub fn unsettled(doc: &Doc, me: &str, peer: &str) -> Vec<Pending> {
    let all = all_entries(doc);
    let mut out: Vec<Pending> = all
        .iter()
        .filter(|(_, p, e)| p == me && e.has_submission() && !e.settled)
        .filter_map(|(round, _, mine)| {
            let theirs = find_peer(&all, *round, me, peer);
            let outcome = outcome_of(Some(mine), theirs)?;
            Some(Pending {
                round: *round,
                mine: mine.clone(),
                outcome,
                final_blows: mine.fb.is_some() || theirs.is_some_and(|e| e.fb.is_some()),
            })
        })
        .collect();
    out.sort_by_key(|p| p.round);
    out
}

// ── Writing ────────────────────────────────────────────────────────

fn write(doc: &Doc, round: u32, player: &str, entry: &Entry) {
    if !is_valid_player_id(player) || round == 0 {
        return;
    }
    let map = doc.get_or_insert_map(FISTS);
    let mut txn = doc.transact_mut();
    let json = serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string());
    map.insert(&mut txn, key(round, player), Any::from(json));
}

/// The round a submission made now belongs to: the current round, or the
/// next one if the current round already has an outcome.
fn submission_round(doc: &Doc, me: &str, peer: &str) -> u32 {
    let v = view(doc, me, peer);
    if v.round == 0 {
        1
    } else if v.outcome.is_some() {
        v.round + 1
    } else {
        v.round
    }
}

/// Lock in this player's choice for the round.
pub fn submit(doc: &Doc, me: &str, peer: &str, sub: FistsSubmission) {
    let round = submission_round(doc, me, peer);
    write(
        doc,
        round,
        me,
        &Entry {
            sub: Some(sub),
            ..Entry::default()
        },
    );
}

/// State Final Blows for the round.
pub fn final_blows(doc: &Doc, me: &str, peer: &str, fb: FinalBlowsSubmission) {
    let round = submission_round(doc, me, peer);
    write(
        doc,
        round,
        me,
        &Entry {
            fb: Some(fb),
            ..Entry::default()
        },
    );
}

/// Start a new round, abandoning whatever is in the current one for both
/// players. Returns false (and writes nothing) when there is nothing to
/// abandon.
pub fn new_round(doc: &Doc, me: &str) -> bool {
    let all = all_entries(doc);
    let round = current_round(&all);
    let anything = all
        .iter()
        .any(|(r, _, e)| *r == round && e.has_submission());
    if !anything {
        return false;
    }
    write(doc, round + 1, me, &Entry::default());
    true
}

/// Record this player's answer to "Did you win?" as `attacker_won`.
/// Returns false if this player has no submission in the current round.
pub fn report_outcome(doc: &Doc, me: &str, attacker_won: bool) -> bool {
    let all = all_entries(doc);
    let round = current_round(&all);
    let Some(mine) = find(&all, round, me) else {
        return false;
    };
    if !mine.has_submission() {
        return false;
    }
    let mut mine = mine.clone();
    mine.outcome = Some(attacker_won);
    write(doc, round, me, &mine);
    true
}

/// Record that this player has applied a round's outcome on their device.
pub fn mark_settled(doc: &Doc, round: u32, me: &str, marked: bool) {
    let all = all_entries(doc);
    let Some(mine) = find(&all, round, me) else {
        return;
    };
    let mut mine = mine.clone();
    mine.settled = true;
    mine.marked = marked;
    write(doc, round, me, &mine);
}

/// The fists dialog was closed. A round in progress stays in progress; a
/// finished round gives way to a new one, so reopening shows a fresh form.
/// Returns true if a new round was started.
pub fn close(doc: &Doc, me: &str, peer: &str) -> bool {
    if view(doc, me, peer).outcome.is_some() {
        new_round(doc, me)
    } else {
        false
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::updates::decoder::Decode;
    use yrs::updates::encoder::Encode;
    use yrs::{StateVector, Update};

    const A: &str = "player-a";
    const B: &str = "player-b";

    fn doc() -> Doc {
        let doc = Doc::new();
        doc.get_or_insert_map(FISTS);
        doc
    }

    /// One direction of the state-vector handshake.
    fn sync(from: &Doc, to: &Doc) {
        let sv = to.transact().state_vector();
        let diff = from.transact().encode_diff_v1(&sv);
        to.transact_mut()
            .apply_update(Update::decode_v1(&diff).unwrap())
            .unwrap();
    }

    fn sync_both(a: &Doc, b: &Doc) {
        sync(a, b);
        sync(b, a);
    }

    fn attack(card: &str) -> FistsSubmission {
        FistsSubmission {
            role: CombatRole::Attacking,
            card: card.to_string(),
            keal_idx: 1,
            affinity: None,
            copy: 1,
        }
    }

    fn defend(card: &str) -> FistsSubmission {
        FistsSubmission {
            role: CombatRole::Defending,
            card: card.to_string(),
            keal_idx: 2,
            affinity: None,
            copy: 1,
        }
    }

    /// A and B both submitted and synced.
    fn both_submitted() -> (Doc, Doc) {
        let (a, b) = (doc(), doc());
        submit(&a, A, B, attack("brox_the_defiant"));
        submit(&b, B, A, defend("liliel_healing_fairy"));
        sync_both(&a, &b);
        (a, b)
    }

    #[test]
    fn an_empty_document_shows_the_form() {
        let d = doc();
        let v = view(&d, A, B);
        assert_eq!(v.round, 0);
        assert_eq!(v.phase(), Phase::Form);
        assert!(!v.remote_ready());
        assert!(unsettled(&d, A, B).is_empty());
    }

    #[test]
    fn two_documents_converge_whichever_way_updates_travel() {
        for a_first in [true, false] {
            let (a, b) = (doc(), doc());
            submit(&a, A, B, attack("brox_the_defiant"));
            submit(&b, B, A, defend("liliel_healing_fairy"));
            if a_first {
                sync(&a, &b);
                sync(&b, &a);
            } else {
                sync(&b, &a);
                sync(&a, &b);
            }

            let (va, vb) = (view(&a, A, B), view(&b, B, A));
            assert_eq!(va.phase(), Phase::Result);
            assert_eq!(vb.phase(), Phase::Result);
            // A's local is B's remote and the other way round.
            assert_eq!(va.combat.local.as_ref().unwrap().card, "brox_the_defiant");
            assert_eq!(vb.combat.remote.as_ref().unwrap().card, "brox_the_defiant");
            assert_eq!(va.combat.remote.as_ref().unwrap().card, "liliel_healing_fairy");
            assert_eq!(vb.combat.local.as_ref().unwrap().card, "liliel_healing_fairy");
        }
    }

    #[test]
    fn a_submission_made_while_the_opponent_is_away_arrives_on_the_handshake() {
        let (a, b) = (doc(), doc());
        submit(&a, A, B, attack("brox_the_defiant"));
        // B never receives the live update (mid page-load). B submits too.
        submit(&b, B, A, defend("liliel_healing_fairy"));
        assert_eq!(view(&a, A, B).phase(), Phase::Waiting);
        assert_eq!(view(&b, B, A).phase(), Phase::Waiting);

        // B reconnects: the handshake brings both up to date.
        sync_both(&a, &b);
        assert_eq!(view(&a, A, B).phase(), Phase::Result);
        assert_eq!(view(&b, B, A).phase(), Phase::Result);
    }

    #[test]
    fn a_page_change_mid_round_keeps_the_round() {
        let a = doc();
        submit(&a, A, B, attack("brox_the_defiant"));

        // Page change: the document is persisted, then restored into a new
        // Doc (which has a new yrs client id).
        let saved = a.transact().encode_diff_v1(&StateVector::default());
        let restored = doc();
        restored
            .transact_mut()
            .apply_update(Update::decode_v1(&saved).unwrap())
            .unwrap();

        let v = view(&restored, A, B);
        assert_eq!(v.phase(), Phase::Waiting);
        assert_eq!(v.combat.local.as_ref().unwrap().card, "brox_the_defiant");

        // The opponent, who never left, submits; the restored side completes.
        let b = doc();
        sync_both(&restored, &b);
        submit(&b, B, A, defend("liliel_healing_fairy"));
        sync_both(&restored, &b);
        assert_eq!(view(&restored, A, B).phase(), Phase::Result);
    }

    #[test]
    fn both_players_changing_page_mid_round_keeps_the_round() {
        let (a, b) = both_submitted();
        let restore = |d: &Doc| {
            let saved = d.transact().encode_diff_v1(&StateVector::default());
            let fresh = doc();
            fresh
                .transact_mut()
                .apply_update(Update::decode_v1(&saved).unwrap())
                .unwrap();
            fresh
        };
        let (a2, b2) = (restore(&a), restore(&b));
        sync_both(&a2, &b2);
        assert_eq!(view(&a2, A, B).phase(), Phase::Result);
        assert_eq!(view(&b2, B, A).phase(), Phase::Result);
    }

    #[test]
    fn a_later_submission_by_the_same_player_replaces_the_earlier_one() {
        let a = doc();
        submit(&a, A, B, attack("brox_the_defiant"));
        // Restore into a new Doc, then submit again from there.
        let saved = a.transact().encode_diff_v1(&StateVector::default());
        let a2 = doc();
        a2.transact_mut()
            .apply_update(Update::decode_v1(&saved).unwrap())
            .unwrap();
        submit(&a2, A, B, defend("liliel_healing_fairy"));

        sync_both(&a, &a2);
        for d in [&a, &a2] {
            let v = view(d, A, B);
            assert_eq!(v.round, 1);
            assert_eq!(v.combat.local.as_ref().unwrap().card, "liliel_healing_fairy");
        }
    }

    #[test]
    fn a_new_round_racing_a_submission_leaves_both_on_an_empty_new_round() {
        let (a, b) = (doc(), doc());
        submit(&a, A, B, attack("brox_the_defiant"));
        sync_both(&a, &b);

        // At the same moment: A cancels the round, B locks in.
        assert!(new_round(&a, A));
        submit(&b, B, A, defend("liliel_healing_fairy"));
        sync_both(&a, &b);

        for (d, me, peer) in [(&a, A, B), (&b, B, A)] {
            let v = view(d, me, peer);
            assert_eq!(v.round, 2);
            assert_eq!(v.phase(), Phase::Form);
            assert!(!v.remote_ready());
        }
    }

    #[test]
    fn both_players_starting_a_new_round_at_once_converge() {
        let (a, b) = both_submitted();
        assert!(new_round(&a, A));
        assert!(new_round(&b, B));
        sync_both(&a, &b);
        for (d, me, peer) in [(&a, A, B), (&b, B, A)] {
            let v = view(d, me, peer);
            assert_eq!(v.round, 2);
            assert_eq!(v.phase(), Phase::Form);
        }
    }

    #[test]
    fn new_round_with_nothing_to_abandon_writes_nothing() {
        let d = doc();
        assert!(!new_round(&d, A));
        assert_eq!(view(&d, A, B).round, 0);

        submit(&d, A, B, attack("brox_the_defiant"));
        assert!(new_round(&d, A));
        assert!(!new_round(&d, A), "the new round is already empty");
        assert_eq!(view(&d, A, B).round, 2);
    }

    #[test]
    fn an_outcome_reported_by_one_player_reaches_both() {
        let (a, b) = both_submitted();
        assert!(report_outcome(&a, A, true));
        sync_both(&a, &b);
        assert_eq!(view(&a, A, B).outcome, Some(Outcome::AttackerWon));
        assert_eq!(view(&b, B, A).outcome, Some(Outcome::AttackerWon));
        assert_eq!(view(&b, B, A).phase(), Phase::Outcome);
    }

    #[test]
    fn both_players_reporting_the_same_outcome_is_one_outcome() {
        let (a, b) = both_submitted();
        report_outcome(&a, A, true);
        report_outcome(&b, B, true);
        sync_both(&a, &b);
        assert_eq!(view(&a, A, B).outcome, Some(Outcome::AttackerWon));
        assert_eq!(view(&b, B, A).outcome, Some(Outcome::AttackerWon));
    }

    #[test]
    fn conflicting_outcomes_are_disputed_on_both_devices() {
        let (a, b) = both_submitted();
        report_outcome(&a, A, true); // attacker says attacker won
        report_outcome(&b, B, false); // defender says defender won
        sync_both(&a, &b);
        assert_eq!(view(&a, A, B).outcome, Some(Outcome::Disputed));
        assert_eq!(view(&b, B, A).outcome, Some(Outcome::Disputed));
    }

    #[test]
    fn an_outcome_cannot_be_reported_without_a_submission() {
        let d = doc();
        assert!(!report_outcome(&d, A, true));
        submit(&d, B, A, defend("liliel_healing_fairy"));
        assert!(!report_outcome(&d, A, true), "A has not submitted this round");
    }

    #[test]
    fn a_round_is_settled_once_however_often_it_is_seen() {
        let (a, b) = both_submitted();
        report_outcome(&a, A, true);
        sync_both(&a, &b);

        // B (the defender) applies the round.
        let pending = unsettled(&b, B, A);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].round, 1);
        assert_eq!(pending[0].outcome, Outcome::AttackerWon);
        assert_eq!(pending[0].mine.role(), Some(CombatRole::Defending));
        mark_settled(&b, 1, B, true);
        assert!(unsettled(&b, B, A).is_empty());

        // B also answers, A's answer is delivered again, B reloads: still settled.
        report_outcome(&b, B, true);
        sync_both(&a, &b);
        sync_both(&a, &b);
        let saved = b.transact().encode_diff_v1(&StateVector::default());
        let b2 = doc();
        b2.transact_mut()
            .apply_update(Update::decode_v1(&saved).unwrap())
            .unwrap();
        assert!(unsettled(&b2, B, A).is_empty());
        let mine = view(&b2, B, A).mine.unwrap();
        assert!(mine.settled && mine.marked);
        assert_eq!(mine.outcome, Some(true), "settling keeps the reported outcome");
    }

    #[test]
    fn a_player_who_was_away_still_settles_a_round_the_opponent_moved_on_from() {
        let (a, b) = both_submitted();
        // B goes away. A reports the outcome, closes the dialog (new round)
        // and even submits for the next round.
        report_outcome(&a, A, true);
        assert!(close(&a, A, B));
        submit(&a, A, B, attack("brox_the_defiant"));

        // B returns and gets everything at once.
        sync_both(&a, &b);
        let vb = view(&b, B, A);
        assert_eq!(vb.round, 2);
        assert_eq!(vb.phase(), Phase::Form);
        assert!(vb.remote_ready());

        // Round 1 is still there for B to apply.
        let pending = unsettled(&b, B, A);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].round, 1);
        assert_eq!(pending[0].outcome, Outcome::AttackerWon);
        assert_eq!(pending[0].mine.card(), "liliel_healing_fairy");
        assert_eq!(pending[0].mine.keal_idx(), 2);
    }

    #[test]
    fn closing_the_dialog_mid_round_keeps_the_round() {
        let (a, b) = (doc(), doc());
        submit(&a, A, B, attack("brox_the_defiant"));
        sync_both(&a, &b);

        assert!(!close(&a, A, B), "waiting for the opponent");
        assert!(!close(&b, B, A), "opponent has not submitted yet");
        assert_eq!(view(&a, A, B).phase(), Phase::Waiting);

        submit(&b, B, A, defend("liliel_healing_fairy"));
        sync_both(&a, &b);
        assert!(!close(&a, A, B), "result shown, no outcome yet");
        assert_eq!(view(&a, A, B).phase(), Phase::Result);
    }

    #[test]
    fn closing_after_an_outcome_starts_a_new_round() {
        let (a, b) = both_submitted();
        report_outcome(&a, A, false);
        sync_both(&a, &b);
        assert!(close(&b, B, A));
        sync_both(&a, &b);
        for (d, me, peer) in [(&a, A, B), (&b, B, A)] {
            assert_eq!(view(d, me, peer).phase(), Phase::Form);
            assert_eq!(view(d, me, peer).round, 2);
        }
    }

    #[test]
    fn a_submission_after_an_outcome_opens_the_next_round() {
        let (a, b) = both_submitted();
        report_outcome(&a, A, true);
        sync_both(&a, &b);
        submit(&b, B, A, attack("liliel_healing_fairy"));
        sync_both(&a, &b);
        let va = view(&a, A, B);
        assert_eq!(va.round, 2);
        assert_eq!(va.phase(), Phase::Form);
        assert!(va.remote_ready());
    }

    #[test]
    fn final_blows_maps_to_the_final_blows_fields_and_defends() {
        let (a, b) = (doc(), doc());
        final_blows(
            &a,
            A,
            B,
            FinalBlowsSubmission {
                card: "brox_the_defiant".to_string(),
                affinity: None,
                copy: 1,
            },
        );
        submit(&b, B, A, attack("liliel_healing_fairy"));
        sync_both(&a, &b);

        let va = view(&a, A, B);
        assert_eq!(va.phase(), Phase::Result);
        assert!(va.combat.is_final_blows());
        assert_eq!(va.combat.local_final_blows.as_ref().unwrap().card, "brox_the_defiant");
        assert_eq!(va.mine.as_ref().unwrap().role(), Some(CombatRole::Defending));

        let vb = view(&b, B, A);
        assert_eq!(vb.combat.remote_final_blows.as_ref().unwrap().card, "brox_the_defiant");

        report_outcome(&b, B, true);
        sync_both(&a, &b);
        let pending = unsettled(&a, A, B);
        assert_eq!(pending.len(), 1);
        assert!(pending[0].final_blows);
    }

    #[test]
    fn entries_from_a_player_who_is_no_longer_the_opponent_are_ignored() {
        let (a, b) = (doc(), doc());
        // A stale entry from a previous opponent at the same round number.
        submit(&a, "old-opponent", A, defend("liliel_healing_fairy"));
        submit(&a, A, B, attack("brox_the_defiant"));
        sync_both(&a, &b);

        // With the real opponent known, the stale entry does not complete the round.
        assert_eq!(view(&a, A, B).phase(), Phase::Waiting);
        assert!(!view(&a, A, B).remote_ready());
    }

    #[test]
    fn before_the_opponent_is_known_the_other_entry_is_used() {
        let (a, b) = both_submitted();
        assert_eq!(view(&a, A, "").phase(), Phase::Result);
        assert_eq!(view(&b, B, "").phase(), Phase::Result);
    }

    #[test]
    fn malformed_keys_and_values_are_ignored() {
        let d = doc();
        {
            let map = d.get_or_insert_map(FISTS);
            let mut txn = d.transact_mut();
            map.insert(&mut txn, "9:x');alert(1);('", Any::from("{}".to_string()));
            map.insert(&mut txn, "notakey", Any::from("{}".to_string()));
            map.insert(&mut txn, "0:player-b", Any::from("{}".to_string()));
            map.insert(&mut txn, "7:player-b", Any::from("not json".to_string()));
            map.insert(&mut txn, "8:player-b", Any::from(3.0));
        }
        let v = view(&d, A, B);
        assert_eq!(v.round, 0);
        assert_eq!(v.phase(), Phase::Form);
    }

    #[test]
    fn player_ids_are_validated() {
        assert!(is_valid_player_id("0f3a9c-ab_12"));
        assert!(!is_valid_player_id(""));
        assert!(!is_valid_player_id("a:b"));
        assert!(!is_valid_player_id("a b"));
        assert!(!is_valid_player_id(&"a".repeat(65)));

        // An invalid id writes nothing.
        let d = doc();
        submit(&d, "bad id", B, attack("brox_the_defiant"));
        assert_eq!(view(&d, "bad id", B).round, 0);
    }

    #[test]
    fn the_state_vector_encoding_round_trips() {
        // Guards the helper used by the tests themselves.
        let d = doc();
        submit(&d, A, B, attack("brox_the_defiant"));
        let sv = d.transact().state_vector().encode_v1();
        assert!(StateVector::decode_v1(&sv).is_ok());
    }
}
