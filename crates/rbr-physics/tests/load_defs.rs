//! The load's physics objects built from the retail files, as the load builds them (`rbr_physics::load`), against the
//! game's: the physics definition (its tweaks and bit-fields, the 256 materials and the collision solver's definition),
//! the registry's definitions (the car's, with its control unit, drive, engine, light system and passenger definitions;
//! the four wheels', with their spring damper and tyre definitions; the solids, the ground, the car body and the hinged
//! and mounted bodies), and every object of `CCompleteCarSetup` and `CCompleteCarSetupOptions`.
//!
//! The vectors hold the objects from the original's load dump of a Corolla gravel run (the dumps are retired):
//! `vectors/load-wheel-defs.txt` (the wheel definitions and the two setup owners, object by object),
//! `vectors/load-car-def.txt` (the car's definition and the 15 blocks its engine definition allocates), and
//! `vectors/load-registry-defs.txt` (the physics definition, part by part, and the registry's other definitions). The test builds them at the same addresses (the
//! heap returns the game's blocks, by name or in the game's order): the owners by their constructors, then each file's
//! root group walked on its owner: `Misc/prime.lsp`, `Physics/physics.lsp`, `Physics/t_coroll/common.lsp` and
//! `gravel.lsp` on the physics definition, `setups/d_gravel.lsp` on `CCompleteCarSetup`, `michelin/tyres.lsp` and
//! `r_gravel.lsp` on `CCompleteCarSetupOptions`. Then, with the race's tyre type 4 and the run's driver aids, it does
//! what the settings code does (the drive options' choices and the driver aids into the setups), builds the car
//! instance (below), and copies the setups into the definitions and the wheels as the car does. The vtables and name
//! words come from the retail exe. Runs only when `RBR_DATA` and `RBR_PATCH` are set.
//!
//! The car instance (step 4, `car_instance_matches_the_game`): the car's factory (`CCar::CDef::Vf50`), the four
//! wheels' (`E51Wheel::CDef::Vf50`), the four hinged bodies' (`CHingedRigidBody::CDef::Vf50`), the three mounted bodies'
//! (`CMountedRigidBody::CDef::Vf50`) and the car body's (`CCarBody::CDef::Vf50x`), called as the game called them, against
//! the same run's dumps at each factory's return (`vectors/load-car-instance.txt`):
//! the car and its exclusion set, the current car word, the five signal lists the car's connections join, each wheel,
//! and each body with its exclusion set. The heap returns the blocks the
//! run's allocation log (`RBR_ALLOCLOG`) shows inside the factory calls, in order, holding what they held at the
//! factory's entry: so every byte is compared, the ones no constructor writes included, apart from the name words.
//! The words of the game's the constructors read and nothing here builds come from the car factory's entry dump.
//!
//! In the definitions, every byte is compared except:
//! - the bytes nothing writes, found by building twice on blocks filled with 0x00 and 0xff: the entity definitions'
//!   link array `+0x1c..+0x11c` (64 registry indices; only the car's has links, and the counts at `+0x18` are
//!   compared) and the solid definitions' `+0x11c..+0x120`, the vectors' padding words (`+0xc` of each `CVector`), the
//!   gaps between tweaks, the wheel definitions' `+0x3dc` and tail (`+0x974..`), the car and car body definitions'
//!   tails, the mounted definitions' `+0x22c`. The game's blocks hold what the heap left there (random floats, text, and
//!   in the ground's link array matrices and pointers of an earlier object);
//! - the name words, which point into the game's token heap: the names they point to are compared instead;
//! - the bytes code not built here writes: the wheel definitions' `+0x880..+0x900`, the mount points in the car's frame
//!   that `E51Wheel::M3A8A0` (0x53a8a0) writes from the car body (`M3D030`, the car's update); the hinged definitions'
//!   `+0x310..+0x340` and `+0x3d0..+0x470`, the frames `SimEFE0` writes there (`VfE070` and `BodyCompose` outputs,
//!   `entities::layout::refresh_handles`; `+0x320` is the hinge point less the car's centre of gravity), and the mounted
//!   definitions' `+0x220..+0x22c`, the base point in the car body's frame that `E45_M1980` (0x521980,
//!   `physics_core::layout::mounted_pose`) has `CCar::BodyCompose` (0x4e6a30) write. `CCar::UpdateMrr` (0x4e9c70) runs both
//!   for each hinged and mounted body: the car instance's (step 4). And the physics definition's three bit-fields'
//!   `+0x10`: each points at a list of its bits (vtable 0x7473e8, the bit count at `+0x14`) for the tweak menu, which
//!   the bit-field's destructor releases as a `CBOOLMenuItem`. Not physics.

