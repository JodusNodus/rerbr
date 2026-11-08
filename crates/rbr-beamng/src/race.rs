//! A time-trial mission's race path (`race.race.json`): path nodes (checkpoints, `useAsSplit`), segments between them
//! by node id, start positions (position and rotation quaternion), the start node and whether the race is closed (a
//! loop). Ids are the entries' `oldId`.

use crate::json::{self, Json};

#[derive(Debug, Clone, PartialEq)]
pub struct PathNode {
    pub id: i64,
    pub name: String,
    pub pos: [f64; 3],
    pub radius: f64,
    pub split: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartPosition {
    pub id: i64,
    pub name: String,
    pub pos: [f64; 3],
    /// `(x, y, z, w)`.
    pub rot: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Race {
    pub nodes: Vec<PathNode>,
    /// `(from, to)` node ids.
    pub segments: Vec<(i64, i64)>,
    pub starts: Vec<StartPosition>,
    pub start_node: i64,
    pub default_start: i64,
    pub closed: bool,
}

impl Race {
    pub fn parse(text: &str) -> Result<Race, String> {
        let doc = json::parse(text)?;
        let num = |o: &Json, k: &str| o.get(k).and_then(Json::num);
        let id = |o: &Json| num(o, "oldId").map_or(-1, |v| v as i64);
        let pos = |o: &Json| -> Result<[f64; 3], String> {
            let n = o.get("pos").map(Json::nums).unwrap_or_default();
            (n.len() == 3).then(|| [n[0], n[1], n[2]]).ok_or_else(|| "race: an entry without pos".to_owned())
        };
        let name = |o: &Json| o.get("name").and_then(Json::str).unwrap_or_default().to_owned();
        let list = |k: &str| doc.get(k).map(Json::arr).unwrap_or_default();
        let nodes = list("pathnodes")
            .iter()
            .map(|o| {
                Ok(PathNode {
                    id: id(o),
                    name: name(o),
                    pos: pos(o)?,
                    radius: num(o, "radius").unwrap_or(0.0),
                    split: o.get("useAsSplit").and_then(Json::flag).unwrap_or(false),
                })
            })
            .collect::<Result<_, String>>()?;
        let segments =
            list("segments").iter().filter_map(|o| Some((num(o, "from")? as i64, num(o, "to")? as i64))).collect();
        let starts = list("startPositions")
            .iter()
            .map(|o| {
                let r = o.get("rot").map(Json::nums).unwrap_or_default();
                let rot = if r.len() == 4 { [r[0], r[1], r[2], r[3]] } else { [0.0, 0.0, 0.0, 1.0] };
                Ok(StartPosition { id: id(o), name: name(o), pos: pos(o)?, rot })
            })
            .collect::<Result<_, String>>()?;
        let closed = doc.get("classification").and_then(|c| c.get("closed")).and_then(Json::flag).unwrap_or(false);
        Ok(Race {
            nodes,
            segments,
            starts,
            start_node: num(&doc, "startNode").map_or(-1, |v| v as i64),
            default_start: num(&doc, "defaultStartPosition").map_or(-1, |v| v as i64),
            closed,
        })
    }

    /// A race along `points` (a road's nodes, in driving order) for a level without one: path nodes 20 m in from
    /// each end and at the middle, the start position on the first.
    pub fn along(points: &[[f64; 3]]) -> Result<Race, String> {
        let plan = |a: &[f64; 3], b: &[f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let mut arc = vec![0.0];
        for w in points.windows(2) {
            arc.push(arc.last().unwrap() + plan(&w[0], &w[1]));
        }
        let total = *arc.last().ok_or("an empty road")?;
        if total < 100.0 {
            return Err(format!("the road is {total:.0} m long"));
        }
        let at = |s: f64| points[arc.iter().position(|&a| a >= s).unwrap_or(points.len() - 1)];
        let node = |id: i64, pos: [f64; 3]| PathNode { id, name: format!("node {id}"), pos, radius: 5.0, split: false };
        Ok(Race {
            nodes: vec![node(0, at(20.0)), node(1, at(total * 0.5)), node(2, at(total - 20.0))],
            segments: vec![(0, 1), (1, 2)],
            starts: vec![StartPosition { id: 0, name: "start".into(), pos: at(20.0), rot: [0.0, 0.0, 0.0, 1.0] }],
            start_node: 0,
            default_start: 0,
            closed: false,
        })
    }

    /// The path nodes in driving order from the start node, following the segments (each node once).
    pub fn ordered_nodes(&self) -> Vec<&PathNode> {
        let mut out = Vec::new();
        let mut at = self.start_node;
        while let Some(node) = self.nodes.iter().find(|n| n.id == at) {
            if out.iter().any(|n: &&PathNode| n.id == at) {
                break;
            }
            out.push(node);
            match self.segments.iter().find(|s| s.0 == at) {
                Some(s) => at = s.1,
                None => break,
            }
        }
        out
    }

    /// The default start position.
    pub fn start(&self) -> Option<&StartPosition> {
        self.starts.iter().find(|s| s.id == self.default_start).or(self.starts.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_follow_the_segments_from_the_start() {
        let race = Race::parse(
            r#"{"classification": {"closed": true}, "startNode": 6, "defaultStartPosition": 2,
            "pathnodes": [{"oldId": 5, "name": "a", "pos": [0,0,0], "useAsSplit": true},
                          {"oldId": 6, "name": "b", "pos": [1,0,0]}],
            "segments": [{"from": 5, "to": 6}, {"from": 6, "to": 5}],
            "startPositions": [{"oldId": 2, "name": "s", "pos": [1,2,3], "rot": [0,0,0,1]}]}"#,
        )
        .unwrap();
        let order: Vec<i64> = race.ordered_nodes().iter().map(|n| n.id).collect();
        assert_eq!(order, vec![6, 5]);
        assert!(race.closed && race.nodes[0].split);
        assert_eq!(race.start().unwrap().pos, [1.0, 2.0, 3.0]);
    }
}
