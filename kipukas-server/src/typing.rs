/// Type matchup calculator — port of typing.js
///
/// Computes attack die modifiers based on archetype and motive interactions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archetype {
    Cenozoic,
    Decrepit,
    Angelic,
    Brutal,
    Arboreal,
    Astral,
    Telekinetic,
    Glitch,
    Magic,
    Endothermic,
    Avian,
    Mechanical,
    Algorithmic,
    Energetic,
    Entropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motive {
    Spirit,
    Possessor,
    Conscience,
    Survival,
    Duty,
    Sacrifice,
    Passion,
    Service,
    Satisfaction,
}

/// Rows = defender archetype, Columns = attacker archetype.
/// `TYPE_CHART[def][atk]` gives the modifier for that matchup.
const TYPE_CHART: [[i8; 15]; 15] = [
    // Cenozoic defender
    [0, 3, 1, 1, -1, 2, 2, -1, -1, 2, -3, -1, -2, 1, -3],
    // Decrepit defender
    [-3, 0, 1, 2, 1, -1, -3, -2, 3, 1, 2, -1, -1, -1, 2],
    // Angelic defender
    [-1, -1, -3, 3, 2, -3, -2, 1, 1, 1, -1, 2, 1, -1, -1],
    // Brutal defender
    [-1, -2, -3, 3, 2, -1, -2, 1, 2, -1, -1, 2, 1, 1, 1],
    // Arboreal defender
    [1, -1, -2, -2, 0, -3, -1, -2, 2, 1, 3, 1, -1, 3, 1],
    // Astral defender
    [-2, 1, 3, 1, 3, 0, -1, 1, -1, -3, -2, -1, 2, -2, 1],
    // Telekinetic defender
    [-2, 3, 2, 2, 1, 1, 0, -3, -3, -1, -1, -1, 1, -1, 2],
    // Glitch defender
    [1, 2, -1, -1, 2, -1, 3, 0, -3, -3, 2, 1, -2, 1, -1],
    // Magic defender
    [1, -3, -1, -2, -2, 1, 3, 3, 0, -1, 2, 1, 1, -2, -1],
    // Endothermic defender
    [-2, -1, -1, 1, -1, 3, 1, 3, 1, 0, 1, 2, -2, -3, -2],
    // Avian defender
    [3, -2, 1, 1, -3, 2, 1, -2, -2, -1, 0, 3, -1, 1, -1],
    // Mechanical defender
    [1, 1, -2, -2, -1, 1, 1, -1, -1, -2, -3, 0, 3, 2, 3],
    // Algorithmic defender
    [2, 1, -1, -1, 1, -2, -1, 2, -1, 2, 1, -3, 0, 3, -3],
    // Energetic defender
    [-1, 1, 1, -1, -3, 2, 1, -1, 2, 3, -1, -2, -3, 0, 2],
    // Entropic defender
    [3, -2, 1, -1, -1, -1, -2, 1, 1, 2, 1, -3, 3, -2, 0],
];

/// Motive attack matrix.
/// Rows = attacker motive (indexed by MOTIVE_ATK_ORDER), value at column = 10 if match, 0 otherwise.
/// Column order: Spirit, Possessor, Conscience, Survival, Duty, Sacrifice, Passion, Service, Satisfaction
///
/// In the original JS, each motive's attack vector has 10 at its own index and 0 elsewhere.
/// This means the motive matchup only contributes when atk_motive's column matches def_motive's index.

/// Defense index order for motives (maps motive → column index in the attack vectors)
fn motive_def_index(m: Motive) -> usize {
    match m {
        Motive::Possessor => 0,
        Motive::Conscience => 1,
        Motive::Spirit => 2,
        Motive::Duty => 3,
        Motive::Sacrifice => 4,
        Motive::Passion => 5,
        Motive::Service => 6,
        Motive::Satisfaction => 7,
        Motive::Survival => 8,
    }
}

