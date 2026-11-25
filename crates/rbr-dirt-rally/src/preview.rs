//! The preview picture of a converted stage, for the launcher's stage card: DiRT Rally's own menu pictures are not in
//! the install, so the stage is rendered by ReRBR's `viewer` (`--camera --screenshot`) and written as the stage
//! folder's `preview.png`, named in `track.ini`'s `splashscreen` (the Assetto Corsa importer's convention, so
//! `rbr_render::stage_picture::btb_picture` finds it).
//!
//! The viewpoint ([`viewpoints`]) is chosen from `driveline.ini` alone: the tightest bend of the middle of the course
//! (the second tightest on a reverse route, so a pair differs; the next one when the top third of the picture is near
//! black), seen from the road [`BACK`] m before its apex and [`HEIGHT`] m up, looking at the apex.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The picture's file name in the stage folder.
pub const FILE: &str = "preview.png";
/// 16:9, what the launcher's cards crop to.
const SIZE: &str = "1920x1080";
/// Give the viewer up after this long (a stage loads in under a minute).
const TIMEOUT: Duration = Duration::from_secs(600);
/// A bend's turn is measured between the headings this far (m) before and after the point.
const WINDOW: f64 = 40.0;
/// The share of the course at each end left out (the start line and the finish).
const MARGIN: f64 = 0.08;
/// How far before the apex (m, along the road) and how far above it the eye stands.
const BACK: f64 = 100.0;
/// Bends taken are this far apart (m along the course).
const APART: f64 = 300.0;
/// Candidates tried before the least dark is kept, and the dark share ([`dark_share`]) that is too much.
const TRIES: usize = 5;
const DARK: f64 = 0.02;
const HEIGHT: f64 = 28.0;

/// A driveline point: scene position `(x, height, z)` and the distance along the stage (m).
type Point = ([f64; 3], f64);

/// `driveline.ini`'s points in the scene frame, as `btb-shots.py` reads them.
pub fn driveline(text: &str) -> Vec<Point> {
    text.lines()
        .filter_map(|line| {
            let v: Vec<f64> = line
                .trim()
                .strip_prefix('K')?
                .split_once('=')?
                .1
                .split(',')
                .map(|n| n.trim().parse().ok())
                .collect::<Option<_>>()?;
            (v.len() > 6).then(|| ([v[0], v[2], v[1]], v[6]))
        })
        .collect()
}

/// The eye and the point it looks at for each of the course's bends, tightest first: a bend's turn is the change of
/// plan heading over [`WINDOW`] m either side of a point outside the [`MARGIN`] of the course's ends; the points are taken
/// by turn (the lower distance on a tie), none within [`APART`] m along the course of one taken. The eye stands on the
/// road [`BACK`] m before the apex, [`HEIGHT`] m above it, the apex is what it looks at.
pub fn viewpoints(points: &[Point]) -> Vec<([f64; 3], [f64; 3])> {
    let Some(length) = points.last().map(|p| p.1) else { return Vec::new() };
    let at = |s: f64| points.partition_point(|p| p.1 < s).min(points.len() - 1);
    let heading = |a: usize, b: usize| (points[b].0[2] - points[a].0[2]).atan2(points[b].0[0] - points[a].0[0]);
    let turn = |i: usize| {
        let (before, after) = (at(points[i].1 - WINDOW), at(points[i].1 + WINDOW));
        let d = (heading(i, after) - heading(before, i)).abs() % std::f64::consts::TAU;
        d.min(std::f64::consts::TAU - d)
    };
    let mut ranked: Vec<(usize, f64)> =
        (at(length * MARGIN)..at(length * (1.0 - MARGIN))).map(|i| (i, turn(i))).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut taken: Vec<usize> = Vec::new();
    for (i, _) in ranked {
        if taken.iter().all(|&j| (points[i].1 - points[j].1).abs() >= APART) {
            taken.push(i);
        }
    }
    taken
        .into_iter()
        .map(|i| {
            let from = points[at((points[i].1 - BACK).max(0.0))].0;
            ([from[0], from[1] + HEIGHT, from[2]], points[i].0)
        })
        .collect()
}

/// The share of the picture's top third that is near black (luma under 40): terrain missing past the stage's edge.
fn dark_share(png: &Path) -> Result<f64, String> {
    let file = std::fs::File::open(png).map_err(|e| format!("{}: {e}", png.display()))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().map_err(|e| e.to_string())?;
    let mut data = vec![0; reader.output_buffer_size().ok_or("picture too large")?];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let channels = info.color_type.samples();
    let row = info.width as usize * channels;
    let top = &data[..row * (info.height as usize / 3)];
    let pixels = top.chunks_exact(channels);
    let count = pixels.len().max(1) as f64;
    let dark = pixels
        .filter(|p| {
            let (r, g, b) = if channels >= 3 { (p[0], p[1], p[2]) } else { (p[0], p[0], p[0]) };
            0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b) < 40.0
        })
        .count();
    Ok(dark as f64 / count)
}

/// The route's number from `dr-conversion.json` (`greece/ghymno_rally_01/route_1`): odd is the reverse.
fn route_number(folder: &Path) -> Option<u32> {
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join(crate::stage::CONVERSION_FILE)).ok()?).ok()?;
    json["route"].as_str()?.rsplit("route_").next()?.parse().ok()
}

