//! The times log: `<player folder>/times.tsv`, one line per finished stage run, kept beside the replays it came from.
//! Line one is `# ReRBR times 1`; each line after is tab separated: recorded (unix seconds), stage number, stage name,
//! car key, car name, class, road word, physics id (`profile@version`), time (s, penalties included), penalties (s),
//! split times (comma separated), the replay's file name. An unreadable line is skipped.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use rbr_recording::physics::PhysicsId;
use rbr_recording::replay;

use super::{class_of, Road};
use crate::catalog::Catalog;

pub const FILE: &str = "times.tsv";
const HEADER: &str = "# ReRBR times 1";

/// One finished run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub recorded: u64,
    pub stage: u32,
    pub stage_name: String,
    pub car_key: String,
    pub car_name: String,
    pub class: String,
    pub road: Road,
    pub physics: PhysicsId,
    /// Milliseconds, penalties included.
    pub time: u32,
    pub penalties: u32,
    pub splits: Vec<u32>,
    /// The replay's file name in `replays/`; empty for none.
    pub replay: String,
}

/// What names the stages and cars of the runs: the catalog's, by number and by key.
#[derive(Default)]
pub struct Names {
    /// Car key to name and class.
    pub cars: HashMap<String, (String, String)>,
    pub stages: HashMap<u32, String>,
}

impl Names {
    pub fn of(catalog: &Catalog) -> Names {
        Names {
            cars: (catalog.cars.iter())
                .map(|c| (c.key.clone(), (crate::catalog::car_label(c), class_of(c.group.as_deref()))))
                .collect(),
            stages: catalog.maps.iter().map(|m| (m.number, m.name())).collect(),
        }
    }
}

fn clean(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn ms(seconds: f32) -> u32 {
    (f64::from(seconds) * 1000.0).round() as u32
}

fn seconds(ms: u32) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

fn parse_seconds(text: &str) -> Option<u32> {
    let value: f64 = text.parse().ok()?;
    (value.is_finite() && value >= 0.0).then(|| (value * 1000.0).round() as u32)
}

/// The first value after `flag`.
fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].as_str())
}

/// The stage number of a stage path ending `track-<number>_<letter>`.
fn stage_number(path: &str) -> Option<u32> {
    let name = path.rsplit(['/', '\\']).next()?;
    name.strip_prefix("track-")?.split_once('_')?.0.parse().ok()
}

/// The car's key of a drive's arguments: `--car-key`, else (drives before it) the pack folder `--rsf-car` names, else
/// the archive car `--car` of the viewer.
fn car_key(args: &[String]) -> Option<String> {
    let live = args.iter().position(|a| a == "--live").unwrap_or(args.len());
    let viewer = &args[..live];
    value(args, "--car-key")
        .map(str::to_owned)
        .or_else(|| {
            let pack = value(viewer, "--rsf-car")?;
            Some(crate::catalog::pack_key(pack.rsplit(['/', '\\']).next()?))
        })
        .or_else(|| value(viewer, "--car").map(str::to_owned))
}

impl Run {
    /// The line it is kept as.
    pub fn line(&self) -> String {
        let splits: Vec<String> = self.splits.iter().map(|&s| seconds(s)).collect();
        [
            self.recorded.to_string(),
            self.stage.to_string(),
            clean(&self.stage_name),
            clean(&self.car_key),
            clean(&self.car_name),
            clean(&self.class),
            self.road.word().to_owned(),
            self.physics.text(),
            seconds(self.time),
            seconds(self.penalties),
            splits.join(","),
            clean(&self.replay),
        ]
        .join("\t")
    }