use rbr_formats::data::Data;
use rbr_formats::lsp::{self, Node};
use rbr_formats::{pe, rbz};
use rbr_physics::load::{self, Registry};
use rbr_physics::mem::Mem;
use rbr_physics::ntweak::{ApplyHost, Unsupported};
use rbr_physics::physics_core::layout::construct::Heap;
use rbr_physics::physics_core::layout::setup::DEF_SIZE;
use rbr_physics::{car_body, rigid_body};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

mod common;

use common::hex;

const VECTORS: [&str; 3] = [
    include_str!("vectors/load-wheel-defs.txt"),
    include_str!("vectors/load-car-def.txt"),
    include_str!("vectors/load-registry-defs.txt"),
];
/// The car instance at its factories' returns, from the same run's dumps, with the calls,
/// the blocks and the words of the game's the constructors read.
const INSTANCE: &str = include_str!("vectors/load-car-instance.txt");
const INSTANCE_LINES: [&str; 3] = ["call", "alloc", "env"];

/// Where the test puts the names and the solid definitions later files replace.
const NAMES: u32 = 0x1000_0000;
const REPLACED: u32 = 0x1200_0000;
/// The setup owners' sizes (up to the last member's end).
const COMPLETE_SIZE: u32 = 0x1700;
const OPTIONS_SIZE: u32 = 0x2790 + 7 * 0x6ac;
/// The race the vectors come from: the variant and its `physics.rbz`, the car's folder and surface, the tyre brand, the
/// tyre type the race chose (`TyreType04`, as 0x566250 returns it) and the driver aids (0x1660814, 0x1660ea8,
/// 0x1660ea0 and 0x1660ea4).
struct Race {
    physics: Option<std::path::PathBuf>,
    car: &'static str,
    surface: &'static str,
    brand: &'static str,
    tyre_type: u32,
    aids: load::DriverAids,
}

/// The reference run's: the Corolla on gravel, intermediate tyres.
const COROLLA: Race = Race {
    physics: None,
    car: "t_coroll",
    surface: "gravel",
    brand: "michelin",
    tyre_type: 4,
    aids: load::DriverAids { auto_gears: 0, gear_guard: 1, neutral_lock: 0, clutch_help: 1 },
};

impl Race {
    /// The files of `archive` (a data path): NGP's `physics.rbz` in its place under NGP.
    fn archive(&self, data: &Data, archive: &str) -> std::path::PathBuf {
        match &self.physics {
            Some(path) if archive == "Root/physics.rbz" => path.clone(),
            _ => data.resolve(archive).expect(archive),
        }
    }
}
/// The bytes code not built here writes, by kind: the car instance's frames in the definitions, and the tweak menu's
/// lists in the physics definition's bit-fields.
const LATER: [(&str, std::ops::Range<u32>); 7] = [
    ("wheel_def", 0x880..0x900),
    ("hinged_def", 0x310..0x340),
    ("hinged_def", 0x3d0..0x470),
    ("mounted_def", 0x220..0x22c),
    ("phys_tweaks", 0x60..0x64),
    ("phys_tweaks", 0x74..0x78),
    ("phys_tweaks", 0x88..0x8c),
];
/// One object of the vectors: its kind, position, address, bytes and name words (offset, name).
struct Object {
    kind: String,
    position: String,
    at: u32,
    bytes: Vec<u8>,
    names: Vec<(u32, Vec<u8>)>,
}

/// The objects of vector files: every line but the header and the instance file's `call`, `alloc` and `env` lines.
fn objects_of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<Object> {
    texts
        .into_iter()
        .flat_map(|v| v.lines())
        .filter(|l| !l.starts_with('#') && !INSTANCE_LINES.contains(&l.split(' ').next().unwrap()))
        .map(|l| {
            let f: Vec<_> = l.split(' ').collect();
            Object {
                kind: f[0].to_string(),
                position: f[1].to_string(),
                at: u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap(),
                bytes: hex(f[3]),
                names: f[4..]
                    .iter()
                    .map(|n| {
                        let (offset, name) = n.split_once('=').unwrap();
                        (u32::from_str_radix(offset, 16).unwrap(), hex(name))
                    })
                    .collect(),
            }
        })
        .collect()
}

