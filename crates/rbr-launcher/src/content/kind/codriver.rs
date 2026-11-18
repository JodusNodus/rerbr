//! Co-drivers and pace notes, through `codriver_packs::import` (which copies what it accepts into `codrivers/` and
//! `notes/`). Recognised by the entry names (only the names, never the contents).

use super::{btb, classic, Ctx, Found, Item, Kind};
use crate::codriver_packs::{import, pack_name};
use crate::content::downloads::{already_added, Owned};
use crate::content::source::Source;
use crate::content::{Goto, Outcome, Refresh};

const AUDIO: [&str; 5] = [".ogg", ".wav", ".mp3", ".flac", ".oga"];

pub struct Codriver;

/// Whether an archive named `archive` with these entry names looks like a co-driver or pace-note pack.
pub fn looks_like_pack(archive: &str, entries: &[String]) -> bool {
    let lower = archive.to_ascii_lowercase();
    let named = ["pacenote", "luppis", "codriver", "co-driver"].iter().any(|k| lower.contains(k));
    entries.iter().any(|entry| {
        let e = entry.replace('\\', "/").to_ascii_lowercase();
        e.rsplit('/').next() == Some("pacenote.ini")
            || e.contains("config/pacenotes/")
            || e.contains("mypacenotes/")
            || (e.contains("sounds/") && AUDIO.iter().any(|a| e.ends_with(a)))
            || (named && e.contains('/') && e.ends_with(".ini"))
    })
}

/// The outcome of an import's result line (`Co-driver <pack>: …`, `Stage notes <pack>: …`).
fn outcome(source: &str, line: &str) -> Outcome {
    let (kind, rest, place) = match (line.strip_prefix("Co-driver "), line.strip_prefix("Stage notes ")) {
        (Some(rest), _) => ("co-driver", rest, "pick it in Co-drivers"),
        (_, Some(rest)) => ("pace notes", rest, "they play with a custom co-driver (Co-drivers)"),
        _ => ("co-driver", line, "pick it in Co-drivers"),
    };
    let pack = rest.split(':').next().unwrap_or(rest);
    Outcome::added(source, kind, pack, place, Goto::Codrivers).refreshing(Refresh::Codriver)
}

impl Kind for Codriver {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        // A BTB stage's own sounds and settings are not a co-driver's.
        let stages = btb::stage_dirs(src.entries());
        let outside: Vec<String> =
            src.entries().iter().filter(|e| !btb::in_stage(&e.path, &stages)).map(|e| e.path.join("/")).collect();
        // A folder of notes is a stage's folders of `.ini` files: any `.ini` two deep, unless it is a classic stage.
        let notes = src.is_dir()
            && !classic::is_classic(src)
            && src.entries().iter().any(|e| {
                !btb::in_stage(&e.path, &stages)
                    && e.path.len() >= 2
                    && e.path[e.path.len() - 1].to_lowercase().ends_with(".ini")
            });
        (looks_like_pack(&src.name(), &outside) || notes).then(|| Found::new("co-driver or pace notes"))
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        ctx.step("Adding co-driver");
        let name = item.src.name();
        match import(ctx.home, item.src.path(), ctx.cancel) {
            Ok(lines) => lines.iter().map(|l| outcome(&name, l)).collect(),
            Err(e) => vec![Outcome::failed(&name, "co-driver", e).refreshing(Refresh::Codriver)],
        }
    }

    fn offer(&self, src: &dyn Source, found: &Found, owned: &Owned) -> Option<&'static str> {
        (!already_added(&pack_name(src.path()), &owned.packs)).then_some(found.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn packs_are_recognised_by_their_entries() {
        assert!(looks_like_pack("a.zip", &names(&["Voice/PaceNote.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["x\\config\\pacenotes\\v3.ini"])));
        assert!(looks_like_pack("a.zip", &names(&["Plugins/Pacenote/sounds/pkg/1.ogg"])));
        assert!(looks_like_pack("a.zip", &names(&["MyPacenotes/Finland/x.ini"])));
        assert!(!looks_like_pack("a.zip", &names(&["sounds/readme.txt", "game.exe"])));
        assert!(!looks_like_pack("a.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("Luppis-notes.zip", &names(&["Finland/x.ini"])));
        assert!(looks_like_pack("My Co-Driver.7z", &names(&["Finland/x.ini"])));
        assert!(!looks_like_pack("Luppis.zip", &names(&["x.ini"])));
    }
}