/// Sets `track.ini`'s `splashscreen` to [`FILE`], leaving every other line as it is.
pub fn name_in_ini(ini: &str) -> String {
    let line = format!("splashscreen={FILE}");
    let mut done = false;
    let mut out: Vec<String> = ini
        .lines()
        .map(|l| {
            let is = l.trim_start().to_ascii_lowercase().starts_with("splashscreen") && l.contains('=');
            if is {
                done = true;
            }
            if is {
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !done {
        let at = out.iter().position(|l| l.trim().eq_ignore_ascii_case("[info]")).map_or(out.len(), |i| i + 1);
        out.insert(at, line);
    }
    out.join("\n") + "\n"
}

/// The `weather_good` of `track.ini`: the stage's own sky.
fn weather(ini: &str) -> Option<&str> {
    ini.lines().find_map(|l| l.trim().strip_prefix("weather_good=")).map(str::trim).filter(|w| !w.is_empty())
}

/// Renders `folder`'s preview with `viewer` (maps from `maps`, the install's `Maps` folder), writes `<folder>/preview.png`
/// and names it in `track.ini`; the picture's path. `scratch` holds the prepared stage and the first render.
pub fn render(folder: &Path, viewer: &Path, maps: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let read = |name: &str| {
        std::fs::read_to_string(folder.join(name)).map_err(|e| format!("{}: {e}", folder.join(name).display()))
    };
    let bends = viewpoints(&driveline(&read("driveline.ini")?));
    let ini = read("track.ini")?;
    let _ = std::fs::remove_dir_all(scratch);
    let stage = rbr_ac_tracks::btb::stage::prepare(folder, &scratch.join("prepared"), Some(maps))?;
    // A reverse route starts at the second bend, so it does not show its forward's.
    let first = usize::from(route_number(folder).is_some_and(|n| n % 2 == 1) && bends.len() > 1);
    let mut best: Option<(f64, PathBuf)> = None;
    for (n, (eye, target)) in bends.iter().skip(first).take(TRIES).enumerate() {
        let png = scratch.join(format!("try-{n}.png"));
        shoot(viewer, &stage, &png, *eye, *target, weather(&ini))?;
        let dark = dark_share(&png)?;
        if best.as_ref().is_none_or(|b| dark < b.0) {
            best = Some((dark, png));
        }
        if dark <= DARK {
            break;
        }
    }
    let (_, png) = best.ok_or("driveline too short")?;
    let target = folder.join(FILE);
    std::fs::copy(&png, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    std::fs::write(folder.join("track.ini"), name_in_ini(&ini)).map_err(|e| format!("track.ini: {e}"))?;
    let _ = std::fs::remove_dir_all(scratch);
    Ok(target)
}

/// One screenshot of `stage` from `eye` looking at `target`, in `sky`, to `png`.
fn shoot(
    viewer: &Path,
    stage: &str,
    png: &Path,
    eye: [f64; 3],
    target: [f64; 3],
    sky: Option<&str>,
) -> Result<(), String> {
    let look = [target[0] - eye[0], target[1] - eye[1] - 1.0, target[2] - eye[2]];
    let camera: Vec<String> = eye.iter().chain(&look).map(|v| format!("{v:.3}")).collect();
    let mut command = Command::new(viewer);
    command
        .arg(stage)
        .args(["--camera", &camera.join(","), "--size", SIZE, "--clock", "13:00", "--no-hud"])
        .args(["--enhance", "all,grass", "--detail", "ultra", "--anti-aliasing", "taa", "--reflections", "high"])
        .arg("--screenshot")
        .arg(png);
    if let Some(sky) = sky {
        command.args(["--sky", sky]);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", viewer.display()))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the viewer took too long".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if !status.success() || !png.is_file() {
        return Err(format!("the viewer failed ({status})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splashscreen_is_set_or_added_and_nothing_else_changes() {
        assert_eq!(name_in_ini("[INFO]\nname=A\n"), "[INFO]\nsplashscreen=preview.png\nname=A\n");
        assert_eq!(name_in_ini("[INFO]\nsplashscreen=old.jpg\nname=A"), "[INFO]\nsplashscreen=preview.png\nname=A\n");
    }

    #[test]
    fn the_tightest_bend_is_chosen_and_seen_from_before_it() {
        // Straight east for 1 km, a right-angle bend, a hairpin at 2 km, straight again.
        let mut points = Vec::new();
        let (mut x, mut z, mut heading) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..1600 {
            let s = i as f64 * 2.5;
            points.push(([x, 0.0, z], s));
            heading += match i {
                400..=419 => std::f64::consts::FRAC_PI_2 / 20.0,
                800..=819 => std::f64::consts::PI / 20.0,
                _ => 0.0,
            };
            x += 2.5 * heading.cos();
            z += 2.5 * heading.sin();
        }
        let bends = viewpoints(&points);
        let (eye, target) = bends[0];
        // The next bend is 300 m or more along the course from the first.
        assert!(bends.len() > 1);
        let apex = points[810].0;
        assert!((target[0] - apex[0]).hypot(target[2] - apex[2]) < 60.0, "{target:?}");
        assert!((eye[0] - target[0]).hypot(eye[2] - target[2]) > 40.0 && eye[1] == HEIGHT);
    }
}
