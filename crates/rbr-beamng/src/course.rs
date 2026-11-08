//! The stage a mission's race makes: the AI road that passes every path node (the level's `DecalRoad` with
//! `drivability` whose worst distance to the nodes is the smallest), run from the start position in the race's
//! direction to the finish (the last path node of an open race; just before the start on a closed one), with the
//! splits at the `useAsSplit` nodes.

use crate::level::{Road, Scene};
use crate::race::Race;
use crate::road::Line;

/// Gap left between the finish and the start on a closed race, metres.
pub const LOOP_GAP: f64 = 30.0;
/// Driveline step, metres: the physics takes at most 10000 driveline entries (a 25 km stage at 2.5 m).
pub const STEP: f64 = 2.5;

#[derive(Debug, Clone)]
pub struct Course {
    /// The chosen road's name.
    pub road: String,
    /// The worst distance of a path node from the road.
    pub fit: f64,
    /// The driveline from the start to the finish.
    pub line: Line,
    /// Start position (on the race's start position) and heading unit vector, BeamNG frame.
    pub start: [f64; 3],
    pub heading: [f64; 3],
    /// Split distances along [`Course::line`].
    pub splits: Vec<f64>,
    /// A loop: the course ends [`LOOP_GAP`] before its start.
    pub closed: bool,
}

fn worst_distance(line: &Line, race: &Race) -> f64 {
    race.nodes.iter().map(|n| line.project(n.pos).1).fold(0.0, f64::max)
}

/// Ends of two open roads closer than this join them into one.
const JOIN: f64 = 15.0;

/// Open roads joined end to start (a level may split one AI road into pieces): from each road, the nearest unused
/// road whose start or end (then reversed) lies within [`JOIN`] of the chain's end, until none does.
fn chained(roads: &[&Road]) -> Vec<Road> {
    let dist =
        |a: &[f64; 4], b: &[f64; 4]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let open: Vec<&Road> = roads.iter().copied().filter(|r| !r.looped).collect();
    let mut out = Vec::new();
    for (i, first) in open.iter().enumerate() {
        let mut used = vec![false; open.len()];
        used[i] = true;
        let mut nodes = first.nodes.clone();
        let mut names = vec![first.name.clone()];
        loop {
            let end = *nodes.last().unwrap();
            let next = (0..open.len())
                .filter(|&j| !used[j])
                .flat_map(|j| {
                    [(j, false, dist(&end, &open[j].nodes[0])), (j, true, dist(&end, open[j].nodes.last().unwrap()))]
                })
                .filter(|c| c.2 < JOIN)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            let Some((j, reversed, _)) = next else { break };
            used[j] = true;
            let mut more = open[j].nodes.clone();
            if reversed {
                more.reverse();
            }
            join(&mut nodes, more);
            names.push(open[j].name.clone());
        }
        if names.len() > 1 {
            out.push(Road { name: names.join("+"), nodes, ..(*first).clone() });
        }
    }
    out
}

