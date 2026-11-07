//! Plausible params of one car per kind, for the tests and `examples/curves.rs`: not read from game files, made-up
//! torque curves on each kind's published figures (`/tmp/engine-model/cars.txt` magnitudes).

use crate::{Car, Params};

fn rad_s(rpm: f32) -> f32 {
    rpm * std::f32::consts::PI / 30.0
}

#[allow(clippy::too_many_arguments)]
fn params(
    folder: &str,
    group: &str,
    year: u32,
    knots: &[(f32, f32)],
    max_torque: f32,
    rev_limit: f32,
    idle: f32,
    brake: f32,
    turbo: bool,
    cylinder_volume: f32,
    inlet_area: f32,
    torque_cut: f32,
    inertia: f32,
) -> Params {
    Params {
        torque_knots: knots.iter().map(|&(n, tq)| (rad_s(n), tq)).collect(),
        max_torque,
        rev_limit: rad_s(rev_limit),
        idle_speed: rad_s(idle),
        brake_coefficient: brake,
        turbo,
        cylinder_volume,
        inlet_area,
        torque_cut,
        inertia,
        car: Car { folder: folder.into(), group: Some(group.into()), year: Some(year) },
    }
}

/// An NA R2: 1.6 L, DBW, one throttle, an ECU idle (208 R2 magnitudes).
pub fn na() -> Params {
    let knots =
        [(0.0, 0.35), (1000.0, 0.45), (3000.0, 0.7), (5000.0, 0.9), (6300.0, 1.0), (7500.0, 0.95), (8200.0, 0.85)];
    params("sample_na", "Group R2", 2012, &knots, 190.0, 8200.0, 1000.0, 0.30, false, 0.0004, 0.00199, 0.5, 0.14)
}

/// A Rally2: 1.6 L turbo, DBW, anti-lag, 32 mm restrictor (Fiesta Rally2 magnitudes).
pub fn turbo() -> Params {
    let knots = [
        (0.0, 0.45),
        (1000.0, 0.55),
        (2500.0, 0.82),
        (3200.0, 0.92),
        (4000.0, 0.98),
        (4400.0, 1.0),
        (5500.0, 0.8),
        (6500.0, 0.66),
        (7500.0, 0.55),
    ];
    params("sample_turbo", "Rally 2", 2023, &knots, 460.0, 7500.0, 1200.0, 0.25, true, 0.0004, 0.00174, 0.35, 0.13)
}

/// A Group 4 car of 1978: 2.0 L NA, cable, a carburettor choke per cylinder, a fixed idle stop (Fiat 131 magnitudes).
pub fn old() -> Params {
    let knots =
        [(0.0, 0.3), (1000.0, 0.45), (3000.0, 0.7), (4500.0, 0.88), (5600.0, 1.0), (7000.0, 0.9), (8600.0, 0.7)];
    params("sample_old", "Group 4", 1978, &knots, 226.0, 8600.0, 1000.0, 0.30, false, 0.000_498_8, 0.00182, 0.5, 0.17)
}

/// A Group A turbo of 1993: 2.0 L, cable, spring wastegate, anti-lag, 38 mm (Celica ST185 magnitudes).
pub fn cable_turbo() -> Params {
    let knots = [
        (0.0, 0.25),
        (1000.0, 0.3),
        (2000.0, 0.4),
        (3000.0, 0.8),
        (3500.0, 0.92),
        (4000.0, 1.0),
        (5500.0, 0.85),
        (7500.0, 0.55),
    ];
    params(
        "sample_cable_turbo",
        "Group A8",
        1993,
        &knots,
        460.0,
        7500.0,
        1000.0,
        0.25,
        true,
        0.000_499_5,
        0.00141,
        0.5,
        0.2,
    )
}

/// Every sample with its name.
pub fn all() -> [(&'static str, Params); 4] {
    [("na", na()), ("turbo", turbo()), ("old", old()), ("cable_turbo", cable_turbo())]
}
