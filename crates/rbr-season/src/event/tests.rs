use super::run::{news, ADAPT_STEP};
use super::*;
use crate::config::{Damage, Preset};

fn gravel_pool() -> Vec<StageRef> {
    use crate::config::Surface;
    let mut stages = Vec::new();
    for map in [4u32, 10, 22, 31] {
        for (i, letter) in ['A', 'B', 'C'].iter().enumerate() {
            stages.push(StageRef {
                key: format!("track-{map}_{letter}"),
                name: format!("Map {map} stage {}", i + 1),
                family: map.to_string(),
                surface: Surface::Gravel,
                community: false,
                length: 8000.0,
            });
        }
    }
    stages
}

fn mixed_pool() -> Vec<StageRef> {
    use crate::config::Surface;
    vec![
        StageRef {
            key: "track-4_A".into(),
            name: "Himalaya A".into(),
            family: "4".into(),
            surface: Surface::Gravel,
            community: false,
            length: 8000.0,
        },
        StageRef {
            key: "track-4_B".into(),
            name: "Himalaya B".into(),
            family: "4".into(),
            surface: Surface::Gravel,
            community: false,
            length: 9000.0,
        },
        StageRef {
            key: "track-10_A".into(),
            name: "Alp A".into(),
            family: "10".into(),
            surface: Surface::Tarmac,
            community: false,
            length: 7000.0,
        },
        StageRef {
            key: "track-10_B".into(),
            name: "Alp B".into(),
            family: "10".into(),
            surface: Surface::Tarmac,
            community: false,
            length: 7500.0,
        },
        StageRef {
            key: "101-a".into(),
            name: "Autiovaara I".into(),
            family: "101".into(),
            surface: Surface::Snow,
            community: true,
            length: 10000.0,
        },
        StageRef {
            key: "101-b".into(),
            name: "Autiovaara II".into(),
            family: "101".into(),
            surface: Surface::Snow,
            community: true,
            length: 11000.0,
        },
    ]
}

fn profile(stage: &StageRef) -> StageProfile {
    StageProfile::flat(stage.length, stage.surface, &crate::pace::class_pace("Rally 2"))
}

fn roster() -> Roster {
    Roster {
        drivers: vec![],
        teams: (0..7)
            .map(|k| Team { name: format!("Team {k}"), engineering: 1 + k % 5, ..crate::ai::fixtures::team() })
            .collect(),
        cars: (0..8)
            .map(|k| rbr_formats::ai_ini::Car {
                name: format!("c{k}"),
                control: 3,
                power: 3,
                handling: 3,
                durability: 3,
            })
            .collect(),
    }
}

fn master() -> Master {
    crate::ai::fixtures::master()
}

fn build(config: EventConfig, pool: &[StageRef], cars: &[String]) -> Event {
    Event::build(config, pool, cars, &roster(), &profile).unwrap()
}

fn finish(time: f32) -> PlayerResult {
    PlayerResult { time, penalties: 0.0, retired: false }
}

#[test]
fn the_itinerary_respects_pool_surface_and_length() {
    let pool = mixed_pool();
    let cars = vec!["car".to_owned()];
    // Only gravel, retail only: the two Himalaya stages, twice over for a 4-stage rally.
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 4 };
    config.pool = StagePool::Retail;
    let event = build(config, &pool, &cars);
    assert_eq!(event.rallies.len(), 1);
    assert_eq!(event.rallies[0].stages.len(), 4);
    assert!(event.rallies[0].stages.iter().all(|s| !s.stage.community));
    assert!(event.rallies[0].stages.iter().all(|s| s.stage.surface == crate::config::Surface::Gravel));
    // No repeat while the pool suffices.
    let keys: Vec<&str> = event.rallies[0].stages.iter().map(|s| s.stage.key.as_str()).collect();
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 2 };
    config.pool = StagePool::Retail;
    let event = build(config, &pool, &cars);
    assert_eq!(keys.len(), 4);
    let keys: Vec<&str> = event.rallies[0].stages.iter().map(|s| s.stage.key.as_str()).collect();
    assert_eq!(keys.len(), 2);
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 2, "no repeats when the pool suffices: {keys:?}");
}