fn objects() -> Vec<Object> {
    objects_of(VECTORS)
}

fn number(text: &str) -> u32 {
    u32::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
}

/// The instance file's lines of `kind`, split.
fn instance_lines(kind: &str) -> Vec<Vec<&'static str>> {
    INSTANCE.lines().map(|l| l.split(' ').collect::<Vec<_>>()).filter(|f| f[0] == kind).collect()
}

/// The heap of the car instance: the blocks the game's factories got, in the log's order (each size checked).
struct InstanceHeap(std::collections::VecDeque<(u32, u32)>);

impl InstanceHeap {
    fn take(&mut self, size: u32) -> u32 {
        let (logged, at) = self.0.pop_front().expect("a block the game allocated");
        assert_eq!(size, logged, "the size of the block at {at:#x}");
        at
    }
}

impl Heap for InstanceHeap {
    fn alloc(&mut self, _: &mut Mem, size: u32) -> u32 {
        self.take(size)
    }
}

impl rbr_physics::entities::exclusion::Heap for InstanceHeap {
    fn allocate(&mut self, bytes: u32) -> u32 {
        self.take(bytes)
    }
}

/// What the instance's blocks hold before the constructors run: a fill, or what the game's held.
#[derive(Clone, Copy)]
enum Fill {
    Byte(u8),
    Game,
}

/// The registry's factory for an instance call's kind, and the body factory (none for the car and the wheels).
fn factory_of(kind: &str) -> (&'static str, Option<load::BodyKind>) {
    match kind {
        "car" => ("Car", None),
        "wheel" => ("Wheel", None),
        "hinged" => ("HingedBody", Some(load::BodyKind::Hinged)),
        "mounted" => ("MountedBody", Some(load::BodyKind::Mounted)),
        "car_body" => ("CarBody", Some(load::BodyKind::CarBody)),
        _ => panic!("an instance call of kind {kind}"),
    }
}

/// The car instance on `mem` (step 4): the words of the game's the constructors read, the logged blocks filled, then
/// the factory calls in the game's order (the car, its wheels, its hinged, mounted and car bodies), each on the
/// definition the files made for its name. Returns the wheels.
fn build_instance(mem: &mut Mem, fill: Fill, registry: &Registry) -> [u32; 4] {
    for f in instance_lines("env") {
        mem.map(number(f[2]), &hex(f[3]));
    }
    let mut blocks = std::collections::VecDeque::new();
    for f in instance_lines("alloc") {
        let (size, at) = (number(f[1]), number(f[2]));
        match fill {
            Fill::Byte(b) => mem.map(at, &vec![b; size as usize]),
            Fill::Game => mem.map(at, &hex(f[3])),
        }
        blocks.push_back((size, at));
    }
    let mut heap = InstanceHeap(blocks);
    let mut wheels = Vec::new();
    for f in instance_lines("call") {
        let (kind, position) = (f[1], f[2]);
        let (def, a, b, result) = (number(f[3]), number(f[4]), number(f[5]), number(f[6]));
        let (factory, body) = factory_of(kind);
        let name = match kind {
            "car" => "CAR_ROOT".to_string(),
            "wheel" => format!("CAR_WHEEL_{position}"),
            _ => position.to_string(),
        };
        assert_eq!(registry.def(factory, &name), Some(def), "the {kind} {position}'s definition");
        let made = match (kind, body) {
            ("car", _) => load::new_car(mem, &mut heap, def, a, b),
            ("wheel", _) => load::new_wheel(mem, &mut heap, def, a, b),
            (_, Some(body)) => Ok(load::new_body(mem, &mut heap, body, def, a, b)),
            _ => unreachable!(),
        };
        assert_eq!(made, Ok(result), "the {kind} {position}");
        if kind == "wheel" {
            wheels.push(result);
        }
    }
    assert!(heap.0.is_empty(), "blocks left: {:x?}", heap.0);
    wheels.try_into().expect("four wheels")
}

