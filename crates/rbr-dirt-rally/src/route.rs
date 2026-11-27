//! A route's course, road, calls and identity, from the route folder's `"Rr` files (`binxml`):
//! - `progress_track.xml`: the timing gates, the start, the splits and the finish (`course`);
//! - `ai_track.xml`: the road's edge and width at each AI gate (`road`);
//! - `<area>_codriver_1_pro_<n>_data.xml`: the pacenote calls (`calls`);
//! - the folder names: which part of the stage the route is, a fallback title and the country (stage names are not on
//!   disk). Positions are in DiRT Rally's frame (`frame`).
use crate::binxml::{self, Element};
use crate::install::Route;

fn read(route: &Route, file: &str) -> Result<Element, String> {
    let path = route.dir.join(file);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    binxml::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn point(parent: &Element, name: &str) -> Result<[f32; 3], String> {
    parent.child(name).and_then(Element::floats).ok_or_else(|| format!("<{name}> is not three numbers"))
}

/// A timing gate: a cross line over the road, `distance` metres along the route from its own origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    pub distance: f64,
    pub left: [f32; 3],
    pub right: [f32; 3],
    pub crossing: [f32; 3],
}

/// The timed part of a route. Distances are the file's, from the route's own origin (ahead of the start line by the
/// run-up); the gates run on past both ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    pub gates: Vec<Gate>,
    pub start: f64,
    /// The intermediate splits only.
    pub splits: Vec<f64>,
    pub finish: f64,
}

impl Course {
    /// Finish minus start: the stage's length. (`total_distance` also counts the run-up and run-out.)
    pub fn length(&self) -> f64 {
        self.finish - self.start
    }

    /// The gate of the start line, by its distance.
    pub fn start_gate(&self) -> Option<&Gate> {
        self.gates.iter().find(|g| g.distance == self.start)
    }
}

/// The course of route id 0 of `progress_track.xml`, the rally stage (rallycross files hold a second, short one).
pub fn course(route: &Route) -> Result<Course, String> {
    parse_course(&read(route, "progress_track.xml")?)
}

fn parse_gates(root: &Element) -> Result<Vec<Gate>, String> {
    let gates = root.child("gates").ok_or("no <gates>")?;
    gates
        .children_named("gate")
        .map(|g| {
            Ok(Gate {
                distance: g.attr_num("distance").ok_or("gate without distance")?,
                left: point(g, "left")?,
                right: point(g, "right")?,
                crossing: point(g, "crossing")?,
            })
        })
        .collect()
}

fn parse_course(root: &Element) -> Result<Course, String> {
    let gates = parse_gates(root)?;
    let routes = root.child("routes").ok_or("no <routes>")?;
    let route = routes.children_named("route").find(|r| r.attr_num("id") == Some(0.0)).ok_or("no route 0")?;
    let distance = |split: &Element| {
        let id = split.attr_num("gate").ok_or("split without gate")? as usize;
        let gate = gates.get(id).ok_or_else(|| format!("split at gate {id} of {}", gates.len()))?;
        Ok::<f64, String>(gate.distance)
    };
    let (mut start, mut finish, mut splits) = (None, None, Vec::new());
    for split in route.children_named("split") {
        match split.attr("type") {
            Some("start") => start = Some(distance(split)?),
            Some("finish") => finish = Some(distance(split)?),
            _ => splits.push(distance(split)?),
        }
    }
    let (Some(start), Some(finish)) = (start, finish) else { return Err("route 0 has no start or finish".into()) };
    Ok(Course { gates, start, splits, finish })
}

/// A rallycross lap: the main loop of route id 0 of `progress_track.xml` (id 1 is the joker lap's own span), which
/// closes on itself. Positions are the gates' crossings, in DiRT Rally's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Lap {
    /// The finish split's gate: the lap's own line.
    pub line: [f32; 3],
    /// The `time` splits, in lap order from the line.
    pub splits: Vec<[f32; 3]>,
    /// The lap's length, the file's `total_distance`.
    pub length: f64,
}