/// Attack index order for motives (maps motive → row in the 9×9 identity-like matrix)
fn motive_atk_index(m: Motive) -> usize {
    match m {
        Motive::Spirit => 0,
        Motive::Possessor => 1,
        Motive::Conscience => 2,
        Motive::Survival => 3,
        Motive::Duty => 4,
        Motive::Sacrifice => 5,
        Motive::Passion => 6,
        Motive::Service => 7,
        Motive::Satisfaction => 8,
    }
}

/// Preservation categories
fn is_societal(m: Motive) -> bool {
    matches!(m, Motive::Spirit | Motive::Duty | Motive::Service)
}

fn is_self_preservation(m: Motive) -> bool {
    matches!(m, Motive::Possessor | Motive::Survival | Motive::Satisfaction)
}

fn is_support_preservation(m: Motive) -> bool {
    matches!(m, Motive::Conscience | Motive::Sacrifice | Motive::Passion)
}

pub struct MatchupResult {
    pub modifier: i32,
    pub societal_mod: Option<&'static str>,
    pub self_mod: Option<&'static str>,
    pub support_mod: Option<&'static str>,
}

impl MatchupResult {
    /// Format as a display string matching the original JS output.
    pub fn to_display_string(&self) -> String {
        let mut result = self.modifier.to_string();
        if let Some(s) = self.societal_mod {
            result.push_str(s);
        }
        if let Some(s) = self.self_mod {
            result.push_str(s);
        }
        if let Some(s) = self.support_mod {
            result.push_str(s);
        }
        result
    }
}

/// What a Final Blow adds to the attacker's roll.
///
/// A Final Blow is rolled as a KEAL means bout is, with the cards' own dice,
/// between the two cards' archetypal genes: TYPE_CHART\[def_genetic\]\[atk_genetic\],
/// plus 10 if the motives interact (the attacker's motive attack index equals
/// the defender's motive defense index).
///
/// Until 2026-11 a Final Blow was a flat D20 for each player, and the chart's
/// value was doubled to count on that scale. The rule changed and the
/// doubling went with it.
pub fn final_blow_modifier(
    atk_genetic: Option<Archetype>,
    def_genetic: Option<Archetype>,
    atk_motive: Option<Motive>,
    def_motive: Option<Motive>,
) -> i32 {
    let mut bonus: i32 = 0;

    // The archetypal genes' matchup
    if let (Some(atk), Some(def)) = (atk_genetic, def_genetic) {
        bonus += TYPE_CHART[def as usize][atk as usize] as i32;
    }

    // +10 if motives interact
    if let (Some(atk_mot), Some(def_mot)) = (atk_motive, def_motive) {
        if motive_atk_index(atk_mot) == motive_def_index(def_mot) {
            bonus += 10;
        }
    }

    bonus
}

/// Check whether two motives interact (attacker's motive attack index matches
/// defender's motive defense index).
pub fn motives_interact(atk_motive: Option<Motive>, def_motive: Option<Motive>) -> bool {
    match (atk_motive, def_motive) {
        (Some(a), Some(d)) => motive_atk_index(a) == motive_def_index(d),
        _ => false,
    }
}

pub fn type_matchup(
    attackers: &[Archetype],
    defenders: &[Archetype],
    atk_motive: Option<Motive>,
    def_motive: Option<Motive>,
) -> MatchupResult {
    let mut modifier: i32 = 0;

    // Sum archetype matchups: for each attacker × defender pair
    for &atk in attackers {
        for &def in defenders {
            modifier += TYPE_CHART[def as usize][atk as usize] as i32;
        }
    }

    let mut societal_mod = None;
    let mut self_mod = None;
    let mut support_mod = None;

    // Motive matchup (only if both are provided)
    if let (Some(atk_mot), Some(def_mot)) = (atk_motive, def_motive) {
        // The motive matrix is essentially identity × 10:
        // atk row i has 10 at column i, 0 elsewhere.
        // So the motive contribution is 10 if atk_index == def_index, else 0.
        let atk_idx = motive_atk_index(atk_mot);
        let def_idx = motive_def_index(def_mot);
        if atk_idx == def_idx {
            modifier += 10;
        }

        // Preservation effects (the attacker's motive for societal, the
        // defender's for self and support), in the rules' words. A first
        // win nullified is not "2 of 3": the benefit is the one side's only,
        // and the other still needs a single win.
        if is_societal(atk_mot) {
            societal_mod = Some("\nDefender's first win is nullified");
        }
        if is_self_preservation(def_mot) {
            self_mod = Some("\nDefender may attempt a retreat before the Final Blow");
        }
        if is_support_preservation(def_mot) {
            support_mod = Some("\nAttacker's first win is nullified");
        }
    }

    MatchupResult {
        modifier,
        societal_mod,
        self_mod,
        support_mod,
    }
}