#[test]
fn a_season_mixes_surfaces_and_a_weekend_has_two_legs() {
    let pool = mixed_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::Season.config("Rally2", "car", 5);
    config.length = Length { rallies: 4, legs: 1, stages: 2 };
    let event = build(config, &pool, &cars);
    assert_eq!(event.rallies.len(), 4);
    // Each rally keeps one surface.
    for rally in &event.rallies {
        assert_eq!(rally.stages.len(), 2);
        assert!(rally.stages.windows(2).all(|w| w[0].stage.surface == w[1].stage.surface));
    }
    let mut config = Preset::Weekend.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 2, stages: 4 };
    let event = build(config, &pool, &cars);
    let nights: Vec<_> = event.rallies[0].stages.iter().filter(|s| s.service_after == Some(45)).collect();
    assert_eq!(nights.len(), 1, "one overnight service between the legs");
    assert!(event.rallies[0].name.ends_with("Rally"));
}

#[test]
fn clocks_and_conditions_follow_daylight_and_weather() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 3 };
    let event = build(config, &pool, &cars);
    let clocks: Vec<f32> = event.rallies[0].stages.iter().map(|s| s.clock).collect();
    assert!((clocks[0] - 8.0).abs() < 0.01, "{clocks:?}");
    assert!(clocks.windows(2).all(|w| w[1] > w[0]), "{clocks:?}");
    assert!(event.rallies[0].stages.iter().all(|s| s.conditions == Conditions::Dry));
    // Night puts every stage at night.
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 3 };
    config.daylight = Daylight::Night;
    let event = build(config, &pool, &cars);
    assert!(event.rallies[0].stages.iter().all(|s| s.clock >= 21.0 || s.clock < 8.0));
}

#[test]
fn standings_score_the_top_ten_and_retirements() {
    let pool = gravel_pool();
    let cars = vec!["car-a".to_owned(), "car-b".to_owned(), "car-c".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 2 };
    config.field_size = 3;
    let mut event = build(config, &pool, &cars);
    event.turn_done(); // Welcome -> stage 0.
    event.stage_done(&master(), finish(300.0));
    // Force known AI times for the standings check.
    event.results[0][0].1 = vec![AiResult::Time(290.0), AiResult::Time(310.0), AiResult::Retired];
    let standings = event.rally_standings(0);
    assert_eq!(standings.len(), 4);
    assert_eq!(standings[0], Standing { entrant: Some(0), total: Some(290.0), points: 25, bonus: 0 });
    assert_eq!(standings[1], Standing { entrant: None, total: Some(300.0), points: 18, bonus: 0 });
    assert_eq!(standings[2].points, 15);
    // A retired entrant scores nothing, whatever its place.
    assert_eq!(standings[3], Standing { entrant: Some(2), total: None, points: 0, bonus: 0 });
    // The retired AI is out of the rally: still last after stage 2.
    event.turn_done();
    event.stage_done(&master(), PlayerResult { time: 300.0, penalties: 10.0, retired: false });
    event.results[0][1].1 = vec![AiResult::Time(290.0), AiResult::Time(310.0), AiResult::Time(200.0)];
    let standings = event.rally_standings(0);
    assert!(standings.last().unwrap().entrant == Some(2));
    assert_eq!(standings.first().unwrap().points, 25);
    // Points of a single rally are its rally standings.
    let points = event.points();
    assert_eq!(points, event.rally_standings(0));
}

