use super::*;
use rbr_season::config::{Daylight, Skill, StagePool, Surfaces, Weather};

fn config() -> EventConfig {
    EventConfig {
        class: "Rally2".into(),
        car: "rsf-1".into(),
        length: Length { rallies: 1, legs: 1, stages: 6 },
        pool: StagePool::All,
        surfaces: Surfaces::Only(Surface::Gravel),
        daylight: Daylight::Day,
        weather: Weather::Changeable,
        damage: Damage::Realistic,
        team: 0,
        skill: Skill::Professional,
        difficulty: 0.6,
        adaptive: true,
        restart: true,
        power_stage: false,
        field_size: 12,
        seed: 1,
    }
}

fn stage(key: &str, surface: Surface, community: bool, length: f32) -> StageRef {
    StageRef { key: key.into(), name: key.into(), family: key.into(), surface, community, length }
}

#[test]
fn the_summary_reads_the_event_at_a_glance() {
    let stages = [
        stage("1", Surface::Gravel, false, 10_000.0),
        stage("2", Surface::Gravel, true, 14_000.0),
        stage("3", Surface::Tarmac, false, 30_000.0),
    ];
    assert_eq!(summary(&config(), Some(&stages)), "6 stages · 72 km · gravel · 12 Rally2 drivers");
    assert_eq!(summary(&config(), None), "6 stages · gravel · 12 Rally2 drivers");
    let retail = EventConfig { pool: StagePool::Retail, field_size: 1, ..config() };
    assert_eq!(summary(&retail, Some(&stages)), "6 stages · 60 km · gravel · 1 Rally2 driver");
    let season = EventConfig {
        length: Length { rallies: 2, legs: 1, stages: 3 },
        surfaces: Surfaces::Mixed,
        class: String::new(),
        ..config()
    };
    assert_eq!(summary(&season, Some(&stages)), "2 rallies · 6 stages · 108 km · mixed surfaces · 12 drivers");
    let weekend = EventConfig { length: Length { rallies: 1, legs: 3, stages: 12 }, ..config() };
    assert_eq!(summary(&weekend, None), "3 legs · 12 stages · gravel · 12 Rally2 drivers");
    let season = EventConfig { length: Length { rallies: 6, legs: 2, stages: 6 }, ..config() };
    assert_eq!(summary(&season, None), "6 rallies · 2 legs each · 36 stages · gravel · 12 Rally2 drivers");
    // No stage fits: no length.
    let snow = EventConfig { surfaces: Surfaces::Only(Surface::Snow), ..config() };
    assert_eq!(summary(&snow, Some(&stages)), "6 stages · snow · 12 Rally2 drivers");
    assert_eq!([0.0, 0.3, 0.6, 1.0].map(difficulty_word), ["Gentle", "Club", "National", "Pro"]);
    assert_eq!(clock_hm(20.5), "20:30");
}

/// A page on the fixture AI with a pool of the fixture's stages.
fn page(here: &Path) -> ChampionshipPage {
    page_on(here, Preset::QuickRally)
}

/// A page on the fixture AI with `preset`'s championship started.
fn page_on(here: &Path, preset: Preset) -> ChampionshipPage {
    let mut page = ChampionshipPage::new(&Data::new(here.join("data"), None));
    page.ai = Ok(fixture::ai());
    let cars = [Car {
        key: "rsf-1".into(),
        group: Some("Rally2".into()),
        model: Ok(rbr_formats::car_pack::Model { entry: rbr_formats::car_pack::model_entry("a", "a"), fallback: None }),
        ..Car::default()
    }];
    page.open_builder(preset, &cars, 0, 3);
    page.start_event(here, "tester", &cars, &fixture::stages());
    page
}

/// Drives the next stage: `rbr-standalone`'s files as it leaves them.
fn drive(page: &mut ChampionshipPage, here: &Path, time: f32, damage: Option<&CarriedDamage>) -> StageStart {
    let start = page.stage_start(here).unwrap();
    let (_, out, result) = ChampionshipPage::stage_files(here);
    let race = EndRace { time, splits: [time / 3.0, time * 2.0 / 3.0], retired: false };
    let report =
        StageReport { end: StageEnd { cause: EndCause::Finish, end_race: race }, drivable: true, false_start: false };
    std::fs::write(&result, report.to_text()).unwrap();
    if let Some(damage) = damage {
        std::fs::write(&out, damage.to_bytes()).unwrap();
    }
    page.stage_closed(here, "tester");
    start
}

