//! DiRT Rally stages of the player's own install, converted into `tracks/` by `rbr_dirt_rally`
//! (`notes/dirt-rally.md`). Not a [`super::kind::Kind`]: nothing is recognised in a dropped file. The player chooses the
//! game folder and the routes in the picker (`dirt_page`), and this runs them as one Add run, one stage after the other.
//! A stage appears whole or not at all, and a cancel leaves nothing of the one it stopped. Each stage keeps the
//! converter's `dr-conversion.json`, which marks it as an import in the Stages list ([`crate::btb::Btb::import`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rbr_btb_writer::control::Control;
use rbr_dirt_rally::import::{self, Options};
use rbr_dirt_rally::install::Install;

use super::{Goto, Outcome, Refresh};
use crate::job::{Progress, Report, Temp};

pub const KIND: &str = "DiRT Rally stage";

/// A route the player chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// `location/area/route_<n>`.
    pub id: String,
    /// The stage folder it is written to.
    pub folder: String,
    /// What the stage is called in the notices.
    pub title: String,
}

/// What renders a stage's picture after its conversion: the viewer, the game's `Maps` folder and the environment the
/// viewer drives with.
pub struct Previews {
    pub viewer: PathBuf,
    pub maps: PathBuf,
    pub env: Vec<(&'static str, PathBuf)>,
}

/// Converts `picks` of the install at `data` into the player's `home`, in order, and says what happened to each. A
/// stage's picture is made after it when `previews` is given; failing to make it does not fail the stage.
pub fn add(
    home: &Path,
    data: &Path,
    picks: &[Pick],
    previews: Option<&Previews>,
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Vec<Outcome> {
    let tracks = home.join(rbr_btb::player::TRACKS);
    let install = match Install::at(data) {
        Ok(install) => install,
        Err(e) => return vec![Outcome::warning(&e)],
    };
    let mut outcomes = Vec::new();
    for (n, pick) in picks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let left = picks.len() - n;
            outcomes.push(Outcome::warning(&format!("Cancelled: {left} {} not added", plural(left))));
            break;
        }
        let subject = format!("{} of {}: {}", n + 1, picks.len(), pick.title);
        report(&Progress { step: "Adding DiRT Rally stage", subject, ..Progress::default() });
        outcomes.push(add_one(&install, &tracks, pick, previews, report, cancel).refreshing(Refresh::Stages));
    }
    outcomes
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        "stage"
    } else {
        "stages"
    }
}

fn add_one(
    install: &Install,
    tracks: &Path,
    pick: &Pick,
    previews: Option<&Previews>,
    report: Report<'_>,
    cancel: &AtomicBool,
) -> Outcome {
    if tracks.join(&pick.folder).exists() {
        return Outcome::failed(&pick.title, KIND, "already added");
    }
    if let Err(e) = std::fs::create_dir_all(tracks) {
        return Outcome::failed(&pick.title, KIND, format!("{}: {e}", tracks.display()));
    }
    if let Err(e) = convert(install, pick, tracks, cancel) {
        return Outcome::failed(&pick.title, KIND, format!("can't be converted: {e}"));
    }
    let picture = previews.filter(|_| !cancel.load(Ordering::Relaxed)).map(|previews| {
        report(&Progress { step: "Making the picture of", subject: pick.title.clone(), ..Progress::default() });
        picture(&tracks.join(&pick.folder), previews)
    });
    let note = match picture {
        Some(Err(e)) => {
            eprintln!("launcher: no picture for {}: {e}", pick.folder);
            " (its picture couldn't be made)"
        }
        _ => "",
    };
    let text = format!("Added {} ({KIND}): it's in Stages{note}", pick.title);
    Outcome::said(&pick.title, KIND, text, Goto::Stage(pick.folder.clone()))
}