#[test]
fn adaptive_moves_the_field_toward_second_to_fourth() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 4 };
    config.field_size = 4;
    config.adaptive = true;
    config.difficulty = 1.0;
    let mut event = build(config, &pool, &cars);
    let before = event.pace;
    event.turn_done();
    // The player wins easily: the field speeds up (a smaller pace over its pars), by at most the step.
    event.stage_done(&master(), finish(100.0));
    assert!(event.pace < before && before - event.pace <= ADAPT_STEP + 1e-6, "{before} -> {}", event.pace);
    // The player crawls: the field slows down.
    let mid = event.pace;
    event.turn_done();
    event.stage_done(&master(), finish(900.0));
    assert!(event.pace > mid);
    assert!((PACE_RANGE.0..=PACE_RANGE.1).contains(&event.pace));
    assert_eq!(event.config.damage, Damage::Realistic);
}

#[test]
fn the_save_round_trips() {
    let pool = mixed_pool();
    let cars = vec!["car-a".to_owned(), "car|b\\c".to_owned()];
    let mut config = Preset::Season.config("Rally 2", "car|b\\c", 5);
    config.length = Length { rallies: 2, legs: 1, stages: 3 };
    config.team = 4;
    let mut event = build(config, &pool, &cars);
    event.turn_done();
    event.stage_done(&master(), PlayerResult { time: 301.5, penalties: 10.0, retired: false });
    event.turn_done();
    assert!(!event.bulletins(0).is_empty());
    let text = event.save();
    assert!(text.starts_with("rbr-season 4\n"));
    let back = Event::load(&text).unwrap();
    assert_eq!(event, back);
    assert_eq!(Event::load(&back.save()).unwrap(), back);
    assert_eq!(back.config.team, 4);
    assert!(Event::load("bogus").is_err());
    assert!(Event::load("rbr-season 2\nC|x").unwrap_err().contains("older"));
    assert!(Event::load("rbr-season 4\nbogus|line").is_err());
}

/// Drives the whole event with a fixed player time, returning the stages driven.
fn run(event: &mut Event, time: f32) -> usize {
    let m = master();
    let mut stages = 0;
    loop {
        match event.turn.clone() {
            Turn::Welcome { .. } | Turn::Service { .. } | Turn::RallyOver { .. } => event.turn_done(),
            Turn::Stage { .. } => {
                event.stage_done(&m, finish(time));
                stages += 1;
            }
            Turn::EventOver => break,
        }
        assert!(stages < 200, "the event must end");
    }
    stages
}

#[test]
fn a_full_season_runs_with_a_fixed_player_time() {
    let pool = gravel_pool();
    let cars = vec!["car-a".to_owned(), "car-b".to_owned()];
    let mut config = Preset::Season.config("Rally2", "car-a", 99);
    config.length = Length { rallies: 2, legs: 1, stages: 6 };
    config.field_size = 12;
    let mut event = build(config, &pool, &cars);
    let ref_time = event.rallies[0].stages[0].reference;
    assert_eq!(run(&mut event, ref_time * 1.05), 12);
    assert_eq!(event.turn, Turn::EventOver);
    let points = event.points();
    assert_eq!(points.len(), 13);
    assert!(points.windows(2).all(|w| w[0].points >= w[1].points));
    // The points go to finishers, 25 + 18 + ... plus the 5 + 4 + ... power-stage bonus a rally at most.
    assert!(
        points.iter().map(|r| r.points).sum::<u32>()
            <= 2 * (25 + 18 + 15 + 12 + 10 + 8 + 6 + 4 + 2 + 1 + 5 + 4 + 3 + 2 + 1)
    );
    assert!(!event.bulletins(0).is_empty());
    // The AI drove its stages with the engine: the times scatter around the pars.
    let times: Vec<f32> =
        event.results[0][0].1.iter().filter_map(|r| if let AiResult::Time(t) = r { Some(*t) } else { None }).collect();
    assert!(times.len() > 6 && times.iter().any(|t| (t - times[0]).abs() > 0.01), "{times:?}");
    // The save still round-trips at the end.
    assert_eq!(Event::load(&event.save()).unwrap(), event);
}

