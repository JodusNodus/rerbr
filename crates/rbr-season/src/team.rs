//! The player's team (ours): the player picks a team for its service crew, not for its car. The crew is the team's
//! engineer and mechanics (`Team::engineering`, `Team::mechanics`), which scale every repair time of the player's
//! service park the way the 1.02 season's player team does.

use rbr_formats::ai_ini::{Roster, Team};
use rbr_gameplay::car_damage::Rules;

use crate::service::Crew;

/// A team the player can sign with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamOffer {
    /// The team's index in the roster, what `EventConfig::team` holds.
    pub index: u8,
    pub name: String,
    /// The crew in words: its engineer, its mechanics and how fast it repairs against a standard crew.
    pub crew: String,
}

/// The crew of a team: its engineering skill and number of mechanics.
pub fn crew_of(team: &Team) -> Crew {
    Crew { a: team.engineering, b: team.mechanics }
}

/// The repair time factor of a crew under NGP's rules (the product of the park's two scales).
fn factor(crew: Crew) -> f32 {
    let (a, b) = crew.scales(Rules::Ngp);
    a * b
}

/// The crew in words.
pub fn describe(crew: Crew) -> String {
    let engineer = match crew.a {
        i32::MIN..=1 => "a rookie engineer",
        2 => "a modest engineer",
        3 => "a solid engineer",
        4 => "a strong engineer",
        _ => "an expert engineer",
    };
    // Against the standard crew (the park's default): below 1 is faster.
    let rel = factor(crew) / factor(Crew::default());
    let pace = if (rel - 1.0).abs() < 0.03 {
        "repairs at the standard pace".to_owned()
    } else if rel < 1.0 {
        format!("repairs about {:.0} % faster than the standard crew", (1.0 - rel) * 100.0)
    } else {
        format!("repairs about {:.0} % slower than the standard crew", (rel - 1.0) * 100.0)
    };
    format!("{engineer} and {} mechanics, {pace}", crew.b.max(0))
}

/// The teams on offer, in the roster's order.
pub fn team_offers(roster: &Roster) -> Vec<TeamOffer> {
    roster
        .teams
        .iter()
        .enumerate()
        .map(|(k, t)| TeamOffer { index: k as u8, name: t.name.clone(), crew: describe(crew_of(t)) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crew_is_described_against_the_standard() {
        assert!(describe(Crew::default()).ends_with("repairs at the standard pace"));
        let better = describe(Crew { a: 5, b: 9 });
        assert!(better.contains("expert engineer") && better.contains("faster"), "{better}");
        let worse = describe(Crew { a: 1, b: 4 });
        assert!(worse.contains("rookie") && worse.contains("slower"), "{worse}");
    }

    #[test]
    fn every_team_is_offered_by_index() {
        let team = |name: &str, eng, mech| Team {
            name: name.into(),
            engineering: eng,
            mechanics: mech,
            ..crate::ai::fixtures::team()
        };
        let roster = Roster { drivers: vec![], teams: vec![team("A", 3, 6), team("B", 5, 5)], cars: vec![] };
        let offers = team_offers(&roster);
        assert_eq!(offers.iter().map(|o| (o.index, o.name.as_str())).collect::<Vec<_>>(), [(0, "A"), (1, "B")]);
        assert_eq!(crew_of(&roster.teams[1]), Crew { a: 5, b: 5 });
    }
}
