//! Prints the surface materials NGP's stage init writes (`rbr_physics::ngp::layout::stage::stage_init`, the 778/779
//! table) for the given material ids: static and kinetic friction, their soil values, soil thickness, water factor
//! and rolling resistance. `cargo run -p rbr-tyre-lab --example materials -- NGP_DLL [ID...]` (default: the tarmac
//! and gravel ids). Values stay on stdout: nothing from the DLL is written to the repository.

use std::collections::BTreeMap;

/// Where a material's words sit in the physics definition: the manager at `+0x18`, its first entry at `+0xc`, entries
/// `0x160` apart, each `CReal`'s value 0xc past its key offset (`data/physics-lsp-fields.md`, `CMaterial`).
const FIRST_ENTRY: u32 = 0x18 + 0xc;
const ENTRY: u32 = 0x160;
const FIELDS: [(&str, u32); 8] = [
    ("myStatic", 0x3c),
    ("myKinetic", 0x4c),
    ("myStaticSoil", 0x5c),
    ("myKineticSoil", 0x6c),
    ("SinkFactor", 0x7c),
    ("SoilThickness", 0xac),
    ("WaterFactor", 0xdc),
    ("RollingResistance", 0xec),
];
const DEFAULT_IDS: [u32; 15] = [37, 38, 39, 45, 46, 47, 53, 54, 55, 65, 66, 67, 17, 18, 19];

fn main() {
    let mut args = std::env::args().skip(1);
    let dll = args.next().expect("usage: materials NGP_DLL [ID...]");
    let ids: Vec<u32> = args.map(|a| a.parse().expect("material id")).collect();
    let ids = if ids.is_empty() { DEFAULT_IDS.to_vec() } else { ids };
    let bytes = std::fs::read(&dll).unwrap_or_else(|e| panic!("{dll}: {e}"));
    let tables = rbr_formats::ngp::parse(&bytes).expect("NGP tables");
    let words: BTreeMap<u32, u32> = tables.stage[0].iter().copied().collect();
    print!("id");
    for (name, _) in FIELDS {
        print!("\t{name}");
    }
    println!();
    for id in ids {
        print!("{id}");
        for (_, key) in FIELDS {
            let at = FIRST_ENTRY + id * ENTRY + key + 0xc;
            match words.get(&at) {
                // SinkFactor is ~1e-7 on loose ground: print small values in full.
                Some(&w) => match f32::from_bits(w) {
                    v if v != 0.0 && v.abs() < 1e-3 => print!("\t{v:e}"),
                    v => print!("\t{v:.4}"),
                },
                None => print!("\t-"),
            }
        }
        println!();
    }
}
