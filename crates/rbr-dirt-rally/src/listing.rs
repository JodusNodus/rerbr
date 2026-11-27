//! What an install offers a picker (the launcher's): the routes that convert, each with the folder the import writes,
//! its title and its length. Rally stages and rallycross circuits only; Pikes Peak and the frontend are left out.
use crate::import::default_name;
use crate::install::{Install, Route};
use crate::route;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rally,
    /// A closed lap (`circuit=1`).
    Rallycross,
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
    /// The stage's length, or a rallycross lap's, in metres; `None` when its course does not read.
    pub length: Option<f64>,
}

/// The routes of `install` that convert, in route order.
pub fn list(install: &Install) -> Vec<Listed> {
    install.routes().iter().filter_map(listed).collect()
}

fn listed(route: &Route) -> Option<Listed> {
    let (kind, length) = if route.is_rally() {
        (Kind::Rally, route::course(route).ok().map(|c| c.length()))
    } else if route.is_rallycross() {
        (Kind::Rallycross, route::lap(route).ok().map(|l| l.length))
    } else {
        return None;
    };
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
        let routes = ["greece/ghymno_rally_01/route_1", "sweden/holjes_rallycross/route_0", "usa/pikes_peak/route_0"];
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
                ("sweden/holjes_rallycross/route_0", "DR_sweden_holjes_rallycross_route_0", Kind::Rallycross, None),
            ]
        );
        assert_eq!(listed[0].title, "Greece, Ghymno 1, long reverse");
    }

    /// Over the install (`RBR_DIRT_RALLY`, the game folder; skipped when unset): the 72 rally routes and 8 rallycross
    /// layouts of the Steam release, each with a length, a folder of its own and an id the import takes.
    #[test]
    fn the_real_install_lists_every_rally_and_rallycross_route() {
        let Some(path) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = Install::at(std::path::Path::new(&path)).expect("RBR_DIRT_RALLY holds no DiRT Rally data");
        let listed = list(&install);
        let count = |kind| listed.iter().filter(|l| l.kind == kind).count();
        assert_eq!((count(Kind::Rally), count(Kind::Rallycross)), (72, 8));
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