#[test]
fn a_rally_runs_its_turns_with_service_damage_and_the_late_penalty() {
    let here = crate::test_dir("championship-turns");
    let mut page = page(&here);
    let turn = |page: &ChampionshipPage| page.run.as_ref().unwrap().event.turn.clone();
    assert_eq!(turn(&page), Turn::Welcome { rally: 0 });
    let (save, dmg, state) = ChampionshipPage::files(&here, "tester");
    assert!(save.is_file(), "saved at the welcome");
    page.turn_done(&here, "tester");
    // SS1: the road's tyre, the itinerary's clock and the flags; no damage carried yet.
    let start = drive(&mut page, &here, 300.0, None);
    let first = page.run.as_ref().unwrap().event.rallies[0].stages[0].clone();
    assert_eq!((start.map.to_string(), start.conditions), (first.stage.key.clone(), first.conditions));
    assert_eq!(start.tyre, rbr_gameplay::weather::tyre_kind(0, wetness(first.conditions).1).unwrap_or(3));
    let flag = |name: &str| start.args.iter().position(|a| a == name).map(|i| start.args[i + 1].as_str());
    assert_eq!(flag("--start-time"), Some(clock_hm(first.clock).as_str()));
    assert_eq!(flag("--championship-damage"), Some("realistic"));
    assert_eq!(flag("--rally-next"), Some("stage"));
    assert_eq!(flag("--damage-in"), None);
    // The fixture's Professional skill is the game's Professional.
    assert_eq!(flag("--championship"), Some("professional"));
    assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 1 });
    drive(&mut page, &here, 305.0, None);
    // SS3 leaves damage and a service follows.
    let mut damage = CarriedDamage::default();
    damage.service_refresh();
    let start = drive(&mut page, &here, 310.0, Some(&damage));
    assert!(start.args.iter().any(|a| a == "service"));
    assert_eq!(turn(&page), Turn::Service { rally: 0, stage: 2, minutes: 20 });
    assert!(dmg.is_file());
    page.service_done(&here, "tester", 20.0, damage.clone());
    assert_eq!(turn(&page), Turn::Stage { rally: 0, stage: 3 });
    assert!(std::fs::read_to_string(&state).unwrap().contains("penalty = 20"));
    // SS4 takes the damage in and the penalty into its result.
    let start = drive(&mut page, &here, 320.0, None);
    assert!(start.args.iter().any(|a| a == "--damage-in"));
    let event = &page.run.as_ref().unwrap().event;
    assert_eq!(event.results[0][3].0.unwrap().penalties, 20.0);
    assert_eq!(page.run.as_ref().unwrap().penalty, 0.0);
    // A stage left before the finish retires: the rally is over, its standings' player line has no total.
    page.stage_start(&here).unwrap();
    page.stage_closed(&here, "tester");
    assert_eq!(turn(&page), Turn::RallyOver { rally: 0 });
    let lines = page.run.as_ref().unwrap().event.rally_standings(0);
    assert_eq!(lines.iter().find(|l| l.entrant.is_none()).unwrap().total, None);
    page.turn_done(&here, "tester");
    assert_eq!(turn(&page), Turn::EventOver);
    // Continue reads it back with the damage; leaving the finished championship removes its files.
    let mut again = ChampionshipPage::new(&Data::new(here.join("data"), None));
    again.ai = Ok(fixture::ai());
    assert!(again.saved_event(&here, "tester").is_some());
    again.continue_event(&here, "tester");
    assert!(again.run.as_ref().is_some_and(|r| r.damage.is_some()));
    again.leave_event(&here, "tester");
    assert!(!save.exists() && !dmg.exists() && !state.exists());
    assert_eq!(again.saved_event(&here, "tester"), None);
    std::fs::remove_dir_all(&here).unwrap();
}

