//! The player's DiRT Rally install: its data folder and the routes under `tracks/locations/<location>/<area>/route_<n>`.
//! An area (`ghymno_rally_01`) is one world its routes share (`routesplit.pssg`, `tracksplit.pssg`); a route holds what
//! differs (its course, objects, collision). Read in place, never copied.
use std::path::{Path, PathBuf};

/// The folder of the game's data: the one holding `tracks/locations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub data: PathBuf,
}

/// One route of an area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub location: String,
    pub area: String,
    pub index: u32,
    /// `.../<location>/<area>/route_<index>`.
    pub dir: PathBuf,
}

impl Route {
    /// The area's folder, which its routes share.
    pub fn area_dir(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    /// `location/area/route_<n>`, as the command line names it.
    pub fn id(&self) -> String {
        format!("{}/{}/route_{}", self.location, self.area, self.index)
    }

    /// Rally stages only: rallycross circuits and Pikes Peak are other formats, the frontend holds none.
    pub fn is_rally(&self) -> bool {
        self.area.contains("_rally_")
    }

    /// A rallycross venue's layout (`holjes_rallycross`): a closed lap.
    pub fn is_rallycross(&self) -> bool {
        self.area.ends_with("_rallycross")
    }
}

/// The Steam folder name of the game.
const STEAM_FOLDER: &str = "DiRT Rally";

impl Install {
    /// The install at `path`: the game folder (Feral's `share/data` under it), its data folder or `tracks` itself.
    pub fn at(path: &Path) -> Result<Install, String> {
        for data in [path.join("share/data"), path.to_owned(), path.parent().map(Path::to_owned).unwrap_or_default()] {
            if data.join("tracks/locations").is_dir() {
                return Ok(Install { data });
            }
        }
        Err(format!("{}: no DiRT Rally data (tracks/locations) found", path.display()))
    }

    /// The install in a Steam library: each library of `libraryfolders.vdf` under the usual Steam roots.
    pub fn find() -> Option<Install> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let roots = [home.join(".local/share/Steam"), home.join(".steam/steam")];
        let libraries = roots.iter().flat_map(|root| {
            let vdf = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
            library_paths(&vdf).into_iter().chain([root.clone()])
        });
        libraries
            .map(|library| library.join("steamapps/common").join(STEAM_FOLDER))
            .find_map(|game| Install::at(&game).ok())
    }

    /// Every route of every area, sorted.
    pub fn routes(&self) -> Vec<Route> {
        let mut routes = Vec::new();
        for location in sorted_dirs(&self.data.join("tracks/locations")) {
            for area in sorted_dirs(&location) {
                for dir in sorted_dirs(&area) {
                    let Some(index) = name(&dir).strip_prefix("route_").and_then(|n| n.parse().ok()) else { continue };
                    routes.push(Route { location: name(&location), area: name(&area), index, dir });
                }
            }
        }
        routes.sort_by(|a, b| (&a.location, &a.area, a.index).cmp(&(&b.location, &b.area, b.index)));
        routes
    }

    /// The route `location/area/route_<n>` (or `location/area/<n>`).
    pub fn route(&self, id: &str) -> Result<Route, String> {
        let parts: Vec<&str> = id.trim_matches('/').split('/').collect();
        let [location, area, route] = parts[..] else { return Err(format!("{id}: expected LOCATION/AREA/route_N")) };
        let index: u32 =
            route.strip_prefix("route_").unwrap_or(route).parse().map_err(|_| format!("{id}: bad route"))?;
        self.routes()
            .into_iter()
            .find(|r| r.location == location && r.area == area && r.index == index)
            .ok_or_else(|| format!("{id}: no such route in {}", self.data.display()))
    }
}

fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn sorted_dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The `"path"` values of Steam's `libraryfolders.vdf`.
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut fields = line.split('"').skip(1).step_by(2);
            (fields.next()? == "path").then(|| fields.next().map(|p| PathBuf::from(p.replace("\\\\", "\\"))))?
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_libraries_are_read_from_their_vdf() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/me/.local/share/Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/data/SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n\t}\n}\n";
        assert_eq!(library_paths(vdf), [PathBuf::from("/home/me/.local/share/Steam"), "/mnt/data/SteamLibrary".into()]);
    }

    #[test]
    fn routes_are_listed_by_location_area_and_index() {
        let root = std::env::temp_dir().join(format!("rbr-dirt-rally-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["greece/ghymno_rally_01/route_1", "greece/ghymno_rally_01/route_0", "usa/pikes_peak/route_0"] {
            std::fs::create_dir_all(root.join("share/data/tracks/locations").join(dir)).unwrap();
        }
        std::fs::create_dir_all(root.join("share/data/tracks/locations/greece/ghymno_rally_01/banks")).unwrap();
        let install = Install::at(&root).unwrap();
        let ids: Vec<String> = install.routes().iter().map(Route::id).collect();
        assert_eq!(ids, ["greece/ghymno_rally_01/route_0", "greece/ghymno_rally_01/route_1", "usa/pikes_peak/route_0"]);
        let route = install.route("greece/ghymno_rally_01/1").unwrap();
        assert!(route.is_rally() && route.area_dir().ends_with("ghymno_rally_01"));
        assert!(!install.route("usa/pikes_peak/route_0").unwrap().is_rally());
        assert!(install.route("greece/ghymno_rally_01/route_7").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