/// Parse an archetype name (case-insensitive) into the enum.
pub fn parse_archetype(s: &str) -> Option<Archetype> {
    match s.trim().to_lowercase().as_str() {
        "cenozoic" => Some(Archetype::Cenozoic),
        "decrepit" => Some(Archetype::Decrepit),
        "angelic" => Some(Archetype::Angelic),
        "brutal" => Some(Archetype::Brutal),
        "arboreal" => Some(Archetype::Arboreal),
        "astral" => Some(Archetype::Astral),
        "telekinetic" => Some(Archetype::Telekinetic),
        "glitch" => Some(Archetype::Glitch),
        "magic" => Some(Archetype::Magic),
        "endothermic" => Some(Archetype::Endothermic),
        "avian" => Some(Archetype::Avian),
        "mechanical" => Some(Archetype::Mechanical),
        "algorithmic" => Some(Archetype::Algorithmic),
        "energetic" => Some(Archetype::Energetic),
        "entropic" => Some(Archetype::Entropic),
        _ => None,
    }
}

/// Parse a motive name (case-insensitive) into the enum.
pub fn parse_motive(s: &str) -> Option<Motive> {
    match s.trim().to_lowercase().as_str() {
        "spirit" => Some(Motive::Spirit),
        "possessor" => Some(Motive::Possessor),
        "conscience" => Some(Motive::Conscience),
        "survival" => Some(Motive::Survival),
        "duty" => Some(Motive::Duty),
        "sacrifice" => Some(Motive::Sacrifice),
        "passion" => Some(Motive::Passion),
        "service" => Some(Motive::Service),
        "satisfaction" => Some(Motive::Satisfaction),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cenozoic_vs_cenozoic() {
        let result = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Cenozoic],
            None,
            None,
        );
        assert_eq!(result.modifier, 0);
    }

    #[test]
    fn cenozoic_vs_decrepit() {
        // Cenozoic attacks Decrepit: TABLE[Decrepit=1][Cenozoic=0] = -3
        let result = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Decrepit],
            None,
            None,
        );
        assert_eq!(result.modifier, -3);
    }

    #[test]
    fn brutal_magic_vs_avian() {
        // Two attackers vs one defender
        // Brutal atk Avian: TABLE[Avian=10][Brutal=3] = 1
        // Magic atk Avian: TABLE[Avian=10][Magic=8] = -2
        // Total: 1 + (-2) = -1
        let result = type_matchup(
            &[Archetype::Brutal, Archetype::Magic],
            &[Archetype::Avian],
            None,
            None,
        );
        assert_eq!(result.modifier, -1);
    }

    #[test]
    fn motive_match_adds_10() {
        // Spirit atk index=0, Spirit def index=2 — these don't match
        let result = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Cenozoic],
            Some(Motive::Spirit),
            Some(Motive::Spirit),
        );
        // Spirit atk_idx=0, Spirit def_idx=2 → no match → 0 + societal mod
        assert_eq!(result.modifier, 0);
        assert!(result.societal_mod.is_some()); // Spirit is societal

        // Possessor vs Possessor: atk_idx=1, def_idx=0 → no match
        let result2 = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Cenozoic],
            Some(Motive::Possessor),
            Some(Motive::Possessor),
        );
        assert_eq!(result2.modifier, 0);
    }

    #[test]
    fn preservation_modifiers() {
        // Societal attacker (Spirit) → the defender's first win is nullified
        let result = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Cenozoic],
            Some(Motive::Spirit),
            Some(Motive::Survival),
        );
        assert!(result.societal_mod.is_some());
        assert!(result.self_mod.is_some()); // Survival is self-preservation

        // Support defender (Conscience) → the attacker's first win is nullified
        let result2 = type_matchup(
            &[Archetype::Cenozoic],
            &[Archetype::Cenozoic],
            Some(Motive::Passion),
            Some(Motive::Conscience),
        );
        assert!(result2.support_mod.is_some());
    }

    #[test]
    fn display_string_format() {
        // Entropic attacks Cenozoic: TABLE[Cenozoic=0][Entropic=14] = -3
        let result = type_matchup(
            &[Archetype::Entropic],
            &[Archetype::Cenozoic],
            None,
            None,
        );
        assert_eq!(result.modifier, -3);
        assert_eq!(result.to_display_string(), "-3");
    }

    #[test]
    fn parse_archetype_works() {
        assert_eq!(parse_archetype("Brutal"), Some(Archetype::Brutal));
        assert_eq!(parse_archetype("AVIAN"), Some(Archetype::Avian));
        assert_eq!(parse_archetype("unknown"), None);
    }

    #[test]
    fn parse_motive_works() {
        assert_eq!(parse_motive("Spirit"), Some(Motive::Spirit));
        assert_eq!(parse_motive("DUTY"), Some(Motive::Duty));
        assert_eq!(parse_motive("unknown"), None);
    }

    #[test]
    fn final_blow_modifier_genetic_only() {
        // Brutal attacks Angelic: TABLE[Angelic=2][Brutal=3] = 3, as it is
        let bonus = final_blow_modifier(
            Some(Archetype::Brutal),
            Some(Archetype::Angelic),
            None,
            None,
        );
        assert_eq!(bonus, 3);
    }

    #[test]
    fn final_blow_modifier_with_motive_interaction() {
        // Cenozoic vs Cenozoic: TYPE_CHART[0][0] = 0
        // Motives that interact add +10
        // Need to find a pair where atk_idx == def_idx
        // Spirit atk_idx=0, Possessor def_idx=0 → match → +10
        let bonus = final_blow_modifier(
            Some(Archetype::Cenozoic),
            Some(Archetype::Cenozoic),
            Some(Motive::Spirit),
            Some(Motive::Possessor),
        );
        assert_eq!(bonus, 10);
    }

    #[test]
    fn final_blow_modifier_genetic_plus_motive() {
        // Entropic attacks Cenozoic: TABLE[Cenozoic=0][Entropic=14] = -3
        // Spirit vs Possessor: interact → +10
        // Total: -3 + 10 = 7
        let bonus = final_blow_modifier(
            Some(Archetype::Entropic),
            Some(Archetype::Cenozoic),
            Some(Motive::Spirit),
            Some(Motive::Possessor),
        );
        assert_eq!(bonus, 7);
    }

    #[test]
    fn final_blow_modifier_no_genetic_disposition() {
        let bonus = final_blow_modifier(None, None, Some(Motive::Spirit), Some(Motive::Possessor));
        assert_eq!(bonus, 10); // only motive interaction
    }

    #[test]
    fn motives_interact_true() {
        assert!(motives_interact(Some(Motive::Spirit), Some(Motive::Possessor)));
    }

    #[test]
    fn motives_interact_false() {
        assert!(!motives_interact(Some(Motive::Spirit), Some(Motive::Spirit)));
    }

    #[test]
    fn motives_interact_none() {
        assert!(!motives_interact(None, Some(Motive::Spirit)));
        assert!(!motives_interact(Some(Motive::Spirit), None));
    }
}
