//! BTB (Bob's Track Builder) stages in the launcher (`notes/btb.md` "Playing"): the BTB folders of the RallySimFans
//! install listed with its stages, and Start preparing the files the stage plays as (`rbr_btb::stage`). The stage plays
//! as retail map 41, M variant: its weathers are map 41's M rows (the plugin only replaces the M files), its records
//! and ghosts are not kept (they would be map 41's), and the surface is `track.ini`'s.

use std::path::{Path, PathBuf};

use rbr_btb::{ids, stage, track};
use rbr_gameplay::weather::Season;

use crate::catalog::{Map, Stage};
use crate::Form;

/// The retail map a BTB stage plays as: its weathers and the numbers the game's files and records go by.
pub const MAP: u32 = 41;

/// A listed BTB stage. The map's number is its RSF id, else a synthetic one no other map has ([`number`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Btb {
    /// The `RX_CONTENT/Tracks/<folder>/` directory.
    pub folder: PathBuf,
    /// `track.ini`'s name, else the folder's.
    pub name: String,
    pub author: Option<String>,
    pub km: Option<f32>,
    /// The `rbr-standalone --surface` word, from `track.ini` (the plugin plays gravel when it names none).
    pub surface: &'static str,
    /// The RallySimFans install.
    pub rsf_root: PathBuf,
    /// Why it cannot start.
    pub problem: Option<String>,
}

impl Btb {
    /// 0 gravel, 1 tarmac, 2 snow, as `Map::surface`.
    pub fn surface_kind(&self) -> u32 {
        ["gravel", "tarmac", "snow"].iter().position(|s| *s == self.surface).unwrap_or(0) as u32
    }

    /// Writes the stage's five map 41 files into `home/btb/<folder>/`; the stage name that loads them.
    pub fn prepare(&self, home: &Path) -> Result<String, String> {
        let track = track::Track::open(&self.folder)?;
        stage::prepare(&track, &self.rsf_root, &home.join("btb").join(&track.name))
    }
}

/// The retail season with map 41's M rows only (the plugin replaces only the M files): the other rows of map 41 are
/// no one's, and its bad weather is the good one. Row numbers stay, `rbr-standalone --season-row` takes them.
pub fn season(retail: &Season) -> Season {
    let mut season = retail.clone();
    for row in season.rows.iter_mut().filter(|r| r.map == MAP && r.letter != 0) {
        row.map = 0;
    }
    let nice = season.nice.iter().copied().find(|&k| season.rows.get(k as usize).is_some_and(|r| r.map == MAP));
    season.nice = nice.into_iter().collect();
    season.bad = season.nice.clone();
    season
}

/// The BTB stages of the RSF install `root` as maps, the ones whose number `taken` not already.
pub fn maps(root: &Path, taken: impl Fn(u32) -> bool) -> Vec<Map> {
    let ids: std::collections::HashMap<PathBuf, u32> =
        ids::match_ids(root).0.into_iter().map(|(f, id, _)| (f, id)).collect();
    let mut used = Vec::new();
    let mut maps = Vec::new();
    for found in track::scan(root) {
        let Ok(track) = found else { continue };
        let free = |n: u32| !taken(n) && !used.contains(&n);
        let number =
            ids.get(&track.folder).copied().filter(|&n| free(n)).unwrap_or_else(|| synthetic(&track.name, free));
        used.push(number);
        let name =
            if track.info.name.trim().is_empty() { track.name.clone() } else { track.info.name.trim().to_owned() };
        let btb = Btb {
            name,
            author: track.info.author.clone().filter(|a| !a.trim().is_empty()),
            km: track.info.length_km.map(|km| km as f32),
            surface: stage::surface(&track).unwrap_or("gravel"),
            rsf_root: root.to_owned(),
            problem: None,
            folder: track.folder.clone(),
        };
        let stage = Stage { name: format!("{}/{}", btb.folder.display(), stage::TRACK), map: number, letter: 'M' };
        maps.push(Map { number, stages: vec![stage], rsf: None, btb: Some(btb) });
    }
    maps
}

/// A stable number for a BTB folder without an RSF id: above any RSF id, from the folder's name.
fn synthetic(name: &str, free: impl Fn(u32) -> bool) -> u32 {
    let hash = name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    let mut number = 1_000_000 + hash % 1_000_000;
    while !free(number) {
        number += 1;
    }
    number
}

impl Form {
    /// The launch of a BTB `stage`: its files prepared in the player's folder and named in place of the listed folder,
    /// the surface for `rbr-standalone`, and no records (`--saves`, `--profile`: they would be map 41's). Other stages
    /// pass unchanged. `None`, with the status, when the files cannot be built.
    pub fn btb_launch(&mut self, mut stage: Stage, mut extra: Vec<String>) -> Option<(Stage, Vec<String>)> {
        let Some(btb) = self.catalog.map(stage.map).and_then(|m| m.btb.clone()) else { return Some((stage, extra)) };
        match btb.prepare(&self.here()) {
            Ok(name) => stage.name = name,
            Err(e) => {
                self.status = format!("{}: {e}", btb.name);
                return None;
            }
        }
        let mut kept = Vec::new();
        let mut args = extra.drain(..);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--saves" | "--profile" => drop(args.next()),
                _ => kept.push(arg),
            }
        }
        kept.extend(["--surface".to_owned(), btb.surface.to_owned()]);
        Some((stage, kept))
    }
}
