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
const BACK: f64 = 60.0;
/// Bends taken are this far apart (m along the course).
const APART: f64 = 300.0;
/// The stretch of road (m either side of the apex) that must show, and the share of it that may be hidden.
const ROAD: f64 = 20.0;
const HIDDEN: f64 = 0.5;
/// A road point is seen when the luma's spread over the 9 by 9 pixels around it is at most this (foliage is busier).
const SMOOTH: f64 = 16.0;
/// The camera's horizontal field of view (degrees) and the picture's size.
const FOV: f64 = 50.0;
const WIDTH: f64 = 1920.0;
const HEIGHT_PX: f64 = 1080.0;
/// Candidates tried before the least dark is kept, and the dark share ([`dark_share`]) that is too much.
const TRIES: usize = 5;
const DARK: f64 = 0.02;
const HEIGHT: f64 = 14.0;

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

/// A bend to shoot: the eye, the apex it looks at and the road's centre line [`ROAD`] m either side of the apex.
#[derive(Debug, Clone)]
pub struct Bend {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub road: Vec<[f64; 3]>,
}

/// The eye and the point it looks at for each of the course's bends, tightest first: a bend's turn is the change of
/// plan heading over [`WINDOW`] m either side of a point outside the [`MARGIN`] of the course's ends; the points are taken
/// by turn (the lower distance on a tie), none within [`APART`] m along the course of one taken. The eye stands on the
/// road [`BACK`] m before the apex, [`HEIGHT`] m above it, the apex is what it looks at.
pub fn viewpoints(points: &[Point]) -> Vec<Bend> {
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
            let s = points[i].1;
            let road = (at((s - ROAD).max(0.0))..=at(s + ROAD)).map(|k| points[k].0).collect();
            Bend { eye: [from[0], from[1] + HEIGHT, from[2]], target: points[i].0, road }
        })
        .collect()
}

/// What the picture shows of a bend: the share of its top third that is dark (luma under 40: terrain missing past the
/// stage's edge, deep shade) and the share of the road points ([`Bend::road`], projected through the camera) that are
/// hidden, off the picture or under foliage (a busy 9 by 9 patch, [`SMOOTH`]). The viewer has no depth output and no
/// switch to leave the trees out, so the road is judged by how smooth the picture is there.
fn judge(png: &Path, bend: &Bend) -> Result<(f64, f64), String> {
    let file = std::fs::File::open(png).map_err(|e| format!("{}: {e}", png.display()))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().map_err(|e| e.to_string())?;
    let mut data = vec![0; reader.output_buffer_size().ok_or("picture too large")?];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let (w, h, channels) = (info.width as usize, info.height as usize, info.color_type.samples());
    let rgb = |x: usize, y: usize| {
        let p = &data[(y * w + x) * channels..];
        if channels >= 3 {
            [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
        } else {
            [f64::from(p[0]); 3]
        }
    };
    let top = w * (h / 3);
    let dark = (0..top)
        .filter(|&i| {
            let [r, g, b] = rgb(i % w, i / w);
            0.2126 * r + 0.7152 * g + 0.0722 * b < 40.0
        })
        .count();
    let (fx, fy, fz) = {
        let f = [bend.target[0] - bend.eye[0], bend.target[1] - bend.eye[1] - 1.0, bend.target[2] - bend.eye[2]];
        let n = f.iter().map(|v| v * v).sum::<f64>().sqrt();
        (f[0] / n, f[1] / n, f[2] / n)
    };
    let horizontal = fx.hypot(fz);
    let right = [fz / horizontal, 0.0, -fx / horizontal];
    let up = [fy * right[2] - fz * right[1], fz * right[0] - fx * right[2], fx * right[1] - fy * right[0]];
    let tan = (FOV.to_radians() / 2.0).tan();
    let seen = bend
        .road
        .iter()
        .filter(|p| {
            let d = [p[0] - bend.eye[0], p[1] - bend.eye[1], p[2] - bend.eye[2]];
            let dot = |v: [f64; 3]| d[0] * v[0] + d[1] * v[1] + d[2] * v[2];
            let depth = dot([fx, fy, fz]);
            if depth < 1.0 {
                return false;
            }
            let x = (dot(right) / depth / tan + 1.0) / 2.0 * WIDTH;
            let y = (1.0 - dot(up) / depth / (tan * WIDTH / HEIGHT_PX)) / 2.0 * HEIGHT_PX;
            if !(2.0..WIDTH - 3.0).contains(&x) || !(2.0..HEIGHT_PX - 3.0).contains(&y) {
                return false;
            }
            let (x, y) = (x.clamp(4.0, WIDTH - 5.0) as usize, y.clamp(4.0, HEIGHT_PX - 5.0) as usize);
            let (mut m, mut sq) = (0.0, 0.0);
            for (dx, dy) in (0..81).map(|k| (k % 9, k / 9)) {
                let c = rgb(x + dx - 4, y + dy - 4);
                let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                m += l / 81.0;
                sq += l * l / 81.0;
            }
            (sq - m * m).max(0.0).sqrt() <= SMOOTH
        })
        .count();
    Ok((dark as f64 / top.max(1) as f64, 1.0 - seen as f64 / bend.road.len().max(1) as f64))
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
/// and names it in `track.ini`; the picture's path. `scratch` holds the prepared stage and the first render, `env` is
/// added to the viewer's environment (the launcher gives it the game folders it drives with).
pub fn render(
    folder: &Path,
    viewer: &Path,
    maps: &Path,
    scratch: &Path,
    env: &[(&str, PathBuf)],
) -> Result<PathBuf, String> {
    let read = |name: &str| {
        std::fs::read_to_string(folder.join(name)).map_err(|e| format!("{}: {e}", folder.join(name).display()))
    };
    let bends = viewpoints(&driveline(&read("driveline.ini")?));
    let ini = read("track.ini")?;
    let _ = std::fs::remove_dir_all(scratch);
    let stage = rbr_btb_writer::stage::prepare(folder, &scratch.join("prepared"), Some(maps))?;
    // A reverse route starts at the second bend, so it does not show its forward's.
    let first = usize::from(route_number(folder).is_some_and(|n| n % 2 == 1) && bends.len() > 1);
    let mut best: Option<(f64, PathBuf)> = None;
    for (n, bend) in bends.iter().skip(first).take(TRIES).enumerate() {
        let png = scratch.join(format!("try-{n}.png"));
        shoot(viewer, env, &stage, &png, bend.eye, bend.target, weather(&ini))?;
        let (dark, hidden) = judge(&png, bend)?;
        if best.as_ref().is_none_or(|b| dark + hidden < b.0) {
            best = Some((dark + hidden, png));
        }
        if dark <= DARK && hidden <= HIDDEN {
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
    env: &[(&str, PathBuf)],
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
        .envs(env.iter().map(|(key, value)| (key, value)))
        .arg(stage)
        .args(["--camera", &camera.join(","), "--size", SIZE, "--fov", "50", "--clock", "13:00", "--no-hud"])
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
        let (eye, target) = (bends[0].eye, bends[0].target);
        // The next bend is 300 m or more along the course from the first.
        assert!(bends.len() > 1);
        let apex = points[810].0;
        assert!((target[0] - apex[0]).hypot(target[2] - apex[2]) < 60.0, "{target:?}");
        assert!((eye[0] - target[0]).hypot(eye[2] - target[2]) > 40.0 && eye[1] == HEIGHT);
    }
}
