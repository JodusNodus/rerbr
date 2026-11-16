//! The retail disc: an `.iso`, or a folder with `data1.hdr`. Not installed silently: the disc setup opens with the
//! disc filled in.

use super::{Ctx, Found, Item, Kind};
use crate::content::source::Source;
use crate::content::{Goto, Outcome};

pub struct Disc;

impl Kind for Disc {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let iso = !src.is_dir() && src.name().to_ascii_lowercase().ends_with(".iso");
        let folder = src.is_dir()
            && src.entries().iter().any(|e| matches!(&e.path[..], [f] if f.eq_ignore_ascii_case("data1.hdr")));
        (iso || folder).then(|| Found { only: true, ..Found::new("disc image") })
    }

    fn add(&self, item: &Item, _: &Ctx) -> Vec<Outcome> {
        let name = item.src.name();
        let text = format!("Disc image {name}: choose where to set it up in Settings > Game folder");
        let goto = Some(Goto::Disc(item.src.path().to_owned()));
        vec![Outcome {
            source: name,
            kind: "disc image",
            result: Ok(text),
            goto,
            open: None,
            refresh: Default::default(),
        }]
    }

    fn add_all(&self, items: &[Item], ctx: &Ctx) -> Vec<Outcome> {
        let mut out = Vec::new();
        for item in items {
            match out.is_empty() {
                true => out.extend(self.add(item, ctx)),
                false => out.push(Outcome::failed(&item.src.name(), "disc image", "one disc at a time")),
            }
        }
        out
    }
}
