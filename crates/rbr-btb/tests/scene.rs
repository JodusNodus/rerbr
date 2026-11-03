//! The draw-ready scene of every BTB folder of an RSF install (`RBR_RSF`) builds, and its textures resolve and read.
//! Skipped when `RBR_RSF` is unset. See `notes/btb.md` "Drawing".

use rbr_btb::scene::{self, Scene};
use rbr_btb::track;
use std::path::PathBuf;

#[test]
fn every_btb_folder_builds_a_scene() {
    let Some(root) = std::env::var_os("RBR_RSF").map(PathBuf::from) else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let tracks = track::scan(&root);
    assert!(!tracks.is_empty());
    for track in tracks {
        let track = track.unwrap_or_else(|(folder, e)| panic!("{}: {e}", folder.display()));
        assert!(track::is_track_folder(&track.folder), "{}", track.name);
        let scene = Scene::load(&track.folder).unwrap_or_else(|e| panic!("{}: {e}", track.name));
        assert!(!scene.objects.is_empty() && !scene.driveline.is_empty(), "{}", track.name);
        for mesh in &scene.meshes {
            let n = mesh.positions.len();
            assert_eq!(mesh.normals.len(), n, "{}/{}", track.name, mesh.name);
            assert!(mesh.indices.iter().all(|&i| (i as usize) < n), "{}/{}", track.name, mesh.name);
            for uv in &mesh.uvs {
                assert_eq!(uv.len(), n, "{}/{}", track.name, mesh.name);
            }
            let Some(material) = mesh.material else { continue };
            let material = &scene.materials[material];
            // The streams the effect reads are there (the collision meshes, with no UVs, carry none).
            if mesh.uvs.is_empty() {
                continue;
            }
            let slots = material.effect.uv_slots();
            if material.effect.vert_lerp {
                assert_eq!(mesh.blend.len(), n, "{}/{}", track.name, mesh.name);
                assert!(slots.lerp.is_some_and(|s| s < mesh.uvs.len()), "{}/{}", track.name, mesh.name);
            }
            if material.effect.bump {
                assert_eq!(mesh.tangents.len(), n, "{}/{}", track.name, mesh.name);
            }
        }
        // Each diffuse texture a mesh uses reads as a `.dds` (JPEGs and PNGs are decoded).
        let mut used: Vec<&str> = scene
            .meshes
            .iter()
            .filter_map(|m| m.material)
            .filter_map(|m| scene.materials[m].diffuse.as_deref())
            .collect();
        used.sort_unstable();
        used.dedup();
        for file in used {
            let bytes = scene.textures.read(file).unwrap_or_else(|| panic!("{}: {file} does not read", track.name));
            assert!(rbr_formats::dds::parse(&bytes).is_ok(), "{}: {file}", track.name);
        }
    }
}

#[test]
fn placing_follows_the_row_vector_convention() {
    // Translation in elements 12..14, rotation of +x onto +z (a quarter turn about y).
    let m = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 20.0, 30.0, 1.0];
    assert_eq!(scene::place_point(&m, [1.0, 0.0, 0.0]), [10.0, 20.0, 29.0]);
    assert_eq!(scene::place_normal(&m, [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]);
}

#[test]
fn effects_and_techniques_parse_from_their_names() {
    let e = scene::Effect::parse(Some("BTB_DiffuseBumpSpecularVertLerp"));
    assert!(e.bump && e.specular && e.vert_lerp && !e.multiply_add && !e.sky);
    let slots = e.uv_slots();
    assert_eq!((slots.bump, slots.specular, slots.lerp, slots.multiplier), (Some(1), Some(2), Some(3), None));
    let e = scene::Effect::parse(Some("BTB_DiffuseMultiplyAddVertLerp")).uv_slots();
    assert_eq!((e.multiplier, e.additive, e.lerp), (Some(1), Some(2), Some(3)));
    assert!(scene::Effect::parse(Some("BTB_SKY")).sky && scene::Effect::parse(Some("BTB_Sky")).sky);
    assert_eq!(scene::Effect::parse(None), scene::Effect::default());
    let t = scene::Technique::parse(Some("TransparentUp"));
    assert!(t.up && t.transparent);
    assert_eq!(scene::Technique::parse(Some("Default")), scene::Technique::default());
}
