//! Checks the `object!` tables on snapshots: `lower(lift(image)) == image` on every owned byte of the car's tabled
//! objects (`rbr_physics::store::car_objects`), and of the struts', the VCU's, the drive's, the engine's, the tyres', the
//! wheels', the car's own and the collision's typed state (`spring_damper::layout::round_trip`, `vcu::layout::round_trip`,
//! `engine_driveline::layout::{drive_round_trip, engine_round_trip}`, `tyre::layout::round_trip`,
//! `wheel::layout::round_trip`, `car_body::layout::round_trip`, `collision::layout::round_trip`) and of the physics
//! core's (`physics_core::layout::typed`); the typed stage's bytes and queries against the image's
//! (`stage::layout::round_trip`, `differential`) and the typed definitions' answers (`store::defs::differential`); the
//! wheel's, the engine's, the car's, the collision's and the core's entries per call against held
//! (`wheel::layout::differential`, `engine_driveline::layout::engine_differential`, `car_body::layout::differential`,
//! `collision::layout::differential`, `physics_core::layout::typed::differential`); the car's solids' round trip and
//! their host-free entries per call against held (`rigid_body::layout::{round_trip, differential}`).
//!
//! Usage: round_trip SNAPSHOT...

use rbr_physics::car_body::store::CarStore;
use std::process::ExitCode;

use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::car_body::layout as car_body;
use rbr_physics::collision::layout as collision;
use rbr_physics::engine_driveline::layout as drive;
use rbr_physics::physics_core::layout::typed as core;
use rbr_physics::rigid_body::layout as rigid;
use rbr_physics::snapshot::Snapshot;
use rbr_physics::spring_damper::layout as spring;
use rbr_physics::stage::layout as stage;
use rbr_physics::store;
use rbr_physics::tyre::layout as tyre;
use rbr_physics::vcu::layout as vcu;
use rbr_physics::wheel::layout as wheel;

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: round_trip SNAPSHOT...");
        return ExitCode::from(2);
    }
    let mut ok = true;
    for path in &paths {
        let snap = match Snapshot::open(path) {
            Ok(snap) => snap,
            Err(e) => {
                println!("{path}: {e}");
                ok = false;
                continue;
            }
        };
        let mem = snap.mem();
        let car = mem.u32(CAR_PTR);
        let objects = store::car_objects(&mem, car);
        // The owners the tables name: each strut state's owner is its wheel, the VCU state's is the car.
        let wheels = mem.wheel_pointers(car);
        for o in &objects {
            let owner = match o.name.as_str() {
                "vcu state" => Some((vcu::StateObj::owner.load(&mem, o.at), car)),
                name if name.ends_with("strut state") => {
                    let k = usize::from(name.as_bytes()[5] - b'0');
                    Some((spring::StateObj::owner.load(&mem, o.at), wheels[k]))
                }
                _ => None,
            };
            if let Some((found, expected)) = owner.filter(|(found, expected)| found != expected) {
                println!("{path}: {} at {:#010x}: owner {found:#010x}, expected {expected:#010x}", o.name, o.at);
                ok = false;
            }
        }
        match store::round_trip(&mem, &objects) {
            Ok(bytes) => println!("{path}: {} objects, {bytes} owned bytes equal after lower(lift)", objects.len()),
            Err(e) => {
                println!("{path}: {e}");
                ok = false;
            }
        }
        let trips = [
            ("struts", spring::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("vcu", vcu::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("drive", drive::drive_round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("engine", drive::engine_round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("tyres", tyre::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("wheels", wheel::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("car", car_body::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("core", core::round_trip(&mem), "typed bytes equal after lower(lift)"),
            ("collision", collision::round_trip(&mem), "typed bytes equal after lower(lift)"),
            ("solids", rigid::round_trip(&mem, car), "typed bytes equal after lower(lift)"),
            ("stage", stage::round_trip(&mem), "typed bytes equal to the image's"),
            ("stage queries", stage::differential(&mem), "answer bytes equal to the image's"),
            ("definitions", store::defs::differential(&mem, car), "answer bytes equal to the image's"),
            (
                "engine entries",
                drive::engine_differential(&mut snap.mem(), &mut snap.mem(), car),
                "bytes equal per call and held",
            ),
            (
                "wheel entries",
                // The outputs go below the snapshot's stack pointer.
                wheel::differential(&mut snap.mem(), &mut snap.mem(), car, snap.metadata().registers.esp - 0x400),
                "bytes equal per call and held",
            ),
            (
                "car entries",
                car_body::differential(&mut snap.mem(), &mut snap.mem(), car, snap.metadata().registers.esp - 0x400),
                "bytes equal per call and held",
            ),
            (
                "core entries",
                // The entries' frames go below the snapshot's stack pointer.
                core::differential(&mut snap.mem(), &mut snap.mem(), snap.metadata().registers.esp - 0x400),
                "bytes equal per call and held",
            ),
            (
                "solid entries",
                rigid::differential(&mut snap.mem(), &mut snap.mem(), car, snap.metadata().registers.esp - 0x400),
                "bytes equal per call and held",
            ),
            (
                "collision entries",
                collision::differential(&mut snap.mem(), &mut snap.mem(), car, snap.metadata().registers.esp - 0x1000),
                "bytes equal per call and held",
            ),
        ];
        for (name, trip, what) in trips {
            match trip {
                Ok(bytes) => println!("{path}: {name}, {bytes} {what}"),
                Err(e) => {
                    println!("{path}: {name}: {e}");
                    ok = false;
                }
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
