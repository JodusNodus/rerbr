//! NGP 7.4.778's stage state: its stage init (`0x1004e360`, after the per-car init as the race starts) and its copies
//! of the collision system, supplied directly to the standalone's typed runtime settings.

use crate::mem::Mem;
use crate::stage::layout::{collision_system_ctor, COLLISION_SYSTEM, SYSTEM_SIZE};
use rbr_formats::ngp::Tables;

/// `0x1004e360` on the physics definition `def`: gravity, `MaxTimeStep` 1/720 s and the material table
/// (read from the DLL), and the timestep's copy in the replay object (`*0x893634`, `+0x18b908`) when it has
/// one. Outside the replay mode only (the game object's `+0x10` is 1 in it, and the step is then the replay's).
pub fn stage_init(mem: &mut Mem, def: u32, tables: &Tables) {
    let words = &tables.stage[usize::from(!REVISIONS.contains(&mem.u32(def + 0x1607c)))];
    for &(offset, word) in words {
        mem.set_u32(def + offset, word);
    }
    let replay = mem.u32(REPLAY);
    if replay != 0 && mem.try_read(replay + REPLAY_STEP, 4).is_some() {
        mem.set_u32(replay + REPLAY_STEP, mem.u32(def + MAX_TIME_STEP));
    }
}

/// A test surface (ours, not the game's): after [`stage_init`], every material the stage init wrote takes material
/// `id`'s tyre values (`CMaterial`'s friction, soil, bump, water and rolling-resistance reals, `myStatic` to
/// `RollingResistance`, and its `Type`, the surface class of the tyre's per-class factors), so the whole stage drives
/// on that one surface (`rbr-standalone --material-as`: 116 ices a stage for a brake test or a circle). The body's
/// collision values (`SolidGroundOffset`, `CollMy*`, `e*`) and `Flags` stay each material's. Only the values' words
/// move (`+0xc` past each key); the entries keep their own objects.
pub fn materials_as(mem: &mut Mem, def: u32, tables: &Tables, id: u32) {
    let words = &tables.stage[usize::from(!REVISIONS.contains(&mem.u32(def + 0x1607c)))];
    copy_material(mem, def, words, id);
}

/// [`materials_as`] over the stage init's `(offset, word)` list `words`.
fn copy_material(mem: &mut Mem, def: u32, words: &[(u32, u32)], id: u32) {
    let source = def + MATERIAL_FIRST + id * MATERIAL_ENTRY;
    let values: Vec<u32> = MATERIAL_KEYS.iter().map(|k| mem.u32(source + k + 0xc)).collect();
    let entries = words.iter().filter_map(|&(offset, _)| {
        let rel = offset.checked_sub(MATERIAL_FIRST + MY_STATIC + 0xc)?;
        (rel % MATERIAL_ENTRY == 0 && offset < MATERIAL_END).then_some(def + offset - MY_STATIC - 0xc)
    });
    for entry in entries.collect::<Vec<_>>() {
        for (k, v) in MATERIAL_KEYS.iter().zip(&values) {
            mem.set_u32(entry + k + 0xc, *v);
        }
    }
}

/// The material table in the physics definition: the manager at `+0x18`, its first entry at `+0xc`, entries `0x160`
/// apart (`data/physics-lsp-fields.md`, `CMaterialManager`, `CMaterial`), below the collision solver (`+0x16024`).
const MATERIAL_FIRST: u32 = 0x18 + 0xc;
const MATERIAL_ENTRY: u32 = 0x160;
const MATERIAL_END: u32 = 0x1_6024;
const MY_STATIC: u32 = 0x3c;
/// The tyre's `CMaterial` keys: the `CReal`s `myStatic` to `RollingResistance`, and `Type`.
const MATERIAL_KEYS: [u32; 13] = [0x3c, 0x4c, 0x5c, 0x6c, 0x7c, 0x8c, 0x9c, 0xac, 0xbc, 0xcc, 0xdc, 0xec, 0x13c];

/// The `PhysicsRevision_NGP` values that take the first table. 7.4.778 takes it for its own 778 only; RSF's
/// 7.5.779 with its 779 drives as the first does, and its `physics.rbz` differs from 778's only in that line, so
/// a 779 archive under 778's code drives as RSF does (rsf-input1, an RSF drive on that archive, exact only so).
const REVISIONS: [u32; 2] = [778, 779];

const REPLAY: u32 = 0x0089_3634;
const REPLAY_STEP: u32 = 0x18_b908;
const MAX_TIME_STEP: u32 = 0x1_608c;

/// NGP's three copies of the stage's collision system at `copies` (each [`SYSTEM_SIZE`] bytes): each constructed
/// with unlinked connections owned by the copy, then the system's words up to its connections (`+4..+0x1814`).
pub fn collision_copies(mem: &mut Mem, copies: [u32; 3]) {
    let system = mem.u32(COLLISION_SYSTEM);
    let words = mem.read(system + 4, 0x1810);
    for copy in copies {
        mem.map(copy, &vec![0; SYSTEM_SIZE as usize]);
        collision_system_ctor(mem, copy);
        mem.map(copy + 4, &words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_as_copies_one_material_s_tyre_values_over_every_written_entry() {
        let def = 0x0100_0000;
        let entry = |id: u32| def + MATERIAL_FIRST + id * MATERIAL_ENTRY;
        let mut mem = Mem::new();
        mem.map(def, &vec![0; 0x1_6100]);
        // Ids 1 and 5 written by the table (myStatic words), 5 the source; id 2 unwritten.
        let words: Vec<(u32, u32)> = [1u32, 5].iter().map(|id| (entry(*id) - def + MY_STATIC + 0xc, 0)).collect();
        for (n, k) in MATERIAL_KEYS.iter().enumerate() {
            mem.set_u32(entry(5) + k + 0xc, 100 + n as u32);
            mem.set_u32(entry(2) + k + 0xc, 7);
        }
        // The collision values and flags stay the entry's own.
        mem.set_u32(entry(1) + 0x4, 0xdead);
        mem.set_u32(entry(5) + 0x14c + 0xc, 5);
        mem.set_u32(entry(1) + 0x14c + 0xc, 1);
        copy_material(&mut mem, def, &words, 5);
        for (n, k) in MATERIAL_KEYS.iter().enumerate() {
            assert_eq!(mem.u32(entry(1) + k + 0xc), 100 + n as u32);
            assert_eq!(mem.u32(entry(2) + k + 0xc), 7);
        }
        assert_eq!(mem.u32(entry(1) + 0x4), 0xdead);
        assert_eq!(mem.u32(entry(1) + 0x14c + 0xc), 1);
    }
}
