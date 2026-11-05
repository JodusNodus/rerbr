//! The events' stage pool: the catalog's stages that can start (retail, RSF, BTB) as `rbr_events::config::StageRef`s,
//! each with its surface, whether it is a community one, and its length from its driveline (the `.trk`'s kind 0x14
//! records, a BTB folder's `driveline.ini`). A full RSF install has hundreds of stages: the drivelines are read on a
//! thread once and kept in the player's folder (`cache/drivelines/<map>.line`, with the source's path, size and time,
//! so a changed stage is read again); a stage whose driveline does not read (RSF-encrypted, missing) is left out. The
//! reference times read the kept driveline back.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use rbr_events::config::{StageRef, Surface};
use rbr_formats::data::Data;
use rbr_formats::{stage_files, trk};
use rbr_gameplay::weather::Season;

use crate::catalog::{self, Catalog, Origin};

/// The kept drivelines' folder in the player's folder.
pub const CACHE: &str = "cache/drivelines";
/// The cache file's first bytes (format 1).
const MAGIC: &[u8; 6] = b"RRDL1\n";

/// Where a stage's driveline is read from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A stage's name (`track-54_O`, or a stage folder's path): its `.trk`.
    Trk(String),
    /// A BTB stage's folder: its `driveline.ini`.
    Btb(PathBuf),
}

/// A stage of the pool before its driveline is read (the length is 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub stage: StageRef,
    pub source: Source,
}

/// The catalog's maps the launcher lists and can start, as pool items: the map number is the key.
pub fn items(catalog: &Catalog, season: &Season) -> Vec<Item> {
    catalog
        .maps
        .iter()
        .filter(|m| catalog::is_listed(m) && m.problem().is_none())
        .filter(|m| catalog.availability(season, m).is_ok())
        .filter_map(|map| {
            let source = match &map.origin {
                Origin::Btb(btb) => Source::Btb(btb.folder.clone()),
                _ => Source::Trk(map.stages.first()?.name.clone()),
            };
            let stage = StageRef {
                key: map.number.to_string(),
                name: map.name(),
                surface: surface(map.surface()),
                community: !matches!(map.origin, Origin::Retail),
                length: 0.0,
            };
            Some(Item { stage, source })
        })
        .collect()
}

/// The game's surface number (0 gravel, 1 tarmac, 2 snow) as the events' surface.
pub fn surface(kind: u32) -> Surface {
    match kind {
        1 => Surface::Tarmac,
        2 => Surface::Snow,
        _ => Surface::Gravel,
    }
}

/// The surface's word.
pub fn surface_word(surface: Surface) -> &'static str {
    match surface {
        Surface::Gravel => "gravel",
        Surface::Tarmac => "tarmac",
        Surface::Snow => "snow",
    }
}

/// The source's file (a `.trk`, or a BTB folder's `driveline.ini`).
fn source_file(data: &Data, source: &Source) -> Option<PathBuf> {
    match source {
        Source::Trk(stage) => stage_files::file(data, stage, "trk"),
        Source::Btb(folder) => stage_files::in_dir(folder, "driveline.ini"),
    }
}

/// What a kept driveline was read from: the file's path, size and modification time.
fn fingerprint(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let time = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{}|{}|{time}", path.display(), meta.len()))
}

/// The driveline's points (m), from the source file.
fn read_driveline(path: &Path, source: &Source) -> Option<Vec<[f32; 3]>> {
    let points: Vec<[f32; 3]> = match source {
        Source::Trk(_) => {
            let bytes = std::fs::read(path).ok().filter(|b| !stage_files::encrypted(b))?;
            let records = trk::parse(&bytes).ok()?.chunks.into_iter().find_map(|c| match c.payload {
                trk::ChunkPayload::DriveLine(d) => Some(d.records),
                _ => None,
            })?;
            records.iter().map(|r| [r.vectors[0].x, r.vectors[0].y, r.vectors[0].z]).collect()
        }
        Source::Btb(folder) => {
            let line = rbr_btb::track::Track::open(folder).ok()?.driveline().ok()?;
            line.points.iter().map(|p| p.position.map(|v| v as f32)).collect()
        }
    };
    (points.len() >= 2).then_some(points)
}

/// The polyline's length (m).
pub fn length(points: &[[f32; 3]]) -> f32 {
    points.windows(2).map(|w| (0..3).map(|k| (w[1][k] - w[0][k]).powi(2)).sum::<f32>().sqrt()).sum()
}

