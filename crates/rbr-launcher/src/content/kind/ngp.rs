//! NGP's own package (`.7z` or folder): its `NGP.dll` and `physics.rbz` kept in the player's folder, as the Settings
//! download does, and then the NGP in use.

use super::{Ctx, Found, Item, Kind};
use crate::content::source::{names, Source};
use crate::content::{Goto, Outcome, Refresh};
use crate::ngp_install::{self, KEPT, NGP};

pub struct Ngp;

impl Kind for Ngp {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        // `read_ngp` reads a `.7z` or a folder.
        let seven = src.name().to_ascii_lowercase().ends_with(".7z");
        if !(src.is_dir() || seven) {
            return None;
        }
        let names = names(src);
        let have = crate::disc_install::with_folders(names.iter().map(String::as_str));
        NGP.iter().all(|w| have.contains(&w.to_lowercase())).then(|| Found { only: true, ..Found::new("NGP") })
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        ctx.step("Adding NGP");
        let (name, to) = (item.src.name(), ctx.home.join(KEPT));
        let result = std::fs::create_dir_all(&to)
            .map_err(|e| format!("{}: {e}", to.display()))
            .and_then(|()| ngp_install::add_ngp_to(item.src.path(), &to, ctx.report, ctx.cancel));
        let outcome = match result {
            Ok(()) => Outcome::added(&name, "NGP", "NGP 7.4.778", "it's the NGP in use now", Goto::Settings),
            Err(e) => Outcome::failed(&name, "NGP", e),
        };
        vec![outcome.refreshing(Refresh::Ngp)]
    }
}
