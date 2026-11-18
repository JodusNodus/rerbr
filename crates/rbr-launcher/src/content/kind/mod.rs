//! The kinds of content the Add knows, one module each: how to recognise it in a [`Source`], and how to add it. A new
//! kind is one module here and one line in the registry (`super::KINDS`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::downloads::Owned;
use super::source::Source;
use super::Outcome;
use crate::job::{Progress, Report};

pub mod btb;
pub mod car;
pub mod classic;
pub mod codriver;
pub mod disc;
pub mod ngp;
pub mod skin;
pub mod unknown;

/// What a kind found in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What it is, for the player.
    pub label: &'static str,
    /// A word the kind keeps for itself (a car piece's role, a refusal's reason).
    pub tag: &'static str,
    /// Where in the source it is: one path of parts each (a BTB stage's folder).
    pub at: Vec<Vec<String>>,
    /// It is all the source is: no other kind is asked.
    pub only: bool,
}

impl Found {
    pub fn new(label: &'static str) -> Found {
        Found { label, tag: "", at: Vec::new(), only: false }
    }
}

/// A source and what a kind found in it.
pub struct Item<'a> {
    pub src: &'a dyn Source,
    pub found: Found,
}

/// What the launcher lists, for the kinds that give a thing a number (classic stages).
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// The numbers of the listed maps: the retail ones, the game folder's stages, the player's BTB and added stages.
    pub numbers: Vec<u32>,
    /// The game's `Maps/TrackSettings.ini`, whose weathers a stage without its own borrows.
    pub weather: Option<PathBuf>,
    /// The cars a skin can dress.
    pub cars: Vec<skin::Target>,
}

/// What a kind needs to add: the player's folder, what is listed, the progress report and the cancel flag.
pub struct Ctx<'a> {
    pub home: &'a Path,
    pub known: &'a Known,
    pub report: Report<'a>,
    pub cancel: &'a AtomicBool,
}

impl Ctx<'_> {
    pub fn step(&self, step: &'static str) {
        (self.report)(&Progress { step, done: 0, total: 0 });
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub trait Kind: Sync {
    /// What the source holds of this kind, if anything.
    fn detect(&self, src: &dyn Source) -> Option<Found>;

    /// Adds what was found in one source: an outcome for each thing.
    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome>;

    /// Adds what was found in all the sources: kinds that put sources together (car pieces) override it.
    fn add_all(&self, items: &[Item], ctx: &Ctx) -> Vec<Outcome> {
        let one = |item: &Item| match ctx.cancelled() {
            true => vec![Outcome::failed(&item.src.name(), item.found.label, "cancelled")],
            false => self.add(item, ctx),
        };
        items.iter().flat_map(one).collect()
    }

    /// Asked only when no other kind found anything: a refusal, or a skin (a car model and a stage have textures too).
    fn fallback(&self) -> bool {
        false
    }

    /// The label of the recent downloads list's offer of this source, unless it is owned already or is not offered
    /// alone.
    fn offer(&self, _src: &dyn Source, _found: &Found, _owned: &Owned) -> Option<&'static str> {
        None
    }
}