/// The lap of a rallycross route.
pub fn lap(route: &Route) -> Result<Lap, String> {
    parse_lap(&read(route, "progress_track.xml")?)
}

fn parse_lap(root: &Element) -> Result<Lap, String> {
    let gates = parse_gates(root)?;
    let main = root
        .child("routes")
        .and_then(|r| r.children_named("route").find(|r| r.attr_num("id") == Some(0.0)))
        .ok_or("no route 0")?;
    let crossing = |split: &Element| {
        let id = split.attr_num("gate").ok_or("split without gate")? as usize;
        gates.get(id).map(|g| g.crossing).ok_or_else(|| format!("split at gate {id} of {}", gates.len()))
    };
    let (mut line, mut splits) = (None, Vec::new());
    for split in main.children_named("split") {
        match split.attr("type") {
            Some("finish") => line = Some(crossing(split)?),
            Some("time") => splits.push(crossing(split)?),
            _ => {}
        }
    }
    let line = line.ok_or("route 0 has no finish")?;
    let track = root.child("track").filter(|t| t.attr("type") == Some("circuit")).ok_or("not a circuit")?;
    let length = track.attr_num("total_distance").ok_or("circuit without total_distance")?;
    Ok(Lap { line, splits, length })
}

/// The road of a rallycross lap in `ai_track.xml`: its gates from the one nearest `from` on along the links, the race
/// branch at each fork (the joker branch is left out), until the loop closes.
pub fn lap_road(route: &Route, from: [f32; 3]) -> Result<Vec<Section>, String> {
    parse_lap_road(&read(route, "ai_track.xml")?, from)
}

fn parse_lap_road(root: &Element, from: [f32; 3]) -> Result<Vec<Section>, String> {
    let track = root.child("track").ok_or("no <track>")?;
    let sections = parse_road(root)?;
    let id = |e: &Element, name: &str| e.attr_num(name).map(|v| v as usize).ok_or_else(|| format!("no {name}"));
    let mut links = Vec::<(usize, usize, usize)>::new();
    for link in track.child("links").into_iter().flat_map(|l| l.children_named("link")) {
        links.push((id(link, "id")?, id(link, "from_gate")?, id(link, "to_gate")?));
    }
    // The links the joker alone takes: a fork's branch used for `race` is the lap's.
    let mut joker = std::collections::HashSet::new();
    for fork in track.child("fork_sets").into_iter().flat_map(|f| f.children_named("fork_set")) {
        for branch in fork.children_named("fork") {
            let race = branch.children_named("use").any(|u| u.attr("type") == Some("race"));
            if !race {
                joker.insert(id(branch, "link_id")?);
            }
        }
    }
    let dist = |s: &Section| {
        let c = s.centre();
        (0..3).map(|i| f64::from(c[i] - from[i]).powi(2)).sum::<f64>()
    };
    // A route's file keeps the gates of the other layouts too: only those with a link of the lap are its road.
    let first = links
        .iter()
        .filter(|l| !joker.contains(&l.0) && l.1 < sections.len())
        .map(|l| l.1)
        .min_by(|&a, &b| dist(&sections[a]).total_cmp(&dist(&sections[b])))
        .ok_or("no links")?;
    let (mut lap, mut at) = (vec![first], first);
    loop {
        let mut next = links.iter().filter(|l| l.1 == at && !joker.contains(&l.0));
        let Some(&(_, _, to)) = next.next() else { return Err(format!("the lap's road ends at gate {at}")) };
        if next.next().is_some() {
            return Err(format!("the lap's road forks at gate {at} with no race branch"));
        }
        if to == first {
            return Ok(lap.into_iter().map(|g| sections[g]).collect());
        }
        if to >= sections.len() || lap.len() > sections.len() {
            return Err(format!("the lap's road does not close (gate {to})"));
        }
        lap.push(to);
        at = to;
    }
}