#[test]
fn a_retired_player_ends_the_rally_and_scores_nothing() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 21);
    config.length = Length { rallies: 2, legs: 1, stages: 4 };
    config.field_size = 8;
    config.restart = false;
    let mut event = build(config, &pool, &cars);
    event.turn_done();
    event.stage_done(&master(), finish(300.0));
    event.turn_done();
    if matches!(event.turn, Turn::Service { .. }) {
        event.turn_done();
    }
    event.stage_done(&master(), PlayerResult { time: 0.0, penalties: 0.0, retired: true });
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    // The field drove all four stages.
    assert!(event.results[0].iter().all(|(_, ai)| ai.len() == 8));
    let rows = event.rally_standings(0);
    let player = rows.iter().find(|r| r.entrant.is_none()).unwrap();
    assert_eq!((player.total, player.points), (None, 0));
    assert!(rows.iter().filter(|r| r.points > 0).count() >= 1);
    // The event goes on to the next rally, and the retired rally's points count for the field alone.
    event.turn_done();
    assert_eq!(event.turn, Turn::Welcome { rally: 1 });
    let points = event.points();
    assert_eq!(points.iter().find(|r| r.entrant.is_none()).unwrap().points, 0);
    assert!(points[0].points > 0);
}

#[test]
fn the_start_order_follows_points_then_standings() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 33);
    config.length = Length { rallies: 2, legs: 1, stages: 3 };
    config.field_size = 6;
    let mut event = build(config, &pool, &cars);
    let first = event.start_order(0, 0);
    assert_eq!(first.len(), 7);
    assert_eq!(first, event.start_order(0, 0));
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(sorted, std::iter::once(None).chain((0..6).map(Some)).collect::<Vec<_>>());
    run_rally(&mut event, 0);
    // Stage 2 of the first rally starts by its standings, leader first.
    let standings: Vec<Option<usize>> = event.standings_after(0, 1).into_iter().map(|r| r.entrant).collect();
    assert_eq!(event.start_order(0, 1), standings);
    // The second rally's first stage starts by the season points.
    let points = event.points();
    let order = event.start_order(1, 0);
    let pts = |who: Option<usize>| points.iter().find(|r| r.entrant == who).unwrap().points;
    assert!(order.windows(2).all(|w| pts(w[0]) >= pts(w[1])), "{order:?}");
    // The road wear follows the player's place: the front three drive a new road.
    let place = event.start_order(0, 1).iter().position(Option::is_none).unwrap();
    assert_eq!(
        event.road_wear(0, 1),
        if place <= 2 {
            0.0
        } else if place <= 10 {
            1.0
        } else {
            2.0
        }
    );
}

fn run_rally(event: &mut Event, rally: usize) {
    let m = master();
    while !matches!(event.turn, Turn::RallyOver { rally: r } if r == rally) {
        match event.turn.clone() {
            Turn::Welcome { .. } | Turn::Service { .. } => event.turn_done(),
            Turn::Stage { .. } => event.stage_done(&m, finish(300.0)),
            _ => unreachable!(),
        }
    }
    event.turn_done();
    event.turn_done();
}

#[test]
fn the_crew_is_the_picked_teams() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 1);
    config.team = 3;
    let event = build(config, &pool, &cars);
    assert_eq!(event.crew(), crate::service::Crew { a: 4, b: 6 });
    // An out-of-range team falls back to the default crew.
    let mut config = Preset::QuickRally.config("Rally2", "car", 1);
    config.team = 99;
    assert_eq!(build(config, &pool, &cars).crew(), crate::service::Crew::default());
}

