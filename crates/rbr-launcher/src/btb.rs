//! BTB (Bob's Track Builder) stages in the launcher (`notes/btb.md` "Playing"): the BTB folders of the RallySimFans
//! install listed with its stages, and Start preparing the files the stage plays (`rbr_btb::stage`). A BTB stage is a
//! stage of its own number: its weathers are the rows of the sky map (`rbr_btb::stage::SKY_MAP`) under that number, and
//! its records and ghosts are kept under it as a community stage's are.

use std::path::{Path, PathBuf};

use rbr_btb::{ids, stage, track};

use crate::catalog::{Map, Origin, Stage};
use crate::Form;

/// A listed BTB stage. The map's number is its RSF id, else a synthetic one no other map has ([`synthetic`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Btb {
    /// The `RX_CONTENT/Tracks/<folder>/` directory.
    pub folder: PathBuf,
    /// `track.ini`'s name, else the folder's.
    pub name: String,
    pub author: Option<String>,
    /// The country's code, upper case: `track.ini`'s `countrycode`, else the RSF stage list's `short_country` under the stage's id.
    pub country: Option<String>,
    pub km: Option<f32>,
    /// The `rbr-standalone --surface` word, from `track.ini` (the plugin plays gravel when it names none).
    pub surface: &'static str,
    /// Why it cannot start.
    pub problem: Option<String>,
}

impl Btb {
    /// 0 gravel, 1 tarmac, 2 snow, as `Map::surface`.
    pub fn surface_kind(&self) -> u32 {
        ["gravel", "tarmac", "snow"].iter().position(|s| *s == self.surface).unwrap_or(0) as u32
    }

    /// Writes the stage `number`'s files into `home/btb/<folder>/`; the stage name that loads them.
    pub fn prepare(&self, number: u32, maps: &Path, home: &Path) -> Result<String, String> {
        let track = track::Track::open(&self.folder)?;
        stage::prepare(&track, number, maps, &home.join("btb").join(&track.name))
    }
}

/// The BTB stages of the RSF install `root` as maps, the ones whose number `taken` not already.
pub fn maps(root: &Path, taken: impl Fn(u32) -> bool) -> Vec<Map> {
    let ids: std::collections::HashMap<PathBuf, u32> =
        ids::match_ids(root).0.into_iter().map(|(f, id, _)| (f, id)).collect();
    let list = rbr_formats::rsf::stage_list(root);
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
        let country = track.info.country.as_ref().or_else(|| list.get(&number)?.get("short_country"));
        let btb = Btb {
            name,
            country: country.map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty()),
            author: track.info.author.clone().filter(|a| !a.trim().is_empty()),
            km: track.info.length_km.map(|km| km as f32),
            surface: stage::surface(&track).unwrap_or("gravel"),
            problem: None,
            folder: track.folder.clone(),
        };
        let stage = Stage { name: stage::stage_name(&btb.folder, number), map: number, letter: stage::LETTER };
        maps.push(Map { number, stages: vec![stage], origin: Origin::Btb(btb) });
    }
    maps
}

/// The first of the numbers [`synthetic`] gives: above any RSF id.
pub const SYNTHETIC: u32 = 1_000_000;

/// A stable number for a BTB folder without an RSF id: from [`SYNTHETIC`] up, from the folder's name.
fn synthetic(name: &str, free: impl Fn(u32) -> bool) -> u32 {
    let hash = name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    let mut number = SYNTHETIC + hash % SYNTHETIC;
    while !free(number) {
        number += 1;
    }
    number
}

impl Form {
    /// The launch of a BTB `stage`: its files prepared in the player's folder and named in place of the listed folder,
    /// and the BTB stage's arguments for `rbr-standalone`: the surface, and the retail map whose weather rows it
    /// plays. Other stages pass unchanged. `None`, with the status, when the files cannot be built.
    pub fn btb_launch(&mut self, mut stage: Stage, mut extra: Vec<String>) -> Option<(Stage, Vec<String>)> {
        let Some(btb) = self.catalog.map(stage.map).and_then(Map::btb).cloned() else { return Some((stage, extra)) };
        match btb.prepare(stage.map, &self.data.install().join("Maps"), &self.here()) {
            Ok(name) => stage.name = name,
            Err(e) => {
                self.status = format!("{}: {e}", btb.name);
                return None;
            }
        }
        extra.extend(["--surface".to_owned(), btb.surface.to_owned(), "--btb".to_owned(), stage::SKY_MAP.to_string()]);
        Some((stage, extra))
    }
}
