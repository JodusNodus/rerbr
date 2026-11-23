//! What the log answers: the best run of each stage, class or car and road under one physics, a key's runs, and the best
//! for a stage and a class.

use std::collections::HashMap;

use rbr_recording::physics::PhysicsId;

use super::{Road, Run};

/// What a best is the best of: a car class, or one car (its key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Group {
    Class(String),
    Car(String),
}

impl Group {
    fn of(&self, run: &Run) -> bool {
        match self {
            Group::Class(class) => run.class == *class,
            Group::Car(key) => run.car_key == *key,
        }
    }
}

/// A best's identity: the stage, the class or car, and the road (the physics is the view's).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub stage: u32,
    pub group: Group,
    pub road: Road,
}

impl Key {
    pub fn of_run(&self, run: &Run) -> bool {
        run.stage == self.stage && run.road == self.road && self.group.of(run)
    }
}

/// The best run of a key, and how many runs it has.
#[derive(Debug, Clone, PartialEq)]
pub struct Best<'a> {
    pub key: Key,
    pub run: &'a Run,
    pub runs: usize,
}

/// Whether `a` is a better run than `b`: faster, or as fast and earlier.
fn better(a: &Run, b: &Run) -> bool {
    (a.time, a.recorded) < (b.time, b.recorded)
}

/// The best run of each (stage, group, road) under `physics`; `group` says what a run counts toward.
fn bests<'a>(runs: &'a [Run], physics: &PhysicsId, group: impl Fn(&Run) -> Group) -> Vec<Best<'a>> {
    let mut found: HashMap<Key, Best<'a>> = HashMap::new();
    for run in runs.iter().filter(|r| r.physics == *physics) {
        let key = Key { stage: run.stage, group: group(run), road: run.road };
        match found.get_mut(&key) {
            Some(best) => {
                best.runs += 1;
                if better(run, best.run) {
                    best.run = run;
                }
            }
            None => _ = found.insert(key.clone(), Best { key, run, runs: 1 }),
        }
    }
    let mut all: Vec<_> = found.into_values().collect();
    all.sort_by(|a, b| (a.run.recorded, &a.run.replay).cmp(&(b.run.recorded, &b.run.replay)));
    all
}

pub fn best_per_class<'a>(runs: &'a [Run], physics: &PhysicsId) -> Vec<Best<'a>> {
    bests(runs, physics, |r| Group::Class(r.class.clone()))
}

pub fn best_per_car<'a>(runs: &'a [Run], physics: &PhysicsId) -> Vec<Best<'a>> {
    bests(runs, physics, |r| Group::Car(r.car_key.clone()))
}

/// The runs of `key` under `physics`, best first.
pub fn runs_of<'a>(runs: &'a [Run], physics: &PhysicsId, key: &Key) -> Vec<&'a Run> {
    let mut found: Vec<&Run> = runs.iter().filter(|r| r.physics == *physics && key.of_run(r)).collect();
    found.sort_by_key(|r| (r.time, r.recorded));
    found
}

/// The best run on `stage` in `class` under `physics`, on `road` when given, else on any.
pub fn best_for<'a>(
    runs: &'a [Run],
    physics: &PhysicsId,
    stage: u32,
    class: &str,
    road: Option<Road>,
) -> Option<&'a Run> {
    let mut found = runs.iter().filter(|r| {
        r.physics == *physics && r.stage == stage && r.class == class && road.is_none_or(|road| r.road == road)
    });
    let first = found.next()?;
    Some(found.fold(first, |best, run| if better(run, best) { run } else { best }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(recorded: u64, stage: u32, car: &str, class: &str, road: Road, physics: &str, time: u32) -> Run {
        Run {
            recorded,
            stage,
            stage_name: format!("S{stage}"),
            car_key: car.into(),
            car_name: car.into(),
            class: class.into(),
            road,
            physics: PhysicsId::parse(physics).unwrap(),
            time,
            penalties: 0,
            splits: vec![],
            replay: format!("{recorded}.rrp"),
        }
    }

    fn log() -> Vec<Run> {
        let (dry, wet) = (Road::Dry, Road::Wet);
        vec![
            run(1, 5, "a", "R5", dry, "ngp@7.4.778", 300_000),
            run(2, 5, "b", "R5", dry, "ngp@7.4.778", 290_000),
            run(3, 5, "a", "R5", dry, "ngp@7.4.778", 295_000),
            run(4, 5, "a", "R5", wet, "ngp@7.4.778", 310_000),
            run(5, 5, "a", "R5", dry, "experimental@0.1.0", 280_000),
            run(6, 6, "c", "Rally 2", dry, "ngp@7.4.778", 100_000),
            run(7, 5, "b", "R5", dry, "ngp@7.4.778", 290_000),
        ]
    }

    #[test]
    fn bests_are_kept_per_stage_group_road_and_physics() {
        let (runs, ngp) = (log(), PhysicsId::parse("ngp@7.4.778").unwrap());
        let class = best_per_class(&runs, &ngp);
        let summary: Vec<_> = class.iter().map(|b| (b.key.stage, b.key.road, b.run.time, b.runs)).collect();
        assert_eq!(
            summary,
            [(5, Road::Dry, 290_000, 4), (5, Road::Wet, 310_000, 1), (6, Road::Dry, 100_000, 1)],
            "a tie goes to the earlier run"
        );
        assert_eq!(class[0].run.recorded, 2);
        let car: Vec<_> = best_per_car(&runs, &ngp).iter().map(|b| (b.key.group.clone(), b.run.time, b.runs)).collect();
        assert_eq!(car.len(), 4);
        assert!(car.contains(&(Group::Car("a".into()), 295_000, 2)));
        assert!(car.contains(&(Group::Car("b".into()), 290_000, 2)));
        let other = PhysicsId::parse("experimental@0.1.0").unwrap();
        assert_eq!(best_per_class(&runs, &other).len(), 1);
        assert!(best_per_class(&runs, &PhysicsId::parse("ngp-brush@0.1.0").unwrap()).is_empty());
    }

    #[test]
    fn a_keys_runs_come_best_first_and_the_best_can_ignore_the_road() {
        let (runs, ngp) = (log(), PhysicsId::parse("ngp@7.4.778").unwrap());
        let key = Key { stage: 5, group: Group::Car("a".into()), road: Road::Dry };
        assert_eq!(runs_of(&runs, &ngp, &key).iter().map(|r| r.recorded).collect::<Vec<_>>(), [3, 1]);
        let key = Key { group: Group::Class("R5".into()), ..key };
        assert_eq!(runs_of(&runs, &ngp, &key).iter().map(|r| r.recorded).collect::<Vec<_>>(), [2, 7, 3, 1]);
        assert_eq!(best_for(&runs, &ngp, 5, "R5", Some(Road::Wet)).unwrap().time, 310_000);
        assert_eq!(best_for(&runs, &ngp, 5, "R5", None).unwrap().time, 290_000);
        assert!(best_for(&runs, &ngp, 5, "R5", Some(Road::Damp)).is_none());
        assert!(best_for(&runs, &ngp, 5, "Rally 2", None).is_none());
    }
}