#[test]
fn the_ai_is_serviced_between_stages() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 4);
    config.length = Length { rallies: 1, legs: 1, stages: 3 };
    config.field_size = 4;
    let mut event = build(config, &pool, &cars);
    event.turn_done();
    event.stage_done(&master(), finish(300.0));
    // The service falls after the second of three stages: a worn car is repaired in it, at the team's cost.
    event.turn_done();
    for c in &mut event.contenders {
        c.car.parts[2] = 3;
    }
    assert_eq!(event.costs.iter().map(|c| c.rally).sum::<u32>(), 0);
    event.stage_done(&master(), finish(300.0));
    assert!(matches!(event.turn, Turn::Service { minutes: 20, .. }));
    let spent: u32 = event.costs.iter().map(|c| c.rally).sum();
    assert!(spent > 0, "the service repaired nothing");
    let alive = (0..4).filter(|&e| event.results[0][1].1[e] != AiResult::Retired).collect::<Vec<_>>();
    assert!(!alive.is_empty() && alive.iter().all(|&e| event.contenders[e].car.parts[2] < 3));
}

// The presets' shared and differing settings.
#[test]
fn the_presets_share_the_field_and_differ_in_shape() {
    for preset in Preset::ALL {
        let c = preset.config("Rally2", "car", 1);
        assert_eq!((c.field_size, c.difficulty, c.adaptive, c.damage), (12, 0.6, false, Damage::Realistic));
    }
    let quick = Preset::QuickRally.config("Rally2", "car", 1);
    assert_eq!(quick.length, Length { rallies: 1, legs: 1, stages: 6 });
    let weekend = Preset::Weekend.config("Rally2", "car", 1);
    assert_eq!(weekend.length, Length { rallies: 1, legs: 3, stages: 12 });
    assert_eq!(weekend.daylight, Daylight::Realistic);
    let championship = Preset::Championship.config("Rally2", "car", 1);
    assert_eq!(championship.length, Length { rallies: 4, legs: 1, stages: 6 });
    let season = Preset::Season.config("Rally2", "car", 1);
    assert_eq!(season.length, Length { rallies: 6, legs: 2, stages: 6 });
    assert_eq!(season.surfaces, Surfaces::Mixed);
    // Super Rally is on everywhere; the WRC-style rallies over several legs pay the power stage, one-day ones not.
    assert!(quick.restart && !quick.power_stage);
    assert!(championship.restart && !championship.power_stage);
    assert!(weekend.restart && weekend.power_stage);
    assert!(season.restart && season.power_stage);
    for seed in 0..20 {
        let c = Preset::Random.config("Rally2", "car", seed);
        assert_eq!(c.power_stage, c.length.legs > 1, "seed {seed}");
        assert!(c.length.stages >= 2 * c.length.legs && c.restart, "seed {seed}");
    }
}

#[test]
fn each_leg_is_a_day_and_a_rallys_legs_split_its_stages() {
    let mut config = Preset::Weekend.config("Rally2", "car", 3);
    config.surfaces = crate::config::Surfaces::Only(crate::config::Surface::Gravel);
    config.length = Length { rallies: 2, legs: 3, stages: 8 };
    let event = build(config.clone(), &gravel_pool(), &["car".to_owned()]);
    for rally in &event.rallies {
        // 3 + 3 + 2, an overnight service after each leg but the last.
        let ends: Vec<usize> = (0..8).filter(|&k| rally.stages[k].ends_leg()).collect();
        assert_eq!(ends, [2, 5]);
        assert!(rally.stages.last().unwrap().service_after.is_none());
        // Each leg starts the next morning; on realistic daylight the first day's last two run at dusk.
        let clocks: Vec<f32> = rally.stages.iter().map(|s| s.clock).collect();
        assert_eq!((clocks[0], clocks[3], clocks[6]), (8.0, 8.0, 8.0), "{clocks:?}");
        assert!(clocks[1] >= 19.0 && clocks[2] >= 19.5, "{clocks:?}");
        assert!(clocks[3..].iter().all(|&c| c < 19.0), "{clocks:?}");
    }
    config.length.legs = 9;
    let gravel = gravel_pool();
    assert!(Event::build(config, &gravel, &["car".to_owned()], &roster(), &profile).is_err());
}