    pub fn parse(line: &str) -> Option<Run> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 12 {
            return None;
        }
        Some(Run {
            recorded: f[0].parse().ok()?,
            stage: f[1].parse().ok()?,
            stage_name: f[2].to_owned(),
            car_key: f[3].to_owned(),
            car_name: f[4].to_owned(),
            class: f[5].to_owned(),
            road: Road::parse(f[6])?,
            physics: PhysicsId::parse(f[7])?,
            time: parse_seconds(f[8])?,
            penalties: parse_seconds(f[9])?,
            splits: f[10].split(',').filter(|s| !s.is_empty()).map(parse_seconds).collect::<Option<_>>()?,
            replay: f[11].to_owned(),
        })
    }

    /// The run a saved drive made, when it qualifies: finished and not retired, a live drive (not laps or a lesson),
    /// on a stage and a road it names. `physics` is what it stepped; none: the profile its command names, from before
    /// the profiles had versions.
    pub fn from_replay(path: &Path, names: &Names, physics: Option<&PhysicsId>) -> Option<Run> {
        let args = replay::read_args(BufReader::new(File::open(path).ok()?)).ok()?;
        let result = replay::read_result(path).ok()?;
        if !result.finished || result.retired || !result.race_time.is_finite() || result.race_time < 0.0 {
            return None;
        }
        let live = args.iter().position(|a| a == "--live")?;
        if args.iter().any(|a| a == "--laps" || a == "--school") {
            return None;
        }
        let (viewer, command) = args.split_at(live);
        let stage = stage_number(value(command, "--stage")?)?;
        let road = Road::parse(value(viewer, "--weather")?)?;
        let physics = match physics {
            Some(id) => id.clone(),
            None => PhysicsId::unversioned(value(command, "--physics").unwrap_or("ngp"))?,
        };
        let key = car_key(&args)?;
        let (car_name, class) = names.cars.get(&key).cloned().unwrap_or_else(|| (key.clone(), "Other".to_owned()));
        let title_stage = value(viewer, "--title").and_then(|t| t.split(" · ").next()).filter(|s| !s.is_empty());
        let stage_name = names
            .stages
            .get(&stage)
            .map(String::as_str)
            .or(title_stage)
            .map_or_else(|| format!("Stage {stage}"), str::to_owned);
        let recorded = crate::replay_page::recorded_time(path)
            .or_else(|| path.metadata().and_then(|m| m.modified()).ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        Some(Run {
            recorded,
            stage,
            stage_name,
            car_key: key,
            car_name,
            class,
            road,
            physics,
            time: ms(result.race_time),
            penalties: ms(result.penalties.max(0.0)),
            splits: result.splits.iter().filter_map(|s| s.time).filter(|t| t.is_finite()).map(ms).collect(),
            replay: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        })
    }
}

/// The runs in the log at `home`; none when there is no log.
pub fn read(home: &Path) -> Vec<Run> {
    let text = fs::read(home.join(FILE)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    text.lines().filter(|l| !l.starts_with('#')).filter_map(Run::parse).collect()
}

/// Writes the whole log, by a temporary file renamed over it.
pub fn write(home: &Path, runs: &[Run]) -> io::Result<()> {
    let mut text = format!("{HEADER}\n");
    for run in runs {
        text.push_str(&run.line());
        text.push('\n');
    }
    let temp = home.join(format!("{FILE}.tmp"));
    fs::write(&temp, text)?;
    fs::rename(temp, home.join(FILE))
}

/// Builds the log from every replay in `replays/` (under the physics of before the versions), oldest first, leaving
/// out `skip`; the runs.
pub fn backfill(home: &Path, names: &Names, skip: Option<&Path>) -> io::Result<Vec<Run>> {
    let mut runs: Vec<Run> = match fs::read_dir(home.join("replays")) {
        Ok(files) => files
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "rrp") && Some(p.as_path()) != skip)
            .filter_map(|p| Run::from_replay(&p, names, None))
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    runs.sort_by(|a, b| a.recorded.cmp(&b.recorded).then_with(|| a.replay.cmp(&b.replay)));
    write(home, &runs)?;
    Ok(runs)
}