fn cache_file(cache: &Path, key: &str) -> PathBuf {
    cache.join(format!("{key}.line"))
}

/// The kept driveline of `key`, when it was read from `print` (any source with `None`).
fn read_cached(cache: &Path, key: &str, print: Option<&str>) -> Option<Vec<[f32; 3]>> {
    let bytes = std::fs::read(cache_file(cache, key)).ok()?;
    let rest = bytes.strip_prefix(MAGIC.as_slice())?;
    let word = |b: &[u8], at: usize| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize);
    let n = word(rest, 0)?;
    let kept = std::str::from_utf8(rest.get(4..4 + n)?).ok()?;
    if print.is_some_and(|p| p != kept) {
        return None;
    }
    let rest = &rest[4 + n..];
    let count = word(rest, 0)?;
    let floats = rest.get(4..4 + count * 12)?;
    let f = |at: usize| f32::from_le_bytes(floats[at..at + 4].try_into().unwrap());
    Some((0..count).map(|i| [f(i * 12), f(i * 12 + 4), f(i * 12 + 8)]).collect())
}

fn write_cached(cache: &Path, key: &str, print: &str, points: &[[f32; 3]]) -> std::io::Result<()> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend((print.len() as u32).to_le_bytes());
    bytes.extend(print.as_bytes());
    bytes.extend((points.len() as u32).to_le_bytes());
    bytes.extend(points.iter().flatten().flat_map(|v| v.to_le_bytes()));
    std::fs::create_dir_all(cache)?;
    std::fs::write(cache_file(cache, key), bytes)
}

/// The item's driveline, kept or read (and kept); `None` when it does not read.
fn driveline(data: &Data, cache: &Path, item: &Item) -> Option<Vec<[f32; 3]>> {
    let path = source_file(data, &item.source)?;
    let print = fingerprint(&path)?;
    if let Some(points) = read_cached(cache, &item.stage.key, Some(&print)) {
        return Some(points);
    }
    let points = read_driveline(&path, &item.source)?;
    if let Err(e) = write_cached(cache, &item.stage.key, &print, &points) {
        eprintln!("launcher: {}: {e}", cache_file(cache, &item.stage.key).display());
    }
    Some(points)
}

/// The items whose driveline reads, with their lengths; `done` counts the items looked at.
pub fn read(data: &Data, cache: &Path, items: Vec<Item>, done: &AtomicUsize, stop: &AtomicBool) -> Vec<StageRef> {
    let mut stages = Vec::new();
    for item in items {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(points) = driveline(data, cache, &item) {
            stages.push(StageRef { length: length(&points), ..item.stage });
        }
        done.fetch_add(1, Ordering::Relaxed);
    }
    stages
}

/// A stage's reference time (s) for `class` at `wetness` (0 dry to 1 wet), from its kept driveline; a stage without
/// one is taken at 25 m/s.
pub fn reference(cache: &Path, stage: &StageRef, wetness: f32, class: &str) -> f32 {
    match read_cached(cache, &stage.key, None) {
        Some(points) => {
            rbr_events::pace::reference_time(&points, stage.surface, wetness, &rbr_events::pace::class_pace(class))
        }
        None => stage.length / 25.0,
    }
}

/// The pool being read on its thread, then read.
pub enum Pool {
    Reading { done: Arc<AtomicUsize>, total: usize, stop: Arc<AtomicBool>, thread: Option<JoinHandle<Vec<StageRef>>> },
    Ready(Vec<StageRef>),
}

impl Pool {
    /// Starts reading `items` on a thread.
    pub fn start(data: Data, cache: PathBuf, items: Vec<Item>) -> Pool {
        let (done, stop) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let total = items.len();
        let (d, s) = (done.clone(), stop.clone());
        let thread = std::thread::spawn(move || read(&data, &cache, items, &d, &s));
        Pool::Reading { done, total, stop, thread: Some(thread) }
    }

    /// The stages once read (the thread joined on its end).
    pub fn stages(&mut self) -> Option<&[StageRef]> {
        if let Pool::Reading { thread, .. } = self {
            if !thread.as_ref().is_some_and(|t| t.is_finished()) {
                return None;
            }
            let stages = thread.take().and_then(|t| t.join().ok()).unwrap_or_default();
            *self = Pool::Ready(stages);
        }
        match self {
            Pool::Ready(stages) => Some(stages),
            Pool::Reading { .. } => None,
        }
    }

