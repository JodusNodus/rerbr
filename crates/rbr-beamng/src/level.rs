//! A level's scene: the objects of its `main/**/items.level.json` files (one JSON object per line, Torque's scene tree
//! written flat with `__parent` names). Kept: `TSStatic` shapes with their transforms, `DecalRoad` node lists (the AI
//! roads), and each object's group chain so hidden groups can be left out.

use crate::json::{self, Json};
use crate::zipfs::Archive;

/// A placed shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Static {
    pub name: String,
    pub parent: String,
    /// The level path of the shape (`/levels/.../x.dae`; the file read is its `.cdae`).
    pub shape: String,
    pub position: [f64; 3],
    /// Row-major 3x3, applied to column vectors.
    pub rotation: [[f64; 3]; 3],
    pub scale: [f64; 3],
    /// `collisionType`: `None` makes the shape non-solid.
    pub collision_type: Option<String>,
    /// `hidden`: never drawn in the game.
    pub hidden: bool,
    /// `isRenderEnabled` false: not drawn by the level's own account, though some levels set it on every shape they
    /// draw (Akina).
    pub render_disabled: bool,
}

impl Static {
    /// The object's transform, row-major 3x4: translation, rotation, scale applied to the shape's points.
    pub fn transform(&self) -> [[f64; 4]; 3] {
        std::array::from_fn(|i| {
            let r = self.rotation[i];
            [r[0] * self.scale[0], r[1] * self.scale[1], r[2] * self.scale[2], self.position[i]]
        })
    }

    /// A placed shape from its JSON `position`, `rotationMatrix` and `scale` (one number or three).
    pub fn placed(
        shape: &str,
        position: Option<&Json>,
        rotation_matrix: Option<&Json>,
        scale: Option<&Json>,
    ) -> Static {
        Static {
            name: String::new(),
            parent: String::new(),
            shape: shape.to_owned(),
            position: vec3(position, [0.0; 3]),
            rotation: rotation(rotation_matrix),
            scale: scale.and_then(Json::num).map_or_else(|| vec3(scale, [1.0; 3]), |n| [n; 3]),
            collision_type: None,
            hidden: false,
            render_disabled: false,
        }
    }

    /// The shape file the game loads for `shape`: the `.cdae` next to the `.dae`.
    pub fn cdae_path(&self) -> String {
        let s = self.shape.trim_start_matches('/');
        match s.rsplit_once('.') {
            Some((stem, ext)) if ext.eq_ignore_ascii_case("dae") => format!("{stem}.cdae"),
            _ => s.to_owned(),
        }
    }
}

/// A `DecalRoad`: its nodes `(x, y, z, width)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    pub name: String,
    pub parent: String,
    pub nodes: Vec<[f64; 4]>,
    pub looped: bool,
    /// `drivability` (AI roads have it above 0).
    pub drivability: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub statics: Vec<Static>,
    pub roads: Vec<Road>,
    /// The `SimGroup`s that are hidden or disabled.
    pub hidden_groups: Vec<String>,
    /// Every object's `(name, parent)` for the group chain.
    pub parents: Vec<(String, String)>,
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn vec3(v: Option<&Json>, default: [f64; 3]) -> [f64; 3] {
    match v.map(Json::nums) {
        Some(n) if n.len() == 3 => [n[0], n[1], n[2]],
        Some(n) if n.len() == 1 => [n[0]; 3],
        _ => default,
    }
}

/// `rotationMatrix`: nine numbers, the matrix's columns (Torque's `MatrixF` columns are the object's axes in the
/// world, and the level writes the axes one after another).
fn rotation(v: Option<&Json>) -> [[f64; 3]; 3] {
    match v.map(Json::nums) {
        Some(n) if n.len() == 9 => std::array::from_fn(|i| std::array::from_fn(|j| n[3 * j + i])),
        _ => IDENTITY,
    }
}

