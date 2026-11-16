//! What is not added, and why. Each is asked only when no other kind found anything.

use super::{Ctx, Found, Item, Kind};
use crate::content::source::Source;
use crate::content::Outcome;

pub const RAR: &str = "RAR archives can't be read: extract it, then add the folder";
pub const INSTALLER: &str = "installers can't be read: extract it with 7-Zip, then add the folder";
pub const CLASSIC: &str = "classic-format stages aren't supported yet; BTB stages are";
pub const UNKNOWN: &str = "not recognised: ReRBR adds BTB stages, co-drivers, pace notes, NGP car packs and NGP";

/// A refusal: its reason, and when it applies.
pub struct Refused {
    reason: &'static str,
    when: fn(&dyn Source) -> bool,
}

fn ends(src: &dyn Source, ends: &[&str]) -> bool {
    let name = src.name().to_ascii_lowercase();
    !src.is_dir() && ends.iter().any(|e| name.ends_with(e))
}

/// Whether the files look like a classic-format stage: `Maps/track-*` files, or `Tracks.ini` / `TrackSettings*.ini`.
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

pub const RAR_FILE: Refused = Refused { reason: RAR, when: |s| ends(s, &[".rar"]) };
pub const INSTALLER_FILE: Refused = Refused { reason: INSTALLER, when: |s| ends(s, &[".exe", ".msi"]) };
pub const CLASSIC_STAGE: Refused = Refused { reason: CLASSIC, when: is_classic };
pub const ANYTHING_ELSE: Refused = Refused { reason: UNKNOWN, when: |_| true };

impl Kind for Refused {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        (self.when)(src).then(|| Found { tag: self.reason, only: true, ..Found::new("not supported") })
    }

    fn add(&self, item: &Item, _: &Ctx) -> Vec<Outcome> {
        vec![Outcome::failed(&item.src.name(), item.found.label, item.found.tag)]
    }

    fn fallback(&self) -> bool {
        true
    }
}
