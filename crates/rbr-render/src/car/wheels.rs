//! Where a model's wheels are drawn at rest (ours). The game places each wheel node by its animation, so a model may
//! leave a class's four nodes at one point: RSF's Fiesta Rally2 and WRC 2019, DS3 WRC, i20 R5, Rally2 and WRC 2017 to
//! 2021, Polo WRC 2016 and Lada VFTS stack the tarmac ones, the i20 WRC 2020 at its right rear wheel. Drawn there,
//! three corners showed only their brake discs (the launcher's preview, the viewer's car at rest), and the i20 WRC
//! 2020's left wheels and the Fiesta Rally2's right ones, turned for the other side, showed their backs in a drive.

use glam::{Mat4, Vec3};
use rbr_formats::sgc;
use rbr_recording::WHEELS;
use std::collections::HashMap;
use std::f32::consts::PI;

use super::{wheel_of, SURFACE};
use crate::coords;

/// Tyre nodes closer than this, m, are at one point: a class the model left stacked.
const STACKED_METRES: f32 = 0.2;
/// The tyre classes, the drawn one first.
const CLASSES: [&str; 3] = [SURFACE, "gravel", "snow"];
/// The hub point nodes, wheels 0 to 3 (the parents of `e_wheelgrunk_<side>`, in every installed model).
const HUBS: [&str; WHEELS] = ["Point_e_wheel_lf", "Point_e_wheel_rf", "Point_e_wheel_lb", "Point_e_wheel_rb"];

/// The car's tyre nodes, wheels 0 to 3, in the model frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelPlaces {
    /// The drawn (tarmac) wheels' nodes, where their meshes are modelled.
    pub drawn: [Vec3; WHEELS],
    /// Where each wheel sits at rest: the first class's (tarmac, gravel, snow) whose four nodes are apart, each taken
    /// by the hub point nearest it (the i20 WRC 2020's gravel `lb` and `rf` are at each other's corners), else
    /// [`WheelPlaces::drawn`].
    pub rest: [Vec3; WHEELS],
    /// A wheel moved to its rest place whose faces point mostly into the car there, so it is turned by pi about up.
    /// The game sets a wheel's turn itself (`CLM_CharacterCar::Unk1ba50`, LocoMotion 0x1001ba50), so a model may leave
    /// its stacked wheels all turned for one side: the i20 WRC 2020's face right, the Fiesta Rally2's left. A wheel
    /// left where it is keeps its node's turn ([`coords::part_to_car`]): of the 412 installed tarmac wheels at their
    /// own side, it faces the normals of all but the C2 R2 Max's and Skoda 130 RS's out.
    pub turned: [bool; WHEELS],
}