/// The levels a zip holds: the `levels/<name>/` folders with a `main/` scene.
pub fn names(archive: &Archive) -> Vec<String> {
    let mut out: Vec<String> = archive
        .names()
        .iter()
        .filter_map(|n| {
            let rest = n.strip_prefix("levels/")?;
            let (level, tail) = rest.split_once('/')?;
            tail.to_lowercase().starts_with("main/").then(|| level.to_owned())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

impl Scene {
    /// Reads every `items.level.json` under `levels/<level>/main/`.
    pub fn read(archive: &mut Archive, level: &str) -> Result<Scene, String> {
        let prefix = format!("levels/{level}/main/").to_lowercase();
        let mut files: Vec<String> = archive
            .names()
            .into_iter()
            .filter(|n| {
                let l = n.to_lowercase();
                l.starts_with(&prefix) && l.ends_with("items.level.json")
            })
            .collect();
        files.sort();
        let mut scene = Scene::default();
        for file in files {
            let text = archive.read_text(&file)?;
            for o in json::lines(&text).map_err(|e| format!("{file}: {e}"))? {
                scene.add(&o);
            }
        }
        Ok(scene)
    }

    fn add(&mut self, o: &Json) {
        let s = |k: &str| o.get(k).and_then(Json::str).unwrap_or_default().to_owned();
        let flag = |k: &str| o.get(k).and_then(Json::flag);
        let name = s("name");
        let parent = s("__parent");
        self.parents.push((name.clone(), parent.clone()));
        match s("class").as_str() {
            "SimGroup" => {
                if flag("hidden") == Some(true) || flag("enabled") == Some(false) {
                    self.hidden_groups.push(name);
                }
            }
            "TSStatic" => self.statics.push(Static {
                name,
                parent,
                shape: s("shapeName"),
                position: vec3(o.get("position"), [0.0; 3]),
                rotation: rotation(o.get("rotationMatrix")),
                scale: vec3(o.get("scale"), [1.0; 3]),
                collision_type: o.get("collisionType").and_then(Json::str).map(str::to_owned),
                hidden: flag("hidden") == Some(true),
                render_disabled: flag("isRenderEnabled") == Some(false),
            }),
            "DecalRoad" => self.roads.push(Road {
                name,
                parent,
                nodes: o
                    .get("nodes")
                    .map(Json::arr)
                    .unwrap_or_default()
                    .iter()
                    .map(Json::nums)
                    .filter(|n| n.len() >= 3)
                    .map(|n| [n[0], n[1], n[2], n.get(3).copied().unwrap_or(0.0)])
                    .collect(),
                looped: flag("looped").unwrap_or(false),
                drivability: o.get("drivability").and_then(Json::num).unwrap_or(0.0),
            }),
            _ => {}
        }
    }

    /// Whether `name`'s group chain passes a hidden group.
    pub fn in_hidden_group(&self, parent: &str) -> bool {
        let mut at = parent.to_owned();
        for _ in 0..64 {
            if at.is_empty() {
                return false;
            }
            if self.hidden_groups.contains(&at) {
                return true;
            }
            match self.parents.iter().find(|(n, _)| *n == at) {
                Some((_, p)) => at = p.clone(),
                None => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statics_roads_and_hidden_groups() {
        let text = concat!(
            "{\"name\":\"G\",\"class\":\"SimGroup\",\"__parent\":\"MissionGroup\",\"hidden\":true}\n",
            "{\"name\":\"a\",\"class\":\"TSStatic\",\"__parent\":\"G\",\"shapeName\":\"/levels/l/art/x.dae\",",
            "\"position\":[1,2,3],\"rotationMatrix\":[0,1,0,-1,0,0,0,0,1],\"scale\":[2,1,1]}\n",
            "{\"name\":\"r\",\"class\":\"DecalRoad\",\"__parent\":\"MissionGroup\",\"looped\":true,",
            "\"drivability\":1,\"nodes\":[[0,0,0,8],[10,0,0,8]]}\n",
        );
        let mut scene = Scene::default();
        for o in json::lines(text).unwrap() {
            scene.add(&o);
        }
        let a = &scene.statics[0];
        assert_eq!(a.cdae_path(), "levels/l/art/x.cdae");
        assert!(scene.in_hidden_group(&a.parent));
        // The object's x axis (first three numbers) points along world +y.
        let t = a.transform();
        assert_eq!([t[0][0], t[1][0], t[2][0]], [0.0, 2.0, 0.0]);
        assert_eq!([t[0][3], t[1][3], t[2][3]], [1.0, 2.0, 3.0]);
        assert!(scene.roads[0].looped);
        assert_eq!(scene.roads[0].nodes[1], [10.0, 0.0, 0.0, 8.0]);
    }
}
