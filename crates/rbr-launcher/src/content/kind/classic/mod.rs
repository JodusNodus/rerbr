//! Classic-format stages (`Maps/track-*` files, `Tracks.ini`, `TrackSettings*.ini`): only recognised, to say they are
//! not supported yet. Asked only when no other kind found anything.

use super::{Ctx, Found, Item, Kind};
use crate::content::source::Source;
use crate::content::Outcome;

pub const CLASSIC: &str = "classic-format stages aren't supported yet; BTB stages are";

pub struct Classic;

/// Whether the files look like a classic-format stage.
pub fn is_classic(src: &dyn Source) -> bool {
    src.entries().iter().any(|e| {
        let path = &e.path;
        let file = path.last().map(|f| f.to_ascii_lowercase()).unwrap_or_default();
        let in_maps = path.len() >= 2 && path[..path.len() - 1].iter().any(|d| d.eq_ignore_ascii_case("maps"));
        (in_maps && file.starts_with("track-"))
            || file == "tracks.ini"
            || (file.starts_with("tracksettings") && file.ends_with(".ini"))
    })
}

impl Kind for Classic {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        is_classic(src).then(|| Found { tag: CLASSIC, only: true, ..Found::new("not supported") })
    }

    fn add(&self, item: &Item, _: &Ctx) -> Vec<Outcome> {
        vec![Outcome::failed(&item.src.name(), item.found.label, item.found.tag)]
    }

    fn fallback(&self) -> bool {
        true
    }
}