/// The [`WheelPlaces`] of a parsed model (`chained` its [`sgc::chained_worlds`]), or `None` without four tarmac tyre
/// nodes.
pub fn wheel_places(model: &[sgc::Element], chained: &HashMap<i32, [[f32; 4]; 3]>) -> Option<WheelPlaces> {
    let at = |node: &sgc::Node| {
        let world = chained.get(&node.id).copied().unwrap_or(node.world);
        Vec3::new(world[0][3], world[1][3], world[2][3])
    };
    // A class's wheel meshes (not their `_tyre_` children), wheels 0 to 3.
    let meshes = |class: &str| -> Option<[&sgc::Mesh; WHEELS]> {
        let mut meshes = [None; WHEELS];
        for element in model {
            if let sgc::Element::Mesh(mesh) = element {
                match wheel_of(&mesh.node.name) {
                    Some((c, k)) if c == class && !mesh.node.name.contains("_tyre_") => meshes[k] = Some(mesh),
                    _ => {}
                }
            }
        }
        meshes.into_iter().collect::<Option<Vec<_>>>()?.try_into().ok()
    };
    let class = |class: &str| meshes(class).map(|meshes| meshes.map(|mesh| at(&mesh.node)));
    let hubs: Option<[Vec3; WHEELS]> = HUBS
        .map(|name| {
            model.iter().find_map(|element| match element {
                sgc::Element::Node(node) if node.name == name => Some(at(node)),
                sgc::Element::Mesh(mesh) if mesh.node.name == name => Some(at(&mesh.node)),
                _ => None,
            })
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .and_then(|hubs| hubs.try_into().ok());
    let wheels = meshes(SURFACE)?;
    let drawn = wheels.map(|mesh| at(&mesh.node));
    let apart = |n: &[Vec3; WHEELS]| (0..WHEELS).all(|i| (i + 1..WHEELS).all(|j| n[i].distance(n[j]) > STACKED_METRES));
    let rest = CLASSES.into_iter().filter_map(class).find(apart).map_or(drawn, |nodes| match hubs {
        Some(hubs) => by_hub(nodes, hubs),
        None => nodes,
    });
    let turned = std::array::from_fn(|k| {
        let mesh = wheels[k];
        let world = chained.get(&mesh.node.id).copied().unwrap_or(mesh.node.world);
        let facing = coords::part_to_car(world, mesh.node.transform.words())
            .transform_vector3(mesh.vertices.iter().map(|v| Vec3::from(v.normal)).sum());
        rest[k] != drawn[k] && facing.x * side(k) < 0.0
    });
    Some(WheelPlaces { drawn, rest, turned })
}

/// The sign of wheel `k`'s side along the model's `x`: the left wheels' `+X`.
fn side(k: usize) -> f32 {
    if k.is_multiple_of(2) {
        1.0
    } else {
        -1.0
    }
}

/// Wheel `k`'s mesh placement (`node`, its node's [`coords::part_to_car`]) moved to the wheel's rest place, turned
/// by [`WheelPlaces::turned`].
pub fn wheel_at_rest(k: usize, places: &WheelPlaces, node: Mat4) -> Mat4 {
    if places.rest[k] == places.drawn[k] {
        return node;
    }
    let turn = if places.turned[k] { Mat4::from_rotation_y(PI) } else { Mat4::IDENTITY };
    Mat4::from_translation(places.rest[k]) * turn * Mat4::from_translation(-places.drawn[k]) * node
}

/// `nodes` reordered so that wheel `k` takes the node nearest hub `k`, when each hub has another; else as they are.
fn by_hub(nodes: [Vec3; WHEELS], hubs: [Vec3; WHEELS]) -> [Vec3; WHEELS] {
    let nearest = hubs.map(|hub| {
        (0..WHEELS).min_by(|&a, &b| nodes[a].distance(hub).total_cmp(&nodes[b].distance(hub))).expect("four nodes")
    });
    let distinct = (0..WHEELS).all(|i| (i + 1..WHEELS).all(|j| nearest[i] != nearest[j]));
    if distinct {
        nearest.map(|i| nodes[i])
    } else {
        nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swapped_names_go_to_the_hubs_they_sit_at() {
        // The i20 WRC 2020's gravel wheels: `lb` and `rf` at each other's corners.
        let nodes = [
            Vec3::new(0.82, 0.12, -2.55),
            Vec3::new(0.82, 0.12, 0.01),
            Vec3::new(-0.81, 0.13, -2.54),
            Vec3::new(-0.81, 0.13, 0.02),
        ];
        let hubs = [
            Vec3::new(0.72, 0.11, -2.63),
            Vec3::new(-0.70, 0.12, -2.63),
            Vec3::new(0.62, 0.08, -0.07),
            Vec3::new(-0.60, 0.08, -0.07),
        ];
        assert_eq!(by_hub(nodes, hubs), [nodes[0], nodes[2], nodes[1], nodes[3]]);
    }

    #[test]
    fn hubs_without_a_node_each_keep_the_names() {
        let nodes = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
        assert_eq!(by_hub(nodes, [Vec3::X; WHEELS]), nodes);
    }
}