/// The road at one AI gate: `left` is the left edge, `normal` the unit direction across the road to the right (flat)
/// and `width` the road's, edge to edge.
///
/// Evidence for the left edge, over every rally route: the AI `position` plus half the width lands within a metre of
/// the progress gates' `crossing` line, the bare position is half a width off to the left (test
/// `the_road_is_centred_on_the_progress_gates`), and `normal` is parallel to the progress gates' left to right axis.
/// Not checked against the mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    pub left: [f32; 3],
    pub normal: [f32; 3],
    pub width: f32,
}

impl Section {
    /// The middle of the road.
    pub fn centre(&self) -> [f32; 3] {
        std::array::from_fn(|i| self.left[i] + self.normal[i] * self.width / 2.0)
    }

    /// The right edge.
    pub fn right(&self) -> [f32; 3] {
        std::array::from_fn(|i| self.left[i] + self.normal[i] * self.width)
    }
}

/// The road along `ai_track.xml`'s gates, in route order.
pub fn road(route: &Route) -> Result<Vec<Section>, String> {
    parse_road(&read(route, "ai_track.xml")?)
}

fn parse_road(root: &Element) -> Result<Vec<Section>, String> {
    let gates = root.child("track").and_then(|t| t.child("gates")).ok_or("no <track><gates>")?;
    gates
        .children_named("gate")
        .map(|g| {
            let width = g
                .child("waypoints")
                .into_iter()
                .flat_map(|w| w.children_named("waypoint"))
                .find(|w| w.attr("type") == Some("right_track_limit"))
                .and_then(|w| w.attr_num("length"))
                .ok_or("gate without right_track_limit")?;
            Ok(Section { left: point(g, "position")?, normal: point(g, "normal")?, width: width as f32 })
        })
        .collect()
}

/// One subcall of a pacenote. Types: 1 left and 2 right corners (`angle` the total turn in degrees), 3 a call without
/// a corner; 5 to 9 undecoded. The modifiers and `distance_link` are undecoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subcall {
    pub kind: u32,
    /// Milliseconds after the call's first subcall.
    pub delay: u32,
    pub angle: f32,
    pub modifier_a: u32,
    pub modifier_b: u32,
    pub distance_link: u32,
}

/// A pacenote call.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// Metres from the start line.
    pub distance: f64,
    pub name: String,
    /// The used subcalls (type 0 dropped), in order.
    pub subcalls: Vec<Subcall>,
}

/// The route's calls, sorted by distance; none where the route has no codriver file (Pikes Peak).
pub fn calls(route: &Route) -> Result<Vec<Call>, String> {
    let file = format!("{}_codriver_1_pro_{}_data.xml", route.area, route.index);
    if !route.dir.join(&file).is_file() {
        return Ok(Vec::new());
    }
    parse_calls(&read(route, &file)?)
}