/// Adds `run` to the log (made first from the replays when there is none, without `run`'s).
pub fn append(home: &Path, names: &Names, run: &Run) -> io::Result<()> {
    if !home.join(FILE).exists() {
        backfill(home, names, Some(&home.join("replays").join(&run.replay)))?;
    }
    let mut file = fs::OpenOptions::new().append(true).open(home.join(FILE))?;
    writeln!(file, "{}", run.line())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_recording::splits::{Split, Splits};

    /// The runs, building the log from the replays first when there is none.
    fn load(home: &Path, names: &Names) -> Vec<Run> {
        if home.join(FILE).exists() {
            return read(home);
        }
        backfill(home, names, None).unwrap()
    }

    fn names() -> Names {
        Names {
            cars: HashMap::from([("evo".into(), ("Lancer Evo".into(), "Group A".into()))]),
            stages: HashMap::from([(54, "Torsby".to_owned())]),
        }
    }

    fn drive(dir: &Path, stamp: u64, extra: &[&str], result: &Splits) -> std::path::PathBuf {
        let mut args: Vec<String> = ["--weather", "damp", "--car-key", "evo", "--title", "Lilaia · Evo", "--live", "x"]
            .map(str::to_owned)
            .into();
        args.extend(["--stage", "/maps/54-Torsby/track-54_E"].map(str::to_owned));
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        let path = dir.join(format!("{stamp}-track_54_E.rrp"));
        fs::write(&path, replay::header(&args)).unwrap();
        replay::save_result(&path, result).unwrap();
        path
    }

    fn finished() -> Splits {
        let split = |t| Split { time: Some(t), reference: None };
        Splits {
            finished: true,
            race_time: 389.1234,
            penalties: 10.0,
            splits: vec![split(100.5), Split::default(), split(200.25)],
            ..Default::default()
        }
    }

    #[test]
    fn a_run_round_trips_through_its_line_and_bad_lines_are_skipped() {
        let run = Run {
            recorded: 1_700_000_000,
            stage: 54,
            stage_name: "Tors\tby\n".into(),
            car_key: "evo".into(),
            car_name: "Evo".into(),
            class: "Group A".into(),
            road: Road::Wet,
            physics: PhysicsId::current("experimental-brush").unwrap(),
            time: 389_123,
            penalties: 10_000,
            splits: vec![100_500, 200_250],
            replay: "1-a.rrp".into(),
        };
        let parsed = Run::parse(&run.line()).unwrap();
        assert_eq!(parsed, Run { stage_name: "Tors by ".into(), ..run.clone() });
        assert!(run.line().contains("\t389.123\t10.000\t100.500,200.250\t"));
        let none = Run { stage_name: "Torsby".into(), splits: vec![], replay: String::new(), ..run };
        assert_eq!(Run::parse(&none.line()).unwrap(), none);
        assert_eq!(Run::parse("garbage"), None);
        assert_eq!(Run::parse(&none.line().replace("wet", "snow")), None);
        let home = crate::test_dir("times-log");
        fs::write(home.join(FILE), format!("{HEADER}\nbad line\n{}\n\n", none.line())).unwrap();
        assert_eq!(read(&home), vec![none]);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn only_a_finished_live_stage_drive_with_a_road_qualifies() {
        let home = crate::test_dir("times-qualify");
        let r = finished();
        let ok = drive(&home, 1_700_000_001, &["--physics", "experimental"], &r);
        let run = Run::from_replay(&ok, &names(), None).unwrap();
        assert_eq!(
            (run.stage, run.stage_name.as_str(), run.car_name.as_str(), run.class.as_str()),
            (54, "Torsby", "Lancer Evo", "Group A")
        );
        assert_eq!((run.road, run.time, run.penalties, run.recorded), (Road::Damp, 389_123, 10_000, 1_700_000_001));
        assert_eq!(run.splits, vec![100_500, 200_250]);
        assert_eq!(run.physics.text(), "experimental@unversioned");
        assert_eq!(run.replay, "1700000001-track_54_E.rrp");
        let now = PhysicsId::current("experimental").unwrap();
        assert_eq!(Run::from_replay(&ok, &names(), Some(&now)).unwrap().physics, now);
        // Without a catalog: the key and the title's stage.
        let bare = Run::from_replay(&ok, &Names::default(), None).unwrap();
        assert_eq!((bare.car_name.as_str(), bare.class.as_str(), bare.stage_name.as_str()), ("evo", "Other", "Lilaia"));
        assert_eq!(bare.physics.text(), "experimental@unversioned");
        let ngp = drive(&home, 2, &[], &r);
        assert_eq!(Run::from_replay(&ngp, &names(), None).unwrap().physics.text(), "ngp@7.4.778");
        for (stamp, extra, result) in [
            (3, &["--laps"][..], r.clone()),
            (4, &["--school"][..], r.clone()),
            (5, &[][..], Splits { retired: true, ..r.clone() }),
            (6, &[][..], Splits { finished: false, ..r.clone() }),
        ] {
            assert!(Run::from_replay(&drive(&home, stamp, extra, &result), &names(), None).is_none(), "{stamp}");
        }
        let no_result = home.join("7-x.rrp");
        fs::write(&no_result, replay::header(&["--live".to_owned()])).unwrap();
        assert!(Run::from_replay(&no_result, &names(), None).is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn the_log_is_built_once_from_the_replays_then_appended_to() {
        let home = crate::test_dir("times-backfill");
        let dir = home.join("replays");
        fs::create_dir_all(&dir).unwrap();
        drive(&dir, 300, &[], &finished());
        drive(&dir, 100, &["--physics", "experimental-brush"], &finished());
        drive(&dir, 200, &[], &Splits { retired: true, ..finished() });
        fs::write(dir.join("junk.rrp"), b"junk").unwrap();
        let runs = load(&home, &names());
        assert_eq!(runs.iter().map(|r| r.recorded).collect::<Vec<_>>(), [100, 300]);
        assert_eq!(read(&home), runs);
        assert!(!home.join("times.tsv.tmp").exists());
        // A later drive is added, and the log is not built again from the replays.
        let fresh = drive(&dir, 400, &[], &finished());
        let run = Run::from_replay(&fresh, &names(), Some(&PhysicsId::current("ngp").unwrap())).unwrap();
        append(&home, &names(), &run).unwrap();
        assert_eq!(load(&home, &names()).len(), 3);
        // The first drive after an upgrade: the others are built from the replays, the new one has its own physics.
        fs::remove_file(home.join(FILE)).unwrap();
        let now = PhysicsId::current("experimental").unwrap();
        let run = Run { physics: now.clone(), ..run };
        append(&home, &names(), &run).unwrap();
        let runs = read(&home);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs.iter().filter(|r| r.physics == now).count(), 1);
        fs::remove_dir_all(home).unwrap();
    }
}