#[test]
fn a_retired_car_is_rebuilt_overnight_and_restarts_the_next_leg() {
    let here = crate::test_dir("championship-super-rally");
    // Three legs of four stages: the overnight services after SS4 and SS8.
    let mut page = page_on(&here, Preset::Weekend);
    assert!(page.run.as_ref().unwrap().event.config.restart, "the presets restart");
    page.turn_done(&here, "tester");
    // SS1 is left wrecked: the field drives the rest of the leg, the car is rebuilt overnight.
    let mut damage = CarriedDamage::default();
    damage.service_refresh();
    page.stage_start(&here).unwrap();
    let (_, out, _) = ChampionshipPage::stage_files(&here);
    std::fs::write(&out, damage.to_bytes()).unwrap();
    page.stage_closed(&here, "tester");
    let run = page.run.as_ref().unwrap();
    assert_eq!(run.event.turn, Turn::Service { rally: 0, stage: 3, minutes: 45 });
    assert!(run.damage.is_none(), "rebuilt");
    assert!((0..4).all(|k| run.event.charged(0, k, None).is_some()));
    assert_eq!(super::view::rejoin(&run.event), Some(4));
    // No park: the restart moves on with the rebuilt car, no damage carried in.
    page.turn_done(&here, "tester");
    assert!(page.run.as_ref().unwrap().damage.is_none());
    let start = page.stage_start(&here).unwrap();
    assert!(!start.args.iter().any(|a| a == "--damage-in"));
    page.abort_stage_start();
    for time in [300.0, 305.0, 310.0, 315.0, 320.0, 325.0, 330.0, 335.0] {
        drive(&mut page, &here, time, None);
        if matches!(page.run.as_ref().unwrap().event.turn, Turn::Service { .. }) {
            page.service_done(&here, "tester", 0.0, CarriedDamage::default());
        }
    }
    // Classified: the charges and the driven stages make the total.
    let event = &page.run.as_ref().unwrap().event;
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    let charges: f32 = (0..4).filter_map(|k| event.charged(0, k, None)).sum();
    let mine = event.rally_standings(0).into_iter().find(|l| l.entrant.is_none()).unwrap();
    assert_eq!(mine.total, Some(charges + 2540.0));
    std::fs::remove_dir_all(&here).unwrap();
}

#[test]
fn a_trial_between_rallies_is_practice_that_is_not_recorded() {
    let here = crate::test_dir("championship-trial");
    let mut page = page(&here);
    let mut event = fixture::fixture_event();
    page.screenshot_event(event.clone());
    assert!(page.trial(false).is_err(), "trials come between rallies");
    event.turn = Turn::RallyOver { rally: 0 };
    page.screenshot_event(event.clone());
    let stage = page.trial(false).unwrap();
    assert_eq!(stage.map.to_string(), event.rallies[1].stages[0].stage.key);
    assert!(stage.args.is_empty(), "no championship flags, no files");
    // Its end changes nothing: the turn stays, nothing is recorded.
    page.stage_closed(&here, "tester");
    assert_eq!(page.run.as_ref().unwrap().event.turn, Turn::RallyOver { rally: 0 });
    assert!(page.run.as_ref().unwrap().event.results[0].iter().all(|(player, _)| player.is_none()));
    assert_eq!(page.trial(true).unwrap().map, YARD);
    page.abort_stage_start();
    assert!(!page.driving && !page.practice);
    std::fs::remove_dir_all(&here).unwrap();
}

#[test]
fn the_skill_levels_are_the_games_three_with_their_help_rules() {
    assert_eq!(assistance_note(Skill::Rookie), "10 s penalty");
    assert_eq!(assistance_note(Skill::Professional), "20 s penalty");
    assert!(assistance_note(Skill::Champion).starts_with("35 s penalty; a stuck car is not helped"));
}

#[test]
fn a_stage_that_never_starts_changes_nothing() {
    let here = crate::test_dir("championship-abort");
    let mut page = page(&here);
    page.turn_done(&here, "tester");
    page.stage_start(&here).unwrap();
    page.abort_stage_start();
    page.stage_closed(&here, "tester");
    assert_eq!(page.run.as_ref().unwrap().event.turn, Turn::Stage { rally: 0, stage: 0 });
    std::fs::remove_dir_all(&here).unwrap();
}