/// The names on a bump arena, 16-byte aligned like the token heap; the heap blocks at the game's addresses, filled with
/// `fill`: a registry definition's by its name (the last name block, which the factory gets) and size, the others by
/// size in the order the game allocated them. The solid definitions `prime.lsp` makes for the names later files give to
/// another factory are not in the game's registry: they go to `REPLACED`.
struct Host {
    names: u32,
    last_name: u32,
    defs: BTreeMap<(Vec<u8>, u32), u32>,
    replaced: u32,
    blocks: BTreeMap<u32, Vec<u32>>,
    fill: u8,
}

impl ApplyHost for Host {
    fn alloc_name(&mut self, _: &mut Mem, len: u32) -> u32 {
        let at = self.names;
        self.names += len.next_multiple_of(0x10);
        self.last_name = at;
        at
    }

    /// Every object `load` hands on has 0x52f480 as its slot 17, which returns.
    fn after_parse(&mut self, _: &mut Mem, _: u32) {}

    /// `load::definitions` answers the registry's `Find`; the setups link nothing.
    fn def_index(&mut self, _: &Mem, _: &str) -> Option<u32> {
        None
    }

    /// `load::physics_file` makes the definitions; the setups make none.
    fn define(&mut self, _: &mut Mem, _: &str, _: u32, _: &str, _: &[Node]) -> Result<(), Unsupported> {
        unreachable!("the setup owners have no definition manager")
    }
}

impl Heap for Host {
    fn alloc(&mut self, mem: &mut Mem, size: u32) -> u32 {
        let name = c_string(mem, self.last_name);
        let at = match self.defs.get(&(name, size)) {
            Some(&at) => at,
            None if size == rigid_body::layout::DEF_SIZE => {
                self.replaced += size;
                self.replaced - size
            }
            None => self.blocks.get_mut(&size).expect("a block the game allocated").remove(0),
        };
        mem.map(at, &vec![self.fill; size as usize]);
        at
    }
}

/// The kinds of the registry's definitions the vectors hold (the car's and the wheels' come with
/// their owners).
const REGISTRY_KINDS: [&str; 5] = ["solid_def", "ground_def", "car_body_def", "hinged_def", "mounted_def"];

/// The registry definitions of the objects, by name and size.
fn registry_defs(objects: &[Object]) -> BTreeMap<(Vec<u8>, u32), u32> {
    objects
        .iter()
        .filter(|o| REGISTRY_KINDS.contains(&o.kind.as_str()))
        .map(|o| ((o.names[0].1.clone(), o.bytes.len() as u32), o.at))
        .collect()
}

fn lsp_root(data: &Data, race: &Race, archive: &str, name: &str) -> Vec<Node> {
    let path = race.archive(data, archive);
    let mut archive = rbz::open(&path).unwrap();
    let mut text = String::new();
    archive.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    let file = lsp::parse(&text).unwrap();
    load::root(&file).expect(name).to_vec()
}

/// The NUL-terminated string at `at`.
fn c_string(mem: &Mem, at: u32) -> Vec<u8> {
    let mut out = Vec::new();
    while out.last() != Some(&0) {
        out.push(mem.u8(at + out.len() as u32));
    }
    out
}

/// The heap blocks of the objects, by size in the game's allocation order: the physics definition, the car's
/// definition and its engine definition's blocks (from `misc/prime.lsp`), then the wheel definitions (from
/// `gravel.lsp`).
fn blocks(objects: &[Object]) -> BTreeMap<u32, Vec<u32>> {
    let mut blocks: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for object in objects {
        let size = match object.kind.as_str() {
            "phys_def" => DEF_SIZE,
            "car_def" => car_body::layout::DEF_SIZE,
            "wheel_def" => 0x980,
            "engine_block" => object.bytes.len() as u32,
            _ => continue,
        };
        blocks.entry(size).or_default().push(object.at);
    }
    blocks
}