    /// The items read and all, while reading.
    pub fn progress(&self) -> Option<(usize, usize)> {
        match self {
            Pool::Reading { done, total, .. } => Some((done.load(Ordering::Relaxed), *total)),
            Pool::Ready(_) => None,
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let Pool::Reading { stop, .. } = self {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Map, Stage};
    use rbr_gameplay::weather::Row;

    /// A `.trk` with a driveline chunk of `points`.
    fn trk_bytes(points: &[[f32; 3]]) -> Vec<u8> {
        let mut payload = (points.len() as u32).to_le_bytes().to_vec();
        for p in points {
            for v in p.iter().chain(&[0.0, 0.0, 0.0, 0.0]) {
                payload.extend(v.to_le_bytes());
            }
            payload.extend([0; 4]);
        }
        let mut bytes = 8u32.to_le_bytes().to_vec();
        bytes.extend(3u32.to_le_bytes());
        bytes.extend(0x14u32.to_le_bytes());
        bytes.extend((payload.len() as u32).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn retail(number: u32) -> Map {
        let stage = Stage { name: format!("track-{number}_O"), map: number, letter: 'O' };
        Map { number, stages: vec![stage], origin: Origin::Retail }
    }

    #[test]
    fn the_pool_has_the_startable_stages_with_their_driveline_lengths_and_keeps_them() {
        let here = crate::test_dir("events-pool");
        let maps = here.join("data").join("Maps");
        std::fs::create_dir_all(&maps).unwrap();
        // 54 reads (a 3-4-5 corner: 7 m), 55 is RSF-encrypted, 56 has no `.trk`, 71 is the school's (not listed).
        std::fs::write(maps.join("track-54_O.trk"), trk_bytes(&[[0.0; 3], [3.0, 0.0, 0.0], [3.0, 4.0, 0.0]])).unwrap();
        let mut encrypted = stage_files::ENCRYPTED.to_vec();
        encrypted.extend([0; 64]);
        std::fs::write(maps.join("track-55_O.trk"), encrypted).unwrap();
        let rows = [54, 55, 56, 71].map(|map| Row { map, letter: 3, cloud: 0, cloud_letter: 0, preset: 0, wetness: 0 });
        let season = Season { rows: rows.to_vec(), nice: vec![0, 1, 2, 3], bad: vec![0, 1, 2, 3], sky: None };
        let mut catalog = Catalog { maps: [54, 55, 56, 71].map(retail).to_vec(), ..Catalog::default() };
        // A map with no row for the weathers cannot start.
        catalog.maps.push(retail(57));
        let found = items(&catalog, &season);
        let keys: Vec<&str> = found.iter().map(|i| i.stage.key.as_str()).collect();
        assert_eq!(keys, ["54", "55", "56"]);
        assert_eq!((found[0].stage.name.as_str(), found[0].stage.surface), ("Tanner", Surface::Gravel));
        assert!(!found[0].stage.community);
        assert_eq!(surface(catalog::surface_kind(41)), Surface::Tarmac);

        let data = Data::new(here.join("data"), None);
        let cache = here.join(CACHE);
        let (done, stop) = (AtomicUsize::new(0), AtomicBool::new(false));
        let stages = read(&data, &cache, found.clone(), &done, &stop);
        assert_eq!(done.load(Ordering::Relaxed), 3);
        assert_eq!(stages.len(), 1);
        assert_eq!((stages[0].key.as_str(), stages[0].length), ("54", 7.0));
        // Kept: read back without the `.trk`'s parse, and again once the stage changes.
        let kept = read_cached(&cache, "54", None).unwrap();
        assert_eq!(kept, [[0.0; 3], [3.0, 0.0, 0.0], [3.0, 4.0, 0.0]]);
        std::fs::write(cache_file(&cache, "54"), {
            let mut b = MAGIC.to_vec();
            let print = fingerprint(&maps.join("track-54_O.trk")).unwrap();
            b.extend((print.len() as u32).to_le_bytes());
            b.extend(print.as_bytes());
            b.extend(1u32.to_le_bytes());
            b
        })
        .unwrap();
        assert_eq!(read_cached(&cache, "54", None), None, "a short file is not a driveline");
        std::fs::write(maps.join("track-54_O.trk"), trk_bytes(&[[0.0; 3], [0.0, 0.0, 10.0]])).unwrap();
        let again = read(&data, &cache, found, &done, &stop);
        assert_eq!(again[0].length, 10.0);
        std::fs::remove_dir_all(&here).unwrap();
    }
}