/// A ten-stage weekend: leg 1 SS1-SS5 (a service after SS3, the overnight service after SS5), leg 2 SS6-SS10.
fn weekend(seed: u32) -> Event {
    let mut config = Preset::Weekend.config("Rally2", "car", seed);
    config.length = Length { rallies: 1, legs: 2, stages: 10 };
    config.surfaces = crate::config::Surfaces::Only(crate::config::Surface::Gravel);
    config.field_size = 6;
    let event = build(config, &gravel_pool(), &["car".to_owned()]);
    let ends: Vec<bool> = event.rallies[0].stages.iter().map(|s| s.ends_leg()).collect();
    assert_eq!(ends, [false, false, false, false, true, false, false, false, false, false]);
    event
}

#[test]
fn a_retired_player_restarts_at_the_next_leg() {
    let mut event = weekend(21);
    assert!(event.config.restart);
    // Not after the mid-leg service: at the next leg, behind the overnight service.
    assert_eq!(event.rejoin_after(0, 0), Some(4));
    assert_eq!(event.rejoin_after(0, 4), Some(4));
    assert_eq!(event.rejoin_after(0, 5), None);
    let m = master();
    event.turn_done();
    event.stage_done(&m, finish(300.0));
    event.stage_done(&m, PlayerResult { time: 0.0, penalties: 0.0, retired: true });
    // The field drives to the overnight service; the player sits out SS3-SS5.
    assert_eq!(event.turn, Turn::Service { rally: 0, stage: 4, minutes: 45 });
    assert!((2..5).all(|k| event.results[0][k].0.is_none() && event.results[0][k].1.len() == 6));
    // Each stage not completed, the retirement's included: the stage's fastest time plus ten minutes.
    assert_eq!(RESTART_CHARGE, 600.0);
    for stage in 1..5 {
        let fastest = event.results[0][stage].1.iter().filter_map(|r| match r {
            AiResult::Time(t) => Some(*t),
            _ => None,
        });
        let fastest = fastest.reduce(f32::min).unwrap_or(event.rallies[0].stages[stage].reference);
        assert_eq!(event.charged(0, stage, None), Some(fastest + RESTART_CHARGE));
    }
    assert_eq!(event.charged(0, 0, None), None, "completed");
    // The player drives leg 2 and is classified with the charges.
    event.turn_done();
    assert_eq!(event.turn, Turn::Stage { rally: 0, stage: 5 });
    for _ in 5..10 {
        event.stage_done(&m, finish(300.0));
        if matches!(event.turn, Turn::Service { .. }) {
            event.turn_done();
        }
    }
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    let charges: f32 = (1..5).map(|s| event.charged(0, s, None).unwrap()).sum();
    let rows = event.rally_standings(0);
    let at = rows.iter().position(|r| r.entrant.is_none()).unwrap();
    assert_eq!(rows[at].total, Some(charges + 300.0 * 6.0));
    assert!(rows[at].points >= POINTS.get(at).copied().unwrap_or(0));
}

#[test]
fn a_retired_player_in_the_last_leg_is_out() {
    let mut event = weekend(21);
    let m = master();
    event.turn_done();
    while event.turn != (Turn::Stage { rally: 0, stage: 6 }) {
        match event.turn {
            Turn::Stage { .. } => event.stage_done(&m, finish(300.0)),
            _ => event.turn_done(),
        }
    }
    event.stage_done(&m, PlayerResult { time: 0.0, penalties: 0.0, retired: true });
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    assert!(event.results[0].iter().all(|(_, ai)| ai.len() == 6));
    let player = event.rally_standings(0).into_iter().find(|r| r.entrant.is_none()).unwrap();
    assert_eq!((player.total, player.points), (None, 0));
    assert_eq!(event.charged(0, 6, None), None);
}