/// The load's objects built from the files on the retail exe's sections, every block filled with `fill` first: the car
/// instance as its factories leave it, and everything after the setups' copies. Without `instance` (a race with no
/// instance vectors) there are no wheels to copy the wheels' setups through, so only the car's are copied.
fn build(data: &Data, race: &Race, objects: &[Object], fill: u8, instance: Option<Fill>) -> (Mem, Mem, Registry) {
    let exe = data.resolve("RichardBurnsRally_SSE.exe").expect("exe");
    let image = pe::parse(&std::fs::read(exe).unwrap()).unwrap();
    let mut mem = Mem::new();
    for name in [".rdata", ".data"] {
        let section = image.section(name).expect(name);
        mem.map(section.address, &section.bytes);
    }
    let first = |kind: &str| objects.iter().find(|o| o.kind == kind).unwrap().at;
    let (complete, options) = (first("complete_setup"), first("setup_options"));
    mem.map(complete, &vec![fill; COMPLETE_SIZE as usize]);
    mem.map(options, &vec![fill; OPTIONS_SIZE as usize]);
    let mut host = Host {
        names: NAMES,
        last_name: NAMES,
        defs: registry_defs(objects),
        replaced: REPLACED,
        blocks: blocks(objects),
        fill,
    };
    // The physics definition is allocated before any name.
    mem.map(NAMES, &[0]);
    load::construct_complete_setup(&mut mem, complete);
    load::construct_setup_options(&mut mem, options);

    let mut registry = Registry::default();
    let physics = load::new_physics_def(&mut mem, &mut host);
    let car = format!("Physics/{}/", race.car);
    for (archive, file) in [
        ("Root/misc.rbz", "Misc/prime.lsp".to_string()),
        ("Root/physics.rbz", "Physics/physics.lsp".to_string()),
        ("Root/physics.rbz", format!("{car}common.lsp")),
        ("Root/physics.rbz", format!("{car}{}.lsp", race.surface)),
    ] {
        let root = lsp_root(data, race, archive, &file);
        load::physics_file(&mut mem, &mut host, &mut registry, physics, &root).unwrap();
    }
    let def = |factory: &str| -> Vec<(String, u32)> {
        registry.entries().iter().filter(|e| e.factory == factory).map(|e| (e.name.clone(), e.def)).collect()
    };
    let wheels = def("Wheel");
    let names: Vec<_> = load::POSITIONS.iter().map(|p| format!("CAR_WHEEL_{p}")).collect();
    assert_eq!(wheels.iter().map(|(n, _)| n).collect::<Vec<_>>(), names.iter().collect::<Vec<_>>());
    let setups_file = lsp_root(data, race, "Root/physics.rbz", &format!("{car}setups/d_{}.lsp", race.surface));
    load::setup_file(&mut mem, &mut host, complete, &setups_file).unwrap();
    for file in [format!("Physics/{}/tyres.lsp", race.brand), format!("{car}r_{}.lsp", race.surface)] {
        let root = lsp_root(data, race, "Root/physics.rbz", &file);
        load::options_file(&mut mem, &mut host, options, &root).unwrap();
    }
    let setups = load::setups(complete);
    load::select_tyre_type(&mut mem, &setups, load::tyre_type(options, race.tyre_type));
    load::select_drive_options(&mut mem, setups.drive, load::drive_options(options));
    load::select_top_mounts(&mut mem, &setups, options);
    load::set_driver_aids(&mut mem, &setups, &race.aids);

    if let Some(fill) = instance {
        let wheel_at = build_instance(&mut mem, fill, &registry);
        let instance = mem.clone();
        load::copy_setups(&mut mem, wheel_at, &setups);
        let car_def = registry.def("Car", "CAR_ROOT").expect("the car's definition");
        load::copy_car_setups(&mut mem, car_def, &setups);
        return (instance, mem, registry);
    }
    let instance = mem.clone();
    let car_def = registry.def("Car", "CAR_ROOT").expect("the car's definition");
    load::copy_car_setups(&mut mem, car_def, &setups);
    (instance, mem, registry)
}

/// The bytes of `object` that differ in `mem`, outside `skip` (offsets) and its name words; its names that differ.
fn differences(mem: &Mem, object: &Object, skip: &BTreeSet<u32>) -> Vec<String> {
    let ours = mem.read(object.at, object.bytes.len());
    let mut out = Vec::new();
    for (offset, name) in &object.names {
        let mine = c_string(mem, mem.u32(object.at + offset));
        if mine != *name {
            out.push(format!(
                "+{offset:#x}: name {:?}, the game's {:?}",
                String::from_utf8_lossy(&mine),
                String::from_utf8_lossy(name)
            ));
        }
    }
    for (i, (a, b)) in ours.iter().zip(&object.bytes).enumerate() {
        let offset = i as u32;
        if a != b && !skip.contains(&offset) {
            out.push(format!("+{offset:#x}: ours {a:02x} the game's {b:02x}"));
        }
    }
    out
}

