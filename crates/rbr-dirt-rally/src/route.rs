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

fn parse_course(root: &Element) -> Result<Course, String> {
    let gates = root.child("gates").ok_or("no <gates>")?;
    let gates = gates
        .children_named("gate")
        .map(|g| {
            Ok(Gate {
                distance: g.attr_num("distance").ok_or("gate without distance")?,
                left: point(g, "left")?,
                right: point(g, "right")?,
                crossing: point(g, "crossing")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
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

/// The route's calls, sorted by distance.
pub fn calls(route: &Route) -> Result<Vec<Call>, String> {
    let file = format!("{}_codriver_1_pro_{}_data.xml", route.area, route.index);
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

/// A readable name from the folders (the game's stage names are not on disk): `Ghymno 1, long, forward`.
pub fn title(route: &Route) -> String {
    let area = route.area.split("_rally_");
    let (base, number) = (area.clone().next().unwrap_or_default(), area.last().filter(|_| route.is_rally()));
    let words: Vec<String> = base.split('_').map(capitalise).collect();
    let mut title = words.join(" ");
    if let Some(n) = number.and_then(|n| n.parse::<u32>().ok()) {
        title = format!("{title} {n}");
    }
    match role(route) {
        Some(role) => {
            format!("{title}, {}, {}", role.part(), if role.is_reverse() { "reverse" } else { "forward" })
        }
        None => format!("{title}, route {}", route.index),
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
            (Some(Role::LongReverse), "Ghymno 1, long, reverse".into(), Some("GR"))
        );
        let r = route("france", "col_de_turini_rally_02", 4);
        assert_eq!(title(&r), "Col De Turini 2, second half, forward");
        assert_eq!(country(&r), Some("FR"));
        let r = route("usa", "pikes_peak_historic", 2);
        assert_eq!((role(&r), title(&r)), (None, "Pikes Peak Historic, route 2".into()));
        assert_eq!(role(&route("uk", "hafren_rally_01", 9)), None);
        assert_eq!(country(&route("mars", "a", 0)), None);
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
                assert!((8.0..13.0).contains(&width), "{id}: gate {width}");
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
        }
        assert_eq!(title(&install.route("greece/ghymno_rally_01/route_2").unwrap()), "Ghymno 1, first half, forward");
    }
}
