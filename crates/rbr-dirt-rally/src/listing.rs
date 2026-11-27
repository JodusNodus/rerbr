//! What an install offers a picker (the launcher's): the routes that convert, each with the folder the import writes,
//! its title and its length. Rally stages and Pikes Peak sections only: the rallycross circuits convert (`import`) but
//! are not offered yet, the frontend holds no course, and Pikes Peak's full climb and third section are held back
//! ([`PIKES_PEAK_OFFERED`]).
use crate::import::default_name;
use crate::install::{Install, Route};
use crate::route;

/// The Pikes Peak routes offered: sections 1 and 2 (routes 2 and 4). Route 0 (the full climb) converts, route 6
/// (section 3) is refused by the import, for the same reason: 45 driveline points (112 m, 13.6 km up the climb) have no
/// road collision, only a surface 4 m below facing down, so a car sinks through it (autopilot drive of route 0,
/// 2026-11-27). All three areas alike. Offer them when the import reads that stretch.
const PIKES_PEAK_OFFERED: [u32; 2] = [2, 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rally,
    PikesPeak,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    /// `location/area/route_<n>`, as [`crate::import::import`] takes it.
    pub id: String,
    /// The stage folder the import writes, `DR_greece_ghymno_rally_01_route_0`.
    pub folder: String,
    /// [`route::title`].
    pub title: String,
    /// The location's folder name (`greece`).
    pub location: String,
    /// The route's number: 0 is a rally area's long forward stage.
    pub index: u32,
    pub kind: Kind,
    /// The stage's length in metres; `None` when its course does not read.
    pub length: Option<f64>,
}

/// The routes of `install` that convert, in route order.
pub fn list(install: &Install) -> Vec<Listed> {
    install.routes().iter().filter_map(listed).collect()
}

fn listed(route: &Route) -> Option<Listed> {
    let kind = if route.is_rally() {
        Kind::Rally
    } else if route.is_pikes_peak() && PIKES_PEAK_OFFERED.contains(&route.index) {
        Kind::PikesPeak
    } else {
        return None;
    };
    let length = route::course(route).ok().map(|c| c.length());
    Some(Listed {
        id: route.id(),
        folder: default_name(route),
        title: route::title(route),
        location: route.location.clone(),
        index: route.index,
        kind,
        length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_routes_that_convert_are_listed() {
        let root = std::env::temp_dir().join(format!("rbr-dirt-rally-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let routes = [
            "greece/ghymno_rally_01/route_1",
            "sweden/holjes_rallycross/route_0",
            "usa/pikes_peak/route_2",
            "usa/pikes_peak/route_0",
            "usa/pikes_peak/route_6",
        ];
        for dir in routes.iter().chain(&["frontend/fe_track/route_0"]) {
            std::fs::create_dir_all(root.join("tracks/locations").join(dir)).unwrap();
        }
        let listed = list(&Install::at(&root).unwrap());
        let _ = std::fs::remove_dir_all(&root);
        let got: Vec<_> = listed.iter().map(|l| (l.id.as_str(), l.folder.as_str(), l.kind, l.length)).collect();
        assert_eq!(
            got,
            [
                ("greece/ghymno_rally_01/route_1", "DR_greece_ghymno_rally_01_route_1", Kind::Rally, None),
                ("usa/pikes_peak/route_2", "DR_usa_pikes_peak_route_2", Kind::PikesPeak, None),
            ]
        );
        assert_eq!(listed[0].title, "Greece, Ghymno 1, long reverse");
    }

    /// Over the install (`RBR_DIRT_RALLY`, the game folder; skipped when unset): the 72 rally routes and the 6 Pikes
    /// Peak sections of the Steam release (the rallycross layouts are not listed), each with a length, a folder of its own
    /// and an id the import takes.
    #[test]
    fn the_real_install_lists_every_rally_and_pikes_peak_route() {
        let Some(path) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = Install::at(std::path::Path::new(&path)).expect("RBR_DIRT_RALLY holds no DiRT Rally data");
        let listed = list(&install);
        let count = |kind| listed.iter().filter(|l| l.kind == kind).count();
        assert_eq!((count(Kind::Rally), count(Kind::PikesPeak)), (72, 6));
        for l in &listed {
            let km = l.length.unwrap_or(0.) / 1000.;
            assert!((0.5..40.).contains(&km), "{}: {km} km", l.id);
            assert!(install.route(&l.id).is_ok(), "{}", l.id);
        }
        let mut folders: Vec<_> = listed.iter().map(|l| &l.folder).collect();
        folders.sort();
        folders.dedup();
        assert_eq!(folders.len(), listed.len());
    }
}