fn parse_calls(root: &Element) -> Result<Vec<Call>, String> {
    let list = root.child("codriver_calls").ok_or("no <codriver_calls>")?;
    let mut calls = list
        .children_named("call")
        .map(|c| {
            let field = |s: &Element, name: &str| s.attr_num(name).ok_or_else(|| format!("subcall without {name}"));
            let subcalls = c
                .children_named("subcall")
                .map(|s| {
                    Ok(Subcall {
                        kind: field(s, "type")? as u32,
                        delay: field(s, "delay")? as u32,
                        angle: field(s, "angle")? as f32,
                        modifier_a: field(s, "modifierA")? as u32,
                        modifier_b: field(s, "modifierB")? as u32,
                        distance_link: field(s, "distanceLink")? as u32,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Call {
                distance: c.attr_num("distanceAlongTrack").ok_or("call without distanceAlongTrack")?,
                name: c.attr("name").unwrap_or_default().to_owned(),
                subcalls: subcalls.into_iter().filter(|s| s.kind != 0).collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    calls.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    Ok(calls)
}

/// Which part of its area a rally route covers; even routes run forward, odd ones reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    LongForward,
    LongReverse,
    FirstHalfForward,
    FirstHalfReverse,
    SecondHalfForward,
    SecondHalfReverse,
}

impl Role {
    /// The role of route number `index` (0..=5).
    pub fn of_index(index: u32) -> Option<Role> {
        Some(match index {
            0 => Role::LongForward,
            1 => Role::LongReverse,
            2 => Role::FirstHalfForward,
            3 => Role::FirstHalfReverse,
            4 => Role::SecondHalfForward,
            5 => Role::SecondHalfReverse,
            _ => return None,
        })
    }

    pub fn is_reverse(self) -> bool {
        matches!(self, Role::LongReverse | Role::FirstHalfReverse | Role::SecondHalfReverse)
    }

    fn part(self) -> &'static str {
        match self {
            Role::LongForward | Role::LongReverse => "long",
            Role::FirstHalfForward | Role::FirstHalfReverse => "first half",
            Role::SecondHalfForward | Role::SecondHalfReverse => "second half",
        }
    }
}

/// The role of a rally route; `None` for rallycross, Pikes Peak and the frontend's.
pub fn role(route: &Route) -> Option<Role> {
    if route.is_rally() {
        Role::of_index(route.index)
    } else {
        None
    }
}

/// A readable name from the folders (the game's stage names are not on disk, its texts are in the encrypted
/// `game.bin`): `Greece, Ghymno 1, long forward`.
pub fn title(route: &Route) -> String {
    if route.is_rallycross() {
        let venue = match route.area.as_str() {
            "holjes_rallycross" => "Höljes".to_owned(),
            area => area.trim_end_matches("_rallycross").split('_').map(capitalise).collect::<Vec<_>>().join(" "),
        };
        return if route.index == 0 { venue } else { format!("{venue}, layout {}", route.index) };
    }
    let area = route.area.split("_rally_");
    let (base, number) = (area.clone().next().unwrap_or_default(), area.last().filter(|_| route.is_rally()));
    // A number in the folder's name is a number (`historic_02` is `Historic 2`).
    let words: Vec<String> =
        base.split('_').map(|w| w.parse::<u32>().map_or_else(|_| capitalise(w), |n| n.to_string())).collect();
    let mut title = words.join(" ");
    if let Some(n) = number.and_then(|n| n.parse::<u32>().ok()) {
        title = format!("{title} {n}");
    }
    match role(route) {
        Some(role) => {
            let direction = if role.is_reverse() { "reverse" } else { "forward" };
            format!("{}, {title}, {} {direction}", capitalise(&route.location), role.part())
        }
        None if route.is_pikes_peak() => format!("{title}, {}", climb_part(route.index)),
        None => format!("{title}, route {}", route.index),
    }
}

/// Which part of a Pikes Peak climb a route covers: 0 is all of it; 2, 4 and 6 are its three sections in order
/// (6.3, 6.5 and 7.1 km: their start and finish gates lie on route 0's at 0 to 6.3, 6.3 to 12.8 and 12.4 to 19.5 km).
fn climb_part(index: u32) -> String {
    match index {
        0 => "full climb".into(),
        2 => "section 1".into(),
        4 => "section 2".into(),
        6 => "section 3".into(),
        n => format!("route {n}"),
    }
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The ISO 3166 code of the location folder (Col de Turini is Monaco's rally, driven in France).
pub fn country(route: &Route) -> Option<&'static str> {
    Some(match route.location.as_str() {
        "finland" => "FI",
        "france" => "FR",
        "germany" => "DE",
        "greece" => "GR",
        "norway" => "NO",
        "sweden" => "SE",
        "uk" => "GB",
        "usa" => "US",
        _ => return None,
    })
}

/// Where the area's venue lies (latitude, longitude, degrees) where the country's rally region (the renderer's default
/// for `countrycode`) is far off: the sun's and moon's paths follow it. `None` keeps the country's.
pub fn place(route: &Route) -> Option<[f64; 2]> {
    let area = route.area.as_str();
    Some(match route.location.as_str() {
        "france" => [43.98, 7.39],  // Col de Turini
        "germany" => [49.62, 7.30], // Baumholder
        "usa" => [38.84, -105.04],  // Pikes Peak
        "norway" => [63.43, 10.93], // Hell
        "sweden" if area.starts_with("holjes") => [60.05, 12.60],
        "uk" if area.starts_with("lydden") => [51.18, 1.20],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::Install;
    use std::path::PathBuf;

    fn el(name: &str, attrs: &[(&str, &str)], text: Option<&str>, children: Vec<Element>) -> Element {
        Element {
            name: name.into(),
            attrs: attrs.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
            text: text.map(Into::into),
            children,
        }
    }

    fn route(location: &str, area: &str, index: u32) -> Route {
        Route {
            location: location.into(),
            area: area.into(),
            index,
            dir: PathBuf::from(format!("/x/{location}/{area}/route_{index}")),
        }
    }

    fn gate(id: u32, distance: &str) -> Element {
        let p = |name, text| el(name, &[], Some(text), vec![]);
        let at = format!("{id} 0 0");
        el(
            "gate",
            &[("distance", distance)],
            None,
            vec![p("left", "0 0 1"), p("right", "0 0 -1"), p("crossing", at.as_str())],
        )
    }

    #[test]
    fn a_course_is_gates_and_split_distances() {
        let split = |ty, gate| el("split", &[("type", ty), ("gate", gate)], None, vec![]);
        let root = el(
            "progress_track_data",
            &[],
            None,
            vec![
                el(
                    "routes",
                    &[],
                    None,
                    vec![el(
                        "route",
                        &[("id", "0")],
                        None,
                        vec![split("start", "1"), split("time", "2"), split("finish", "3")],
                    )],
                ),
                el("gates", &[], None, (0..4).map(|i| gate(i, &format!("{}", i * 12))).collect()),
            ],
        );
        let course = parse_course(&root).unwrap();
        assert_eq!(
            (course.start, course.splits.clone(), course.finish, course.length()),
            (12.0, vec![24.0], 36.0, 24.0)
        );
        assert_eq!(course.start_gate().unwrap().crossing, [1.0, 0.0, 0.0]);
        let bad = el("x", &[], None, vec![]);
        assert!(parse_course(&bad).is_err());
    }

    #[test]
    fn a_section_is_centred_half_a_width_from_the_left_edge() {
        let section = Section { left: [1.0, 2.0, 3.0], normal: [0.0, 0.0, 1.0], width: 6.0 };
        assert_eq!((section.centre(), section.right()), ([1.0, 2.0, 6.0], [1.0, 2.0, 9.0]));
        let waypoint = |ty, len| el("waypoint", &[("type", ty), ("length", len)], None, vec![]);
        let gate = el(
            "gate",
            &[],
            None,
            vec![
                el("position", &[], Some("1 2 3"), vec![]),
                el("normal", &[], Some("0 0 1"), vec![]),
                el(
                    "waypoints",
                    &[],
                    None,
                    vec![waypoint("left_track_limit", "0.00"), waypoint("right_track_limit", "6.00")],
                ),
            ],
        );
        let root =
            el("ai_track_data", &[], None, vec![el("track", &[], None, vec![el("gates", &[], None, vec![gate])])]);
        assert_eq!(parse_road(&root).unwrap(), [section]);
    }

    #[test]
    fn calls_drop_unused_subcalls_and_sort() {
        let sub = |ty, delay| {
            el(
                "subcall",
                &[
                    ("type", ty),
                    ("delay", delay),
                    ("angle", "50"),
                    ("modifierA", "1"),
                    ("modifierB", "0"),
                    ("distanceLink", "5"),
                ],
                None,
                vec![],
            )
        };
        let call = |name, d, subs| el("call", &[("name", name), ("distanceAlongTrack", d)], None, subs);
        let root = el(
            "root",
            &[],
            None,
            vec![el(
                "codriver_calls",
                &[],
                None,
                vec![
                    call("b", "20.5", vec![sub("2", "0")]),
                    call("a", "8", vec![sub("1", "0"), sub("9", "500"), sub("0", "0")]),
                ],
            )],
        );
        let calls = parse_calls(&root).unwrap();
        assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(calls[0].subcalls.iter().map(|s| (s.kind, s.delay)).collect::<Vec<_>>(), [(1, 0), (9, 500)]);
        assert_eq!(
            calls[0].subcalls[0],
            Subcall { kind: 1, delay: 0, angle: 50.0, modifier_a: 1, modifier_b: 0, distance_link: 5 }
        );
    }

    #[test]
    fn identity_comes_from_the_folders() {
        let r = route("greece", "ghymno_rally_01", 1);
        assert_eq!(
            (role(&r), title(&r), country(&r)),
            (Some(Role::LongReverse), "Greece, Ghymno 1, long reverse".into(), Some("GR"))
        );
        let r = route("france", "col_de_turini_rally_02", 4);
        assert_eq!(title(&r), "France, Col De Turini 2, second half forward");
        assert_eq!(country(&r), Some("FR"));
        let r = route("usa", "pikes_peak_historic", 2);
        assert_eq!((role(&r), title(&r)), (None, "Pikes Peak Historic, section 1".into()));
        assert_eq!(title(&route("usa", "pikes_peak", 0)), "Pikes Peak, full climb");
        assert_eq!(title(&route("usa", "pikes_peak_historic_02", 6)), "Pikes Peak Historic 2, section 3");
        assert_eq!(role(&route("uk", "hafren_rally_01", 9)), None);
        assert_eq!(country(&route("mars", "a", 0)), None);
        assert_eq!(place(&route("france", "col_de_turini_rally_01", 0)), Some([43.98, 7.39]));
        assert_eq!(place(&route("sweden", "holjes_rallycross", 0)), Some([60.05, 12.60]));
        assert_eq!(place(&route("sweden", "vargasen_rally_01", 0)), None);
        assert_eq!(place(&route("uk", "hafren_rally_01", 0)), None);
    }

    // The tests over the real install (`RBR_DIRT_RALLY`, the game folder; skipped when unset).

    fn install() -> Option<Install> {
        let path = std::env::var_os("RBR_DIRT_RALLY")?;
        Some(Install::at(std::path::Path::new(&path)).expect("RBR_DIRT_RALLY holds no DiRT Rally data"))
    }

    fn rally_routes(install: &Install) -> Vec<Route> {
        install.routes().into_iter().filter(Route::is_rally).collect()
    }

    #[test]
    fn every_binary_route_file_parses() {
        let Some(install) = install() else { return };
        let mut parsed = 0;
        for route in install.routes() {
            for entry in std::fs::read_dir(&route.dir).unwrap().flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "xml") {
                    continue;
                }
                let bytes = std::fs::read(&path).unwrap();
                if bytes.starts_with(&[0x1a, 0x22, b'R', b'r']) {
                    binxml::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    parsed += 1;
                }
            }
        }
        eprintln!("{parsed} binary xml files parsed");
        assert!(parsed >= 93 * 4, "{parsed}");
    }

    #[test]
    fn every_rally_course_is_ordered_and_of_its_length() {
        let Some(install) = install() else { return };
        let routes = rally_routes(&install);
        assert_eq!(routes.len(), 72);
        for route in &routes {
            let c = course(route).unwrap_or_else(|e| panic!("{}: {e}", route.id()));
            let id = route.id();
            assert!(c.start < c.splits.first().copied().unwrap_or(c.finish), "{id}");
            assert!(c.splits.windows(2).all(|w| w[0] < w[1]), "{id}");
            assert!(c.splits.last().is_none_or(|&s| s < c.finish), "{id}");
            assert!((3..=9).contains(&c.splits.len()), "{id}: {} splits", c.splits.len());
            assert!(c.gates.windows(2).all(|w| w[0].distance < w[1].distance), "{id}");
            assert!(c.start_gate().is_some() && (3500.0..16300.0).contains(&c.length()), "{id}: {}", c.length());
            for g in &c.gates {
                let width = dist(g.left, g.right);
                assert!((2.0..40.0).contains(&width), "{id}: gate {width}");
            }
        }
        // route.md's table.
        let length = |id: &str| course(&install.route(id).unwrap()).unwrap().length();
        for (id, expect) in [
            ("greece/ghymno_rally_01/route_0", 9666.5),
            ("finland/ouninpohja_rally_01/route_0", 15041.0),
            ("uk/hafren_rally_01/route_1", 10064.0),
            ("france/col_de_turini_rally_02/route_5", 3952.0),
            ("sweden/vargasen_rally_02/route_0", 12122.0),
        ] {
            assert!((length(id) - expect).abs() < 1.0, "{id}: {}", length(id));
        }
        let ghymno = course(&install.route("greece/ghymno_rally_01/route_0").unwrap()).unwrap();
        assert_eq!((ghymno.gates.len(), ghymno.splits.len()), (839, 7));
    }

    fn dist(a: [f32; 3], b: [f32; 3]) -> f64 {
        (0..3).map(|i| f64::from(a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
    }

    /// Where `p` lies across the nearest section (by its centre), as a fraction of the road's width from the left edge.
    fn across(p: [f32; 3], sections: &[Section]) -> f64 {
        let s = sections.iter().min_by(|a, b| dist(p, a.centre()).total_cmp(&dist(p, b.centre()))).unwrap();
        let along: f64 = (0..3).map(|i| f64::from((p[i] - s.left[i]) * s.normal[i])).sum();
        along / f64::from(s.width)
    }

    #[test]
    fn the_road_is_centred_on_the_progress_gates() {
        let Some(install) = install() else { return };
        let mut fractions = Vec::new();
        for route in rally_routes(&install) {
            let (id, c, sections) = (route.id(), course(&route).unwrap(), road(&route).unwrap());
            assert!((300..1400).contains(&sections.len()), "{id}: {}", sections.len());
            let (lo, hi) = sections.iter().fold((f32::MAX, 0f32), |(lo, hi), s| (lo.min(s.width), hi.max(s.width)));
            assert!(lo > 1.0 && hi < 30.0, "{id}: width {lo}..{hi}");
            let unit = |s: &Section| s.normal.iter().map(|v| f64::from(v * v)).sum::<f64>();
            assert!(sections.iter().all(|s| (unit(s) - 1.0).abs() < 0.01), "{id}");
            fractions.extend(c.gates.iter().skip(3).step_by(25).map(|g| across(g.crossing, &sections)));
        }
        let mean = fractions.iter().sum::<f64>() / fractions.len() as f64;
        let near = fractions.iter().filter(|f| (0.2..0.8).contains(*f)).count() as f64 / fractions.len() as f64;
        eprintln!("progress crossings across the road from its left edge: mean {mean:.2} of the width, {near:.2} inside 0.2..0.8");
        assert!((0.4..0.6).contains(&mean) && near > 0.9, "mean {mean}, inside {near}");
    }

    #[test]
    fn calls_are_sorted_and_inside_the_stage() {
        let Some(install) = install() else { return };
        let mut total = 0;
        for route in rally_routes(&install) {
            let (id, c, calls) = (route.id(), course(&route).unwrap(), calls(&route).unwrap());
            assert!(calls.len() > 20, "{id}: {}", calls.len());
            assert!(calls.windows(2).all(|w| w[0].distance <= w[1].distance), "{id}");
            // The finish call sits just past the line: the stage plus a margin.
            let (first, last) = (calls[0].distance, calls.last().unwrap().distance);
            assert!(first >= 0.0 && last <= c.length() + 100.0, "{id}: {first}..{last} of {}", c.length());
            assert!(calls.iter().all(|c| c.subcalls.iter().all(|s| s.kind != 0)), "{id}");
            total += calls.len();
        }
        eprintln!("{total} calls");
        let first = &calls(&install.route("greece/ghymno_rally_01/route_0").unwrap()).unwrap();
        assert_eq!(first.len(), 115);
        assert_eq!((first[0].distance, first[0].subcalls[0].kind, first[0].subcalls[0].angle), (8.00737, 2, 50.0));
    }

    #[test]
    fn every_rally_route_has_an_identity() {
        let Some(install) = install() else { return };
        for route in rally_routes(&install) {
            assert!(role(&route).is_some() && country(&route).is_some(), "{}", route.id());
            // The renderer's country default is far off at these venues (Turini's FR, Baumholder's DE, Pikes Peak's US).
            assert_eq!(
                place(&route).is_some(),
                ["france", "germany", "usa"].contains(&route.location.as_str()),
                "{}",
                route.id()
            );
        }
        assert_eq!(
            title(&install.route("greece/ghymno_rally_01/route_2").unwrap()),
            "Greece, Ghymno 1, first half forward"
        );
    }

    fn ai_gate(x: f32) -> Element {
        let at = |name: &str, text: &str| el(name, &[], Some(text), vec![]);
        let limit = el("waypoint", &[("type", "right_track_limit"), ("length", "10")], None, vec![]);
        el(
            "gate",
            &[],
            None,
            vec![at("position", &format!("{x} 0 0")), at("normal", "0 0 1"), el("waypoints", &[], None, vec![limit])],
        )
    }

    #[test]
    fn a_lap_follows_the_race_branch_of_a_fork_and_leaves_gates_without_links_out() {
        // Gates 0..3 are the lap and gate 5 a joker detour from 1 to 3; gate 6 is another layout's, unlinked, and
        // nearest the line.
        let link = |id, from, to| el("link", &[("id", id), ("from_gate", from), ("to_gate", to)], None, vec![]);
        let branch = |id, kind| el("fork", &[("link_id", id)], None, vec![el("use", &[("type", kind)], None, vec![])]);
        let gates = [0., 10., 20., 30., 100., 15., 0.5].map(ai_gate).to_vec();
        let links = vec![
            link("0", "0", "1"),
            link("1", "1", "2"),
            link("2", "2", "3"),
            link("3", "3", "0"),
            link("4", "1", "5"),
            link("5", "5", "3"),
        ];
        let forks = vec![el("fork_set", &[], None, vec![branch("1", "race"), branch("4", "joker")])];
        let track = el(
            "track",
            &[],
            None,
            vec![el("gates", &[], None, gates), el("links", &[], None, links), el("fork_sets", &[], None, forks)],
        );
        let root = el("ai_track_data", &[], None, vec![track]);
        // Gate positions are the left edge: the centre is 5 m on along the normal.
        let road = parse_lap_road(&root, [0., 0., 5.]).unwrap();
        let xs: Vec<f32> = road.iter().map(|s| s.left[0]).collect();
        assert_eq!(xs, [0., 10., 20., 30.]);
        assert_eq!(road[0].centre(), [0., 0., 5.]);
    }

    #[test]
    fn every_rallycross_route_is_a_closed_lap_on_a_closed_road() {
        let Some(install) = install() else { return };
        let routes: Vec<Route> = install.routes().into_iter().filter(Route::is_rallycross).collect();
        assert_eq!(routes.len(), 8);
        for route in &routes {
            let (id, lap) = (route.id(), lap(route).unwrap());
            assert!((500.0..1400.0).contains(&lap.length) && (1..=3).contains(&lap.splits.len()), "{id}: {lap:?}");
            let road = lap_road(route, lap.line).unwrap();
            // The road closes (the last gate to the first is a gate's step) and is the lap's length.
            let sections = road.len();
            assert!(dist(road[sections - 1].centre(), road[0].centre()) < 40.0, "{id}");
            let length: f64 = (0..sections).map(|i| dist(road[i].centre(), road[(i + 1) % sections].centre())).sum();
            assert!((length - lap.length).abs() < lap.length * 0.03, "{id}: {length} of {}", lap.length);
            // The line lies on the road, and so does every split.
            for p in std::iter::once(&lap.line).chain(&lap.splits) {
                assert!(road.iter().any(|s| dist(s.centre(), *p) < 20.0), "{id}: {p:?} is off the road");
            }
            assert!(role(route).is_none() && country(route).is_some(), "{id}");
        }
        let names: Vec<String> = routes.iter().map(title).collect();
        assert_eq!(names[0], "Hell");
        assert!(
            names.contains(&"Höljes".to_owned()) && names.contains(&"Lydden Hill, layout 2".to_owned()),
            "{names:?}"
        );
    }
}