/// A Quick rally on the installed stages and the real `ai.ini`, played with a fixed player pace (`RBR_DATA`).
#[test]
fn a_quick_rally_on_the_real_stages_and_ai_plays_through() {
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    let Some(data) = Data::from_env() else {
        eprintln!("skipping: RBR_DATA not set");
        return;
    };
    let install = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap();
    let season = rbr_gameplay::weather::Season::load(&data).unwrap();
    let catalog = crate::catalog::Catalog::scan(&install).unwrap();
    let here = crate::test_dir("championship-real");
    let cache = here.join(pool::CACHE);
    let (done, stop) = (AtomicUsize::new(0), AtomicBool::new(false));
    let stages = pool::read(&data, &cache, pool::items(&catalog, &season), &done, &stop);
    assert!(stages.len() >= 20, "{} stages read", stages.len());
    let (master, roster) = rbr_season::ai::load_ai(&data).unwrap();
    assert!(!rbr_season::team::team_offers(&roster).is_empty());

    let config = Preset::QuickRally.config("Rally2", "", 11);
    let class = pace::class_pace(&config.class);
    let profile = |s: &StageRef| pool::profile(&cache, s, &class);
    let cars = ["Skoda Fabia Rally2".to_owned(), "Citroen C3 Rally2".to_owned()];
    let mut event = Event::build(config, &stages, &cars, &roster, &profile).unwrap();
    let field = event.field.len();
    assert_eq!(event.rallies[0].stages.len(), 6);
    event.turn_done();
    let mut player = 0.0;
    // Saved and read back mid-rally, the championship runs on as the one never saved.
    let mut resumed: Option<Event> = None;
    while let Turn::Stage { rally, stage } = event.turn {
        if stage == 3 && resumed.is_none() {
            let loaded = Event::load(&event.save()).unwrap();
            assert_eq!(loaded, event, "read back before SS4");
            resumed = Some(loaded);
        }
        let order = event.start_order(rally, stage);
        assert_eq!(order.len(), field + 1);
        assert_eq!(order.iter().filter(|who| who.is_none()).count(), 1);
        assert!(event.road_wear(rally, stage) <= 2.0);
        let reference = event.rallies[rally].stages[stage].reference;
        assert!(reference > 30.0, "SS{} reference {reference}", stage + 1);
        let time = reference * 1.08;
        player += time;
        for event in std::iter::once(&mut event).chain(resumed.as_mut()) {
            event.stage_done(&master, PlayerResult { time, penalties: 0.0, retired: false });
            if let Turn::Service { .. } = event.turn {
                event.turn_done();
            }
        }
    }
    assert_eq!(resumed.as_ref(), Some(&event), "the resumed championship ends as the one never saved");
    assert_eq!(event.turn, Turn::RallyOver { rally: 0 });
    let standings = event.rally_standings(0);
    assert_eq!(standings.len(), field + 1);
    let place = standings.iter().position(|row| row.entrant.is_none()).unwrap();
    let total = standings[place].total.expect("the player finished");
    assert!((total - player).abs() < 0.5, "{total} against {player}");
    // A 8 % slower player is not winning against a field of the class's pace, and is not last.
    assert!(place > 0 && place < field, "the player is {} of {}", place + 1, field + 1);
    let totals: Vec<f32> = standings.iter().filter_map(|row| row.total).collect();
    assert!(totals.windows(2).all(|w| w[0] <= w[1]), "sorted by total");
    assert!(standings[0].points > 0 && standings[0].points >= standings.last().unwrap().points);
    let news = event.bulletins(0);
    assert!(!news.is_empty() && news.iter().all(|line| !line.trim().is_empty()), "{news:?}");
    assert_eq!(Event::load(&event.save()).unwrap(), event);
    std::fs::remove_dir_all(&here).unwrap();
}

#[test]
fn the_players_own_itinerary_and_field_models_start_the_championship() {
    let here = crate::test_dir("championship-own-plan");
    let mut page = ChampionshipPage::new(&Data::new(here.join("data"), None));
    page.ai = Ok(fixture::ai());
    let model =
        || Ok(rbr_formats::car_pack::Model { entry: rbr_formats::car_pack::model_entry("a", "a"), fallback: None });
    let car = |key: &str| Car { key: key.into(), group: Some("Rally2".into()), model: model(), ..Car::default() };
    let cars = [car("rsf-1"), car("rsf-2"), car("rsf-3")];
    page.open_builder(Preset::QuickRally, &cars, 0, 3);
    let stages = fixture::stages();
    page.pool = Some(Pool::Ready(stages.clone()));
    page.plan.follow(page.config.as_ref().unwrap(), &stages);
    // SS1 changed through the stage browser; a map outside the pool is refused.
    page.picking = Some(plan::Slot { rally: 0, leg: 0, at: Some(0) });
    page.pick_stage(99);
    assert!(!page.status.is_empty() && !page.plan.own());
    page.picking = Some(plan::Slot { rally: 0, leg: 0, at: Some(0) });
    page.pick_stage(27);
    assert!(page.plan.own());
    // Only the first model in the field.
    page.left_out = vec!["rsf-2".into(), "rsf-3".into()];
    page.start_event(&here, "tester", &cars, &stages);
    let event = &page.run.as_ref().unwrap().event;
    assert_eq!(event.rallies[0].stages[0].stage.name, "Pirka Menoko");
    assert_eq!(event.rallies[0].stages.len(), 6);
    assert!(event.field.iter().all(|e| e.car == "rsf-1"));
    std::fs::remove_dir_all(&here).unwrap();
}