/// Compares `objects` with the build on 0x00 and 0xff fills (`zeros`, `ones`), outside `later` and the bytes the two
/// builds disagree on; prints the counts by kind (compared, never written, in a name word, written by a step not built
/// here) and panics on a difference.
fn compare(objects: &[Object], zeros: &Mem, ones: &Mem, later: &[(&str, std::ops::Range<u32>)]) {
    let mut counts: BTreeMap<&str, [u32; 4]> = BTreeMap::new();
    let mut differing = Vec::new();
    for object in objects {
        let len = object.bytes.len() as u32;
        let a = zeros.read(object.at, len as usize);
        let b = ones.read(object.at, len as usize);
        let count = counts.entry(object.kind.as_str()).or_default();
        let mut skip = BTreeSet::new();
        for offset in 0..len {
            let class = if later.iter().any(|(kind, range)| object.kind == *kind && range.contains(&offset)) {
                3
            } else if a[offset as usize] != b[offset as usize] {
                1
            } else if object.names.iter().any(|(o, _)| (*o..*o + 4).contains(&offset)) {
                2
            } else {
                0
            };
            count[class] += 1;
            if class != 0 {
                skip.insert(offset);
            }
        }
        differing.extend(
            differences(zeros, object, &skip).into_iter().map(|d| format!("{} {} {d}", object.kind, object.position)),
        );
    }
    for (kind, [compared, unwritten, named, later]) in &counts {
        eprintln!(
            "{kind}: {compared} bytes compared; excluded: {unwritten} never written, {named} in name words, \
             {later} written by a step not built here"
        );
    }
    assert!(
        differing.is_empty(),
        "{} bytes differ:\n{}",
        differing.len(),
        differing.iter().take(200).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The same definitions under NGP: NGP's `physics.rbz` (`RBR_NGP_PHYSICS`, with its cars in their slots) parsed with
/// NGP's keys, against the objects cut from an NGP load dump (`RBR_NGP_LOAD`: a folder with
/// `load-wheel-defs.txt`, `load-car-defs.txt` and `load-registry-defs.txt`). The reference is `ngp-rpl11`'s snapshot
/// at 6438: the Fiesta Rally3 in slot 5 (`s_i2003`, pirelli) on stage 11's gravel.
#[test]
fn ngp_definitions_match_the_game() {
    let (Some(data), Some(physics), Some(vectors)) = (
        Data::from_env().filter(|d| d.patch().is_some()),
        std::env::var_os("RBR_NGP_PHYSICS"),
        std::env::var_os("RBR_NGP_LOAD"),
    ) else {
        eprintln!("skipping: RBR_DATA, RBR_PATCH, RBR_NGP_PHYSICS or RBR_NGP_LOAD not set");
        return;
    };
    let dir = std::path::PathBuf::from(vectors);
    let texts: Vec<&'static str> = ["load-wheel-defs.txt", "load-car-defs.txt", "load-registry-defs.txt"]
        .iter()
        .map(|f| &*std::fs::read_to_string(dir.join(f)).unwrap().leak())
        .collect();
    let objects = objects_of(texts);
    let race = Race {
        physics: Some(physics.into()),
        car: "s_i2003",
        surface: "snow",
        brand: "pirelli",
        tyre_type: 3,
        ..COROLLA
    };
    let ((_, zeros, _), (_, ones, _)) =
        (build(&data, &race, &objects, 0, None), build(&data, &race, &objects, 0xff, None));
    let later: Vec<_> =
        LATER.iter().chain(&NGP_LATER).cloned().chain(ngp_torque_rpms()).chain(wheel_setup_copy(&data)).collect();
    compare(&objects, &zeros, &ones, &later);
}

/// The bytes NGP writes after the files (layer 2 of `notes/ngp.md`'s plan, not built here), and one word that holds an
/// address in NGP's memory: its copy of `CUnsigned`'s vtable at the drive definition's +0x58c (`NGP_TABLE_VTABLE`, the
/// copy's address in NGP's file here). NGP's per-car init (`0x1003f5b0`) rewrites the drive type (+0x153c), the engine
/// definition's tables (+0x1bac..+0x3bc8, `0x1005ab10` scaling them), the words after `CylinderVolume_NGP`, the engine
/// type (`EngineType_NGP` 11 becomes 1) and the real at +0x40ac, and scales the wheels' and tyres' values (by the car's
/// factor, 1.00705 for this car); the materials' grip comes from its per-stage tables; `MaxTimeStep` becomes 1/720
/// (`0x1004e3a7`).
const NGP_LATER: [(&str, std::ops::Range<u32>); 19] = [
    ("car_def", 0x1488..0x148c),
    ("car_def", 0x153c..0x1540),
    ("car_def", 0x1bac..0x3bc8),
    ("car_def", 0x3fe8..0x4024),
    ("car_def", 0x4088..0x408c),
    ("car_def", 0x40ac..0x40b0),
    ("wheel_def", 0x538..0x53c),
    ("wheel_def", 0x5b8..0x5cc),
    ("wheel_def", 0x608..0x60c),
    ("wheel_def", 0x678..0x698),
    ("wheel_def", 0x6b8..0x6d8),
    ("wheel_def", 0x6f0..0x710),
    ("wheel_def", 0x730..0x7b0),
    ("wheel_def", 0x7dc..0x7e0),
    ("wheel_def", 0x810..0x81c),
    ("wheel_def", 0x930..0x944),
    ("wheel_def", 0x960..0x974),
    ("material", 0x18..0x15c),
    ("phys_tweaks", 0x1c..0x20),
];

/// The values of `Tq_01_RPM_NGP` to `Tq_15_RPM_NGP` (engine definition +0x2270, 0x20 apart), which NGP's per-car init
/// turns from rpm into rad/s.
fn ngp_torque_rpms() -> impl Iterator<Item = (&'static str, std::ops::Range<u32>)> {
    (0..15).map(|k| ("car_def", 0x3c00 + 0x20 * k..0x3c04 + 0x20 * k))
}

/// The bytes of a wheel definition `load::copy_setups` writes, from the 1.02 build, which has the car instance the copy
/// needs: the NGP test builds none, so it leaves them out.
fn wheel_setup_copy(data: &Data) -> Vec<(&'static str, std::ops::Range<u32>)> {
    let objects = objects();
    let mut offsets = BTreeSet::new();
    for fill in [0, 0xff] {
        let (before, after, _) = build(data, &COROLLA, &objects, fill, Some(Fill::Byte(fill)));
        for wheel in objects.iter().filter(|o| o.kind == "wheel_def") {
            let len = wheel.bytes.len();
            let (a, b) = (before.read(wheel.at, len), after.read(wheel.at, len));
            offsets.extend((0..len as u32).filter(|&k| a[k as usize] != b[k as usize]));
        }
    }
    offsets.into_iter().map(|k| ("wheel_def", k..k + 1)).collect()
}

#[test]
fn car_instance_matches_the_game() {
    let Some(data) = Data::from_env().filter(|d| d.patch().is_some()) else {
        eprintln!("skipping: RBR_DATA or RBR_PATCH not set");
        return;
    };
    let objects = objects();
    let instance = objects_of([INSTANCE]);
    let ((zeros, _, _), (ones, _, _)) = (
        build(&data, &COROLLA, &objects, 0, Some(Fill::Byte(0))),
        build(&data, &COROLLA, &objects, 0xff, Some(Fill::Byte(0xff))),
    );
    let mut kept: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for object in &instance {
        let len = object.bytes.len();
        let (a, b) = (zeros.read(object.at, len), ones.read(object.at, len));
        let count = kept.entry(object.kind.as_str()).or_default();
        count.0 += a.iter().zip(&b).filter(|(a, b)| a != b).count();
        count.1 += len;
    }
    for (kind, (kept, len)) in kept {
        eprintln!("{kind}: {kept} of {len} bytes not written (or copied from such bytes)");
    }
    let (game, _, _) = build(&data, &COROLLA, &objects, 0, Some(Fill::Game));
    compare(&instance, &game, &game, &[]);
}
