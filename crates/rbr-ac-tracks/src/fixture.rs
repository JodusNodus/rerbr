//! A synthetic Assetto Corsa track (one layout, `test`) for tests of this crate and of the launcher: a short road with
//! its timing gates, a spawn helper, an AI line and a hidden wall. Behind the `fixture` feature.
use crate::assetto::kn5::IDENTITY;
use std::path::Path;

pub fn word(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
pub fn floats(out: &mut Vec<u8>, values: &[f32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}
pub fn string(out: &mut Vec<u8>, value: &str) {
    word(out, value.len() as u32);
    out.extend(value.as_bytes());
}
pub fn node(out: &mut Vec<u8>, kind: u32, name: &str, children: u32) {
    word(out, kind);
    string(out, name);
    word(out, children);
    out.push(1);
}

pub fn fixture(root: &Path) {
    let mut out = b"sc6969".to_vec();
    word(&mut out, 6);
    word(&mut out, 0);
    // Inactive textures still carry a length field.
    word(&mut out, 2);
    word(&mut out, 0);
    string(&mut out, "unused.dds");
    word(&mut out, 0);
    let texture = rbr_btb_writer::textures::flat([0.3, 0.4, 0.5, 1.]);
    word(&mut out, 1);
    string(&mut out, "Road.DDS");
    word(&mut out, texture.len() as u32);
    out.extend(texture);
    word(&mut out, 1);
    string(&mut out, "road");
    string(&mut out, "ksPerPixel");
    out.extend([0; 6]);
    word(&mut out, 0);
    word(&mut out, 1);
    string(&mut out, "txDiffuse");
    word(&mut out, 0);
    string(&mut out, "road.dds");
    node(&mut out, 1, "ROOT", 9);
    floats(&mut out, &IDENTITY);
    for (name, x, z) in [
        ("AC_AB_START_L", -3., 20.),
        ("AC_AB_START_R", 3., 20.),
        ("AC_AB_FINISH_L", -3., 100.),
        ("AC_AB_FINISH_R", 3., 100.),
        ("AC_START_0", -1., 14.),
        ("AC_TIME_1_L", 0., 0.),
        ("AC_TIME_1_R", 0., 0.),
    ] {
        node(&mut out, 1, name, 0);
        *out.last_mut().unwrap() = 0; // Hidden timing helpers still define the gates.
        let mut matrix = IDENTITY;
        matrix[12..15].copy_from_slice(&[x, 0., z]);
        floats(&mut out, &matrix);
    }
    node(&mut out, 2, "01ROAD001", 0);
    out.extend([0, 1, 0]);
    word(&mut out, 242);
    for i in 0..=120 {
        for x in [-3., 3.] {
            floats(
                &mut out,
                &[x, 10. + i as f32 * 0.01, i as f32, 0., 1., -0.01, (x + 3.) / 6., i as f32 / 10., 1., 0., 0.],
            );
        }
    }
    word(&mut out, 720);
    for i in 0..120u16 {
        for index in [i * 2, i * 2 + 2, i * 2 + 1, i * 2 + 1, i * 2 + 2, i * 2 + 3] {
            out.extend(index.to_le_bytes());
        }
    }
    word(&mut out, 0);
    word(&mut out, 0);
    floats(&mut out, &[0., 2000., 0., 0., 0., 100.]);
    out.push(1);
    // An invisible wall: inactive, so AC hides it, and wound away from the road.
    node(&mut out, 2, "1WALL1", 0);
    *out.last_mut().unwrap() = 0;
    out.extend([0, 1, 0]);
    word(&mut out, 3);
    for (y, z) in [(10., 40.), (10., 50.), (14., 45.)] {
        floats(&mut out, &[4., y, z, -1., 0., 0., 0., 0., 0., 0., 1.]);
    }
    word(&mut out, 3);
    for index in [0u16, 2, 1] {
        out.extend(index.to_le_bytes());
    }
    word(&mut out, 0);
    word(&mut out, 0);
    floats(&mut out, &[0., 0., 0., 0., 0., 100.]);
    out.push(1);
    std::fs::create_dir_all(root.join("test/ai")).unwrap();
    std::fs::create_dir_all(root.join("test/data")).unwrap();
    std::fs::write(root.join("test.kn5"), &out).unwrap();
    std::fs::write(root.join("models_test.ini"), "[MODEL_0]\nFILE=test.kn5\nPOSITION=0,0,0\nROTATION=0,0,0\n").unwrap();
    std::fs::write(root.join("test/data/surfaces.ini"), "[SURFACE_0]\nKEY=ROAD\nFRICTION=0.94\n").unwrap();
    let mut ai = Vec::new();
    for n in [7, 25, 0, 0] {
        word(&mut ai, n);
    }
    for i in 0..25 {
        let z = i as f32 * 5.;
        floats(&mut ai, &[0., 10. + z * 0.01, z, z]);
        word(&mut ai, i);
    }
    std::fs::write(root.join("test/ai/fast_lane.ai"), ai).unwrap();
}