#[test]
fn a_retired_crew_restarts_at_the_next_leg_and_a_hurt_one_does_not() {
    let mut event = weekend(3);
    let m = master();
    event.turn_done();
    while event.turn != (Turn::Stage { rally: 0, stage: 4 }) {
        match event.turn {
            Turn::Stage { .. } => event.stage_done(&m, finish(300.0)),
            _ => event.turn_done(),
        }
    }
    // Entrant 0 retired on SS2, entrant 1 hurt there; both sat out SS3 and SS4.
    for k in 1..4 {
        event.results[0][k].1[0] = AiResult::Retired;
        event.results[0][k].1[1] = AiResult::Out;
    }
    event.contenders[0].car.parts = [4; 8];
    // SS5 ends the leg: both still sit out.
    event.stage_done(&m, finish(300.0));
    assert_eq!((event.results[0][4].1[0], event.results[0][4].1[1]), (AiResult::Retired, AiResult::Out));
    // The overnight service rebuilt the retired car; SS6 has the restart and not the hurt crew.
    assert_eq!(event.contenders[0].car.parts, [0; 8]);
    event.turn_done();
    event.stage_done(&m, finish(300.0));
    assert_ne!(event.results[0][5].1[0], AiResult::Retired, "restarted");
    assert_eq!(event.results[0][5].1[1], AiResult::Out);
    assert!(event.bulletins(0).iter().any(|l| l.starts_with("SS6: ") && l.ends_with("restarts under Super Rally")));
    // The restarted crew is charged for SS2-SS5 and classified while it runs; the hurt one is not.
    let charged = (1..5).filter_map(|k| event.charged(0, k, Some(0))).count();
    assert_eq!(charged, 4);
    assert_eq!(event.charged(0, 1, Some(1)), None);
    let rows = event.rally_standings(0);
    let total = |e| rows.iter().find(|r| r.entrant == Some(e)).unwrap().total;
    assert!(total(1).is_none());
    assert_eq!(total(0).is_some(), matches!(event.results[0][5].1[0], AiResult::Time(_)));
}

#[test]
fn the_power_stage_pays_the_five_fastest_on_the_last_stage() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::QuickRally.config("Rally2", "car", 7);
    config.length = Length { rallies: 1, legs: 1, stages: 2 };
    config.field_size = 6;
    config.power_stage = true;
    let mut event = build(config, &pool, &cars);
    let m = master();
    event.turn_done();
    event.stage_done(&m, finish(300.0));
    event.turn_done();
    event.stage_done(&m, finish(300.0));
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    // Fixed times: the player ties an entrant for fifth on the last stage, and nobody carries penalties.
    event.penalties[0] = vec![0.0; 6];
    event.results[0][0].1 = vec![AiResult::Time(290.0); 6];
    event.results[0][1].1 = [280.0, 285.0, 290.0, 295.0, 300.0, 310.0].map(AiResult::Time).into();
    let rows = event.rally_standings(0);
    let at = |who: Option<usize>| rows.iter().find(|r| r.entrant == who).unwrap().clone();
    assert_eq!(at(Some(0)), Standing { entrant: Some(0), total: Some(570.0), points: 30, bonus: 5 });
    assert_eq!(at(Some(1)), Standing { entrant: Some(1), total: Some(575.0), points: 22, bonus: 4 });
    assert_eq!(at(Some(2)), Standing { entrant: Some(2), total: Some(580.0), points: 18, bonus: 3 });
    assert_eq!(at(Some(3)), Standing { entrant: Some(3), total: Some(585.0), points: 14, bonus: 2 });
    // The tie keeps the rows' order: the better-placed entrant takes fifth ahead of the player.
    assert_eq!(at(Some(4)), Standing { entrant: Some(4), total: Some(590.0), points: 11, bonus: 1 });
    assert_eq!(at(None), Standing { entrant: None, total: Some(600.0), points: 8, bonus: 0 });
    assert_eq!(at(Some(5)), Standing { entrant: Some(5), total: Some(600.0), points: 6, bonus: 0 });
    // Without the power stage every bonus is 0 and the points are the place points.
    event.config.power_stage = false;
    let rows = event.rally_standings(0);
    assert!(rows.iter().all(|r| r.bonus == 0));
    assert_eq!(rows.iter().map(|r| r.points).collect::<Vec<_>>(), vec![25, 18, 15, 12, 10, 8, 6]);
}

