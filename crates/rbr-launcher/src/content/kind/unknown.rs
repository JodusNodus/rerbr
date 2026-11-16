//! Anything no kind recognised: the last of the kinds, so every source has an outcome.

use super::{Ctx, Found, Item, Kind};
use crate::content::source::Source;
use crate::content::Outcome;

pub const UNKNOWN: &str = "not recognised: ReRBR adds BTB stages, co-drivers, pace notes, NGP car packs and NGP";

pub struct Unknown;

impl Kind for Unknown {
    fn detect(&self, _: &dyn Source) -> Option<Found> {
        Some(Found { tag: UNKNOWN, only: true, ..Found::new("not supported") })
    }

    fn add(&self, item: &Item, _: &Ctx) -> Vec<Outcome> {
        vec![Outcome::failed(&item.src.name(), item.found.label, item.found.tag)]
    }

    fn fallback(&self) -> bool {
        true
    }
}