/// `more` appended to `nodes`. Pieces may overlap (Irohazaka's by 2-3 m, Nagao's): the chain's last nodes past the
/// piece's start and the piece's first nodes not ahead of the chain's end, within [`JOIN`] of the join, are left out,
/// else the line doubles back.
fn join(nodes: &mut Vec<[f64; 4]>, mut more: Vec<[f64; 4]>) {
    // Whether `to` lies ahead of `from` along the plan direction `a` to `b`, and their distance.
    let ahead = |from: &[f64; 4], to: &[f64; 4], a: &[f64; 4], b: &[f64; 4]| {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        (d[0] * (b[0] - a[0]) + d[1] * (b[1] - a[1]) > 0.0, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
    };
    while nodes.len() >= 2 && more.len() >= 2 {
        let (past, gap) = ahead(&more[0], &nodes[nodes.len() - 1], &more[0], &more[1]);
        if !past || gap >= JOIN {
            break;
        }
        nodes.pop();
    }
    while nodes.len() >= 2 && more.len() >= 2 {
        let n = nodes.len();
        let (beyond, gap) = ahead(&nodes[n - 1], &more[0], &nodes[n - 2], &nodes[n - 1]);
        if beyond || gap >= JOIN {
            break;
        }
        more.remove(0);
    }
    nodes.extend(more);
}

/// The longest drivable road of the scene, joined pieces included, as its nodes' positions; reversed on `reverse`.
pub fn longest_road(scene: &Scene, reverse: bool) -> Option<Vec<[f64; 3]>> {
    let roads: Vec<&Road> = scene.roads.iter().filter(|r| r.drivability > 0.0 && r.nodes.len() >= 2).collect();
    let chains = chained(&roads);
    let length = |r: &Road| Line::spline(&r.nodes, r.looped, STEP).length();
    let best = roads.iter().copied().chain(&chains).max_by(|a, b| length(a).total_cmp(&length(b)))?;
    let mut points: Vec<[f64; 3]> = best.nodes.iter().map(|n| [n[0], n[1], n[2]]).collect();
    if reverse {
        points.reverse();
    }
    Some(points)
}

/// The course of `race` on the scene's roads.
pub fn course(scene: &Scene, race: &Race) -> Result<Course, String> {
    let roads: Vec<&Road> = scene.roads.iter().filter(|r| r.drivability > 0.0 && r.nodes.len() >= 2).collect();
    let chains = chained(&roads);
    let (road, line, fit) = roads
        .iter()
        .copied()
        .chain(&chains)
        .map(|r| {
            let line = Line::spline(&r.nodes, r.looped, STEP);
            let fit = worst_distance(&line, race);
            (r, line, fit)
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .ok_or("no drivable DecalRoad in the level")?;
    let start = race.start().ok_or("the race has no start position")?;
    let order = race.ordered_nodes();
    if order.is_empty() {
        return Err("the race has no path from its start node".into());
    }
    let (s0, _) = line.project(start.pos);
    let total = line.length();
    let closed = race.closed && road.looped;
    // Direction: the arc positions of the nodes after the start should increase.
    let ahead = |s: f64, forward: bool| {
        let d = if forward { s - s0 } else { s0 - s };
        if closed {
            d.rem_euclid(total)
        } else {
            d
        }
    };
    let arcs: Vec<f64> = order.iter().map(|n| line.project(n.pos).0).collect();
    let rising = |forward: bool| arcs.windows(2).filter(|w| ahead(w[1], forward) > ahead(w[0], forward)).count();
    let forward = rising(true) >= rising(false);
    let length = if closed {
        total - LOOP_GAP
    } else {
        let last = *arcs.last().unwrap_or(&s0);
        ahead(last, forward)
    };
    if length <= 0.0 {
        return Err(format!("the race runs {length:.0} m along road {}", road.name));
    }
    let stage = line.stretch(s0, length, forward, STEP);
    let mut splits: Vec<f64> = order
        .iter()
        .zip(&arcs)
        .filter(|(n, _)| n.split)
        .map(|(_, &s)| ahead(s, forward))
        .filter(|&d| d > 50.0 && d < length - 50.0)
        .collect();
    splits.sort_by(f64::total_cmp);
    let (_, heading) = stage.sample(0.0);
    Ok(Course { road: road.name.clone(), fit, start: start.pos, heading, splits, closed, line: stage })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(name: &str, xs: &[f64]) -> Road {
        Road {
            name: name.into(),
            parent: String::new(),
            nodes: xs.iter().map(|&x| [x, 0.0, 0.0, 5.0]).collect(),
            looped: false,
            drivability: 1.0,
        }
    }

    #[test]
    fn overlapping_pieces_join_without_doubling_back() {
        // The first runs 3 m past the second's start; the second starts 3 m before the first's end.
        let (a, b) = (road("a", &[0.0, 20.0, 40.0, 43.0]), road("b", &[40.0, 60.0, 80.0]));
        let chain = &chained(&[&a, &b])[0];
        let xs: Vec<f64> = chain.nodes.iter().map(|n| n[0]).collect();
        assert_eq!(xs, [0.0, 20.0, 40.0, 60.0, 80.0]);
        // Ends a metre apart, in line: kept both.
        let (a, b) = (road("a", &[0.0, 20.0]), road("b", &[21.0, 40.0]));
        let xs: Vec<f64> = chained(&[&a, &b])[0].nodes.iter().map(|n| n[0]).collect();
        assert_eq!(xs, [0.0, 20.0, 21.0, 40.0]);
    }
}
