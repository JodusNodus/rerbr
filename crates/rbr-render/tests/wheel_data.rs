//! Every installed model's wheels at rest (`RBR_DATA`'s and `RBR_RSF`'s `Cars`): the drawn tarmac wheels at four
//! corners, faces out. RSF's Fiesta Rally2, i20 WRC 2020 and others leave their tarmac wheels at one point, turned for
//! one side (`car::WheelPlaces`); drawn there, the launcher's preview showed one wheel and three bare brake discs.

use glam::Vec3;
use rbr_formats::data::Data;
use rbr_formats::sgc;
use rbr_render::{car, coords};
use std::path::PathBuf;

/// Models whose tarmac wheels' normals point mostly in at their own nodes (by their rims they face out).
const INWARD_NORMALS: [&str; 2] = ["C2_R2_Max.sgc", "Skoda_130RS.sgc"];

#[test]
fn every_installed_model_draws_four_wheels_at_its_corners_facing_out() {
    let Some(data) = Data::from_env() else {
        eprintln!("skipping: RBR_DATA not set");
        return;
    };
    let rsf = std::env::var_os("RBR_RSF").map(PathBuf::from);
    let models: Vec<_> = [Some(data.install().to_path_buf()), rsf.clone()]
        .into_iter()
        .flatten()
        .filter_map(|root| std::fs::read_dir(root.join("Cars")).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|folder| std::fs::read_dir(folder.path()).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sgc")))
        .collect();
    let (mut checked, mut moved) = (0, 0);
    for path in &models {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(model) = sgc::parse(&std::fs::read(path).expect("model")) else { continue };
        let chained = sgc::chained_worlds(&model);
        let Some(places) = car::wheel_places(&model, &chained) else { continue };
        checked += 1;
        if places.rest != places.drawn {
            eprintln!("{file}: moved, turned {:?}", places.turned);
            moved += 1;
        }
        let [lf, rf, lb, rb] = places.rest;
        assert!(lf.x > 0.3 && lb.x > 0.3 && rf.x < -0.3 && rb.x < -0.3, "{file}: sides {:?}", places.rest);
        assert!(lf.z < lb.z - 1.0 && rf.z < rb.z - 1.0, "{file}: front and back {:?}", places.rest);
        if INWARD_NORMALS.contains(&file.as_str()) {
            continue;
        }
        for element in &model {
            let sgc::Element::Mesh(mesh) = element else { continue };
            let Some(k) =
                ["lf", "rf", "lb", "rb"].iter().position(|side| mesh.node.name == format!("e_wheel_tarmac_{side}"))
            else {
                continue;
            };
            let node = coords::part_to_car(chained[&mesh.node.id], mesh.node.transform.words());
            let placed = car::wheel_at_rest(k, &places, node);
            let normals: Vec3 = mesh.vertices.iter().map(|v| Vec3::from(v.normal)).sum();
            let out = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
            assert!(placed.transform_vector3(normals).x * out > 0.0, "{file} {}: faces in", mesh.node.name);
        }
    }
    eprintln!("{checked} models, {moved} with wheels moved to their rest places");
    // The retail cars' wheels are where their nodes are; RSF's stacked ones move.
    assert!(checked > 0 && (rsf.is_none() || moved > 0), "{checked} models, {moved} moved");
}