#[test]
fn the_save_round_trips_with_restart_and_power_stage() {
    let pool = gravel_pool();
    let cars = vec!["car".to_owned()];
    let mut config = Preset::Season.config("Rally2", "car", 5);
    config.length = Length { rallies: 1, legs: 1, stages: 2 };
    assert!(config.restart && config.power_stage);
    let mut event = build(config, &pool, &cars);
    event.turn_done();
    event.stage_done(&master(), finish(300.0));
    let text = event.save();
    assert!(text.starts_with("rbr-season 4\n"));
    let head = text.lines().find(|l| l.starts_with("C|")).unwrap();
    assert_eq!(head.split('|').count(), 17, "the tag and 16 config fields");
    assert_eq!(Event::load(&text).unwrap(), event);
    // A v3 text (no restart/power fields) loads with both flags off and is otherwise equal.
    assert!(head.contains("|length:1:1:2|"), "{head}");
    // A v3 text has the old length words and no restart/power fields.
    let v3: Vec<String> = text
        .replace("rbr-season 4", "rbr-season 3")
        .replace("|length:1:1:2|", "|rally:2|")
        .lines()
        .map(|line| match line.strip_prefix("C|") {
            Some(rest) => {
                let mut fields: Vec<&str> = rest.split('|').collect();
                fields.truncate(fields.len() - 2);
                format!("C|{}", fields.join("|"))
            }
            None => line.to_owned(),
        })
        .collect();
    let old = Event::load(&v3.join("\n")).unwrap();
    assert!(!old.config.restart && !old.config.power_stage);
    let mut expected = event.clone();
    expected.config.restart = false;
    expected.config.power_stage = false;
    assert_eq!(old, expected);
    // The old weekend and season words: two legs, and one leg a rally.
    for (word, length) in [
        ("weekend:2", Length { rallies: 1, legs: 2, stages: 2 }),
        ("season:3:2", Length { rallies: 3, legs: 1, stages: 2 }),
    ] {
        let old = Event::load(&v3.join("\n").replace("|rally:2|", &format!("|{word}|"))).unwrap();
        assert_eq!(old.config.length, length);
    }
}

#[test]
fn the_news_sums_a_drivers_accidents_and_skips_the_minor_ones() {
    use crate::ai::{AccidentRecord, Bulletin, RunLog};
    let name = |id: i32| format!("D{id}");
    let accident = |driver, time| Bulletin::Accident { driver, accident: 1, time };
    let shaken = |driver, section| Bulletin::Shaken { driver, accident: 1, stage: 1, section, injury: 1 };
    let log = RunLog {
        bulletins: vec![
            accident(0, 2.0),
            accident(1, 1.0),
            shaken(0, 1),
            accident(0, 4.0),
            shaken(0, 2),
            Bulletin::ManagerOrder { driver: 2, order: 1 },
            accident(2, 30.0),
            Bulletin::FalseStart { driver: 1, stage: 1 },
        ],
        accidents: vec![AccidentRecord { driver: 3, section: 2, accident: 4, retired: true, time: 0.0 }],
    };
    assert_eq!(
        news(&log, 1, &name, true),
        [
            "SS2: D0 lost 6.0 s in 2 accidents",
            "SS2: D0 was shaken by an accident in section 2",
            "SS2: D2 lost 30.0 s in an accident",
            "SS2: D1 jumped the start",
            "SS2: D3 retired after an accident in section 3",
        ]
    );
    // Only minor losses: an uneventful stage, but not after a service.
    let quiet = RunLog { bulletins: vec![accident(0, 1.5), accident(1, 0.4)], accidents: Vec::new() };
    assert_eq!(news(&quiet, 0, &name, true), ["SS1: An uneventful stage"]);
    assert!(news(&RunLog::default(), 0, &name, false).is_empty());
}