/// Converts the route into `tracks`, stopping when `cancel` is set: the conversion runs on a thread of its own while
/// this one watches the flag.
fn convert(install: &Install, pick: &Pick, tracks: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut options = Options::new(Control::new(|_| {}, stop.clone()));
    options.out = Some(tracks.to_owned());
    options.name = Some(pick.folder.clone());
    std::thread::scope(|scope| {
        let work = scope.spawn(|| import::import(install, &pick.id, &options));
        while !work.is_finished() {
            if cancel.load(Ordering::Relaxed) {
                stop.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        work.join().unwrap_or_else(|_| Err("it stopped unexpectedly".into()))
    })
    .map(|_| ())
}

/// The stage's preview picture, rendered by the viewer in a scratch folder that is removed after.
fn picture(folder: &Path, previews: &Previews) -> Result<(), String> {
    let scratch = Temp::create(&std::env::temp_dir(), "rerbr-dirt-picture")?;
    rbr_dirt_rally::preview::render(folder, &previews.viewer, &previews.maps, &scratch.0.join("render"), &previews.env)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    fn pick(id: &str, folder: &str, title: &str) -> Pick {
        Pick { id: id.into(), folder: folder.into(), title: title.into() }
    }

    fn lines(done: &[Outcome]) -> Vec<String> {
        done.iter().map(|o| format!("{} {}", if o.ok() { "ok" } else { "no" }, o.line())).collect()
    }

    /// An install of two empty routes: enough to list, not to convert.
    fn hollow_install(dir: &Path) -> PathBuf {
        let data = dir.join("game");
        for route in ["greece/ghymno_rally_01/route_0", "greece/ghymno_rally_01/route_1"] {
            fs::create_dir_all(data.join("tracks/locations").join(route)).unwrap();
        }
        data
    }

    #[test]
    fn a_folder_without_stages_is_one_warning_and_a_failed_stage_leaves_nothing() {
        let dir = crate::test_dir("content-dirt-fails");
        let home = dir.join("home");
        let off = |_: &Progress| {};
        let done = add(&home, &dir, &[pick("a/b/route_0", "DR_a", "A")], None, &off, &AtomicBool::new(false));
        assert_eq!(done.len(), 1);
        assert!(!done[0].ok() && done[0].line().contains("no DiRT Rally data"), "{}", done[0].line());
        let data = hollow_install(&dir);
        let missing = pick("greece/nowhere_rally_01/route_0", "DR_greece_nowhere_rally_01_route_0", "Nowhere");
        let empty = pick("greece/ghymno_rally_01/route_0", "DR_greece_ghymno_rally_01_route_0", "Ghymno");
        let done = add(&home, &data, &[missing, empty], None, &off, &AtomicBool::new(false));
        let got = lines(&done);
        assert!(got[0].starts_with("no Couldn't add Nowhere: can't be converted: "), "{got:?}");
        assert!(got[1].starts_with("no Couldn't add Ghymno: can't be converted: "), "{got:?}");
        assert!(done.iter().all(|o| o.refresh == Refresh::Stages));
        assert!(fs::read_dir(home.join("tracks")).unwrap().next().is_none(), "no folder, no staging left");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stage_already_there_is_not_converted_again_and_progress_names_each_stage() {
        let dir = crate::test_dir("content-dirt-again");
        let (home, data) = (dir.join("home"), hollow_install(&dir));
        fs::create_dir_all(home.join("tracks/DR_one")).unwrap();
        let seen = Mutex::new(Vec::new());
        let on = |p: &Progress| seen.lock().unwrap().push(p.line());
        let picks = [pick("greece/ghymno_rally_01/route_0", "DR_one", "One"), pick("x/y/route_0", "DR_two", "Two")];
        let done = add(&home, &data, &picks, None, &on, &AtomicBool::new(false));
        assert_eq!(lines(&done)[0], "no Couldn't add One: already added");
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen, ["Adding DiRT Rally stage 1 of 2: One…", "Adding DiRT Rally stage 2 of 2: Two…"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancel_stops_before_the_next_stage_and_says_how_many_were_left() {
        let dir = crate::test_dir("content-dirt-cancel");
        let (home, data) = (dir.join("home"), hollow_install(&dir));
        let picks = [pick("a/b/route_0", "DR_a", "A"), pick("a/b/route_1", "DR_b", "B")];
        let done = add(&home, &data, &picks, None, &|_| {}, &AtomicBool::new(true));
        assert_eq!(lines(&done), ["no Cancelled: 2 stages not added"]);
        assert!(!home.join("tracks").exists() || fs::read_dir(home.join("tracks")).unwrap().next().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Over the install (`RBR_DIRT_RALLY`, the game folder; skipped when unset): Ghymno 1's first half converts into a
    /// temp home, lists as a DiRT Rally import, and the same route again is "already added". With `RBR_VIEWER` (a
    /// `viewer` program) and `RBR_DATA` too, its picture is made as the launcher makes it.
    #[test]
    fn a_real_route_adds_and_lists() {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY").map(PathBuf::from) else {
            eprintln!("skipping: RBR_DIRT_RALLY not set");
            return;
        };
        let dir = crate::test_dir("content-dirt-real");
        let home = dir.join("home");
        let install = Install::at(&game).unwrap();
        let listed = rbr_dirt_rally::listing::list(&install);
        let route = listed.iter().find(|l| l.id == "greece/ghymno_rally_01/route_2").expect("Ghymno 1 first half");
        let picks = [pick(&route.id, &route.folder, &route.title)];
        let previews = std::env::var_os("RBR_VIEWER").map(|viewer| Previews {
            viewer: viewer.into(),
            maps: Path::new(&std::env::var_os("RBR_DATA").expect("RBR_VIEWER needs RBR_DATA")).join("Maps"),
            env: ["RBR_DATA", "RBR_PATCH", "RBR_RSF", "RBR_NGP_DLL"]
                .into_iter()
                .filter_map(|key| std::env::var_os(key).map(|value| (key, PathBuf::from(value))))
                .collect(),
        });
        let done = add(&home, &install.data, &picks, previews.as_ref(), &|_| {}, &AtomicBool::new(false));
        done.iter().for_each(|o| eprintln!("{}", o.line()));
        assert!(done[0].ok(), "{}", done[0].line());
        if previews.is_some() {
            assert_eq!(done[0].line(), format!("Added {} ({KIND}): it's in Stages", route.title));
            assert!(home.join("tracks").join(&route.folder).join(rbr_dirt_rally::preview::FILE).is_file());
        }
        assert_eq!(done[0].goto, Some(Goto::Stage(route.folder.clone())));
        let tracks = home.join("tracks");
        assert!(tracks.join(&route.folder).join(rbr_dirt_rally::stage::CONVERSION_FILE).is_file());
        let maps = crate::btb::folder_maps(&tracks, |_| false);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].btb().unwrap().import, Some(crate::btb::Import::DirtRally));
        let again = add(&home, &install.data, &picks, None, &|_| {}, &AtomicBool::new(false));
        assert_eq!(again[0].result, Err("already added".into()), "{again:?}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
