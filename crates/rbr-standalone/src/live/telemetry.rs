//! `--telemetry-udp ADDR:PORT`: NGP's UDP telemetry packet (`udpTelemetry`, `udpTelemetryEndpoints`, default
//! 127.0.0.1:6776), so SimHub, motion rigs, dashboards and bass shakers read ReRBR as they read NGP. The packet is
//! NGP's 664-byte packed little-endian record (its SDK's `TelemetryData`), written field by field here.
//!
//! Each field's source is the memory NGP reads (`FUN_100cce60` in NGP.dll 7.4.778, traced in Ghidra; `notes/ngp.md`,
//! "UDP telemetry"); every one is exe memory the step, the input filter or the HUD keeps, none is NGP's own. Ours:
//! one packet per live frame after its last step (NGP sends one per driving substep, before its step), and none while
//! a replay plays, in the countdown, the pause or past the race's end, as NGP. The step count is ours (driving steps
//! since the stage's start), and the controls are the car control the frame's last step took, which is the filter's
//! output NGP reads unless a recorded payload drives. Reads the physics memory and the game; writes neither.

use std::net::{SocketAddr, UdpSocket};

use rbr_gameplay::driveline;
use rbr_gameplay::game::Game;
use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::car_body::store::CarStore;
use rbr_physics::engine_driveline::layout::DriveState;
use rbr_physics::mem::Mem;
use rbr_physics::rigid_body::layout::{
    ACCEL_ANGULAR_EST, ACCEL_LINEAR_EST, ANGULAR_VELOCITY, LINEAR_VELOCITY, ORIENTATION, POSITION,
};
use rbr_physics::spring_damper::store::SpringStore;
use rbr_physics::tyre::layout::{STATE as TYRE_STATE, STATE_INNER_AIR_TEMPERATURE};
use rbr_physics::tyre::store::TyreStore;
use rbr_physics::wheel::layout::{STRUT, STRUT_LENGTH, TYRE_SCRATCH};
use rbr_recording::sound::{CAR_ENGINE, CAR_WHEELS, ENGINE_STATE, WHEEL_STRUT_VELOCITY};
use rbr_recording::CONTROL_BYTES;

use super::picture::{hud_gear, CAR_DRIVE, CAR_DRIVELINE_PARAMETER, DRIVE_STATE, STATE_ENGINE_SPEED};

/// The packet's length, bytes.
pub const PACKET_BYTES: usize = 664;

/// NGP's radians to degrees (`0x404ca5dc0d03cf26`), applied in doubles.
const DEGREES: f64 = 57.29577791868205;
/// The HUD's engine speed to rpm (`0x573ced`).
pub(super) const RPM: f32 = 9.549_296;
/// The HUD's m/s to km/h (`0x573cc0`; the game's unit option gives mph, 2.2369363, which ReRBR does not have).
const KMH: f32 = 3.6;
/// The g NGP divides the linear accelerations by.
const G: f32 = 9.81;
/// `CCar` +0x520 and +0x530: the footbrake and handbrake pressures (`rbr_physics::ngp::brake::BrakeIn`'s inputs).
const CAR_FOOTBRAKE_PRESSURE: u32 = 0x520;
const CAR_HANDBRAKE_PRESSURE: u32 = 0x530;
/// Each wheel's brake disc words in the car (`CCar` +0xb90 +0x48 + 200 i): the friction layer's and the core's
/// temperatures, microkelvin, and the pad's wear counter (NGP scales it by `0x338f2a63`).
const CAR_BRAKE_LAYER_TEMPERATURE: u32 = 0xc54;
const CAR_BRAKE_TEMPERATURE: u32 = 0xc64;
const CAR_BRAKE_WEAR: u32 = 0xc9c;
const BRAKE_STRIDE: u32 = 0xc8;
const BRAKE_WEAR_SCALE: f32 = 6.666_666_5e-8;
/// The engine state's coolant temperatures, K: the radiator's (+0x20, named by NGP's packet) and the engine's
/// (+0x28, `coolant_temperature`'s), and the engine's (+0x23c, `rbr-hybrid` `ngp::engine`'s `ENGINE_TEMPERATURE`).
const ENGINE_RADIATOR_COOLANT: u32 = 0x20;
const ENGINE_COOLANT: u32 = 0x28;
const ENGINE_TEMPERATURE: u32 = 0x23c;
/// The drive state's gear in use is -1 for a word NGP's HUD copy shows as 7 (`0x573cfd`).
const GEAR_UNSET: u32 = u32::MAX;
const GEAR_UNSET_SHOWN: i32 = 7;
/// The wheel's strut forces the step stores (`rbr_physics::wheel::layout::step`): anti-roll bar, spring, damper and
/// the strut's sum.
const WHEEL_ROLLBAR_FORCE: u32 = 0xe70;
const WHEEL_SPRING_FORCE: u32 = 0xe74;
const WHEEL_DAMPER_FORCE: u32 = 0xe78;
const WHEEL_STRUT_FORCE: u32 = 0xe7c;

/// NGP's telemetry packet, in its fields' order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Packet {
    pub total_steps: u32,
    pub stage: Stage,
    pub control: Control,
    pub car: Car,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stage {
    /// The map number.
    pub index: i32,
    /// The car's driveline parameter (`CCar` +0x17c, an index into the driveline's points).
    pub progress: f32,
    /// The race clock, s, penalties in.
    pub race_time: f32,
    /// The driveline distance, m; 0 off the driveline.
    pub drive_line_location: f32,
    /// The finish key's distance less the driveline distance and the start key's, m; 0 off the driveline.
    pub distance_to_end: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Control {
    pub steering: f32,
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: f32,
    pub clutch: f32,
    /// The gear in use: 0 reverse, 1 neutral, 2 first.
    pub gear: i32,
    pub footbrake_pressure: f32,
    pub handbrake_pressure: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Car {
    /// The car's slot (`Cars.ini`'s index).
    pub index: i32,
    /// The driven wheels' speed, km/h.
    pub speed: f32,
    /// World position, m (z up).
    pub position: [f32; 3],
    /// Roll, pitch and yaw, degrees.
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
    /// In the car's axes: m/s and degrees/s, then g and degrees/s².
    pub velocities: Motion,
    pub accelerations: Motion,
    pub engine: Engine,
    /// Left front, right front, left rear, right rear: the car's wheels 0 to 3.
    pub suspension: [Suspension; 4],
}

/// Surge (forward), sway (right) and heave (up), then roll, pitch and yaw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub surge: f32,
    pub sway: f32,
    pub heave: f32,
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Engine {
    pub rpm: f32,
    /// K.
    pub radiator_coolant_temperature: f32,
    pub engine_coolant_temperature: f32,
    pub engine_temperature: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Suspension {
    /// The strut's travel, m.
    pub spring_deflection: f32,
    pub rollbar_force: f32,
    pub spring_force: f32,
    pub damper_force: f32,
    pub strut_force: f32,
    pub helper_spring_active: i32,
    /// The damper's fade, 0 to 1.
    pub damper_damage: f32,
    pub piston_velocity: f32,
    /// K; the wear 0 to 1.
    pub brake_layer_temperature: f32,
    pub brake_temperature: f32,
    pub brake_wear: f32,
    pub tyre: Tyre,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tyre {
    /// Pa.
    pub pressure: f32,
    /// K: the inner air's, the carcass's and the tread's.
    pub temperature: f32,
    pub carcass_temperature: f32,
    pub tread_temperature: f32,
    /// The sector in contact.
    pub current_segment: u32,
    /// Each tread sector's surface temperature, K, and wear, 0 to 1.
    pub segments: [(f32, f32); 8],
}

impl Packet {
    pub fn to_bytes(self) -> [u8; PACKET_BYTES] {
        fn bits<const N: usize>(v: [f32; N]) -> [u32; N] {
            v.map(f32::to_bits)
        }
        let (s, c, car, e) = (&self.stage, &self.control, &self.car, &self.car.engine);
        let mut words = vec![self.total_steps, s.index as u32];
        words.extend(bits([s.progress, s.race_time, s.drive_line_location, s.distance_to_end]));
        words.extend(bits([c.steering, c.throttle, c.brake, c.handbrake, c.clutch]));
        words.push(c.gear as u32);
        words.extend(bits([c.footbrake_pressure, c.handbrake_pressure]));
        words.push(car.index as u32);
        words.extend(bits([
            car.speed,
            car.position[0],
            car.position[1],
            car.position[2],
            car.roll,
            car.pitch,
            car.yaw,
        ]));
        for m in [car.velocities, car.accelerations] {
            words.extend(bits([m.surge, m.sway, m.heave, m.roll, m.pitch, m.yaw]));
        }
        words.extend(bits([e.rpm, e.radiator_coolant_temperature, e.engine_coolant_temperature, e.engine_temperature]));
        for w in &car.suspension {
            words.extend(bits([w.spring_deflection, w.rollbar_force, w.spring_force, w.damper_force, w.strut_force]));
            words.push(w.helper_spring_active as u32);
            words.extend(bits([w.damper_damage, w.piston_velocity]));
            words.extend(bits([w.brake_layer_temperature, w.brake_temperature, w.brake_wear]));
            let t = &w.tyre;
            words.extend(bits([t.pressure, t.temperature, t.carcass_temperature, t.tread_temperature]));
            words.push(t.current_segment);
            words.extend(t.segments.iter().flat_map(|&(temperature, wear)| bits([temperature, wear])));
        }
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        bytes.try_into().expect("the packet's fields make 664 bytes")
    }
}

/// What a packet takes besides the memory and the game: ours or the stage's.
pub struct Frame<'a> {
    pub map: u32,
    pub car: u32,
    /// The driveline's arc lengths (`StageData::track_values`).
    pub track: &'a [f32],
    /// The CTL1 words of the car control the frame's last step took.
    pub control: &'a [u8; CONTROL_BYTES],
}

/// The packet after a frame's step, the `total_steps`th driving step.
pub fn packet(mem: &Mem, game: &Game, frame: &Frame, total_steps: u32) -> Packet {
    let car = mem.u32(CAR_PTR);
    let word = |k: usize| f32::from_le_bytes(frame.control[4 * k..4 * k + 4].try_into().unwrap());
    let parameter = mem.f32(car + CAR_DRIVELINE_PARAMETER);
    let distance = driveline::sample_x(frame.track, parameter);
    let keys = game.markers().keys();
    let key = |tag| keys.iter().find(|k| k.tag == tag).map(|k| k.distance);
    let to_end = distance
        .zip(key(rbr_formats::markers::TAG_FINISH))
        .map(|(x, finish)| finish - x - key(rbr_formats::markers::TAG_START).unwrap_or(0.0));
    let drive_state = mem.u32(car + CAR_DRIVE + DRIVE_STATE);
    let gear = match hud_gear(mem) {
        GEAR_UNSET => GEAR_UNSET_SHOWN,
        gear => gear as i32,
    };
    let engine = mem.u32(car + CAR_ENGINE + ENGINE_STATE);
    let q = mem.f32s::<4>(car + ORIENTATION);
    let to_car = |at: u32| to_car(q, mem.f32s::<3>(car + at));
    let (roll, pitch, yaw) = angles(q);
    let (v, w) = (to_car(LINEAR_VELOCITY), to_car(ANGULAR_VELOCITY));
    let (a, b) = (to_car(ACCEL_LINEAR_EST), to_car(ACCEL_ANGULAR_EST));
    Packet {
        total_steps,
        stage: Stage {
            index: frame.map as i32,
            progress: parameter,
            race_time: game.race_clock().time,
            drive_line_location: distance.unwrap_or(0.0),
            distance_to_end: to_end.unwrap_or(0.0),
        },
        control: Control {
            throttle: word(0),
            brake: word(1),
            handbrake: word(2),
            clutch: word(3),
            steering: word(4),
            gear,
            footbrake_pressure: mem.f32(car + CAR_FOOTBRAKE_PRESSURE),
            handbrake_pressure: mem.f32(car + CAR_HANDBRAKE_PRESSURE),
        },
        car: Car {
            index: frame.car as i32,
            speed: DriveState::load(mem, drive_state).drive_speed_key * mem.eff_wheel_radius(car) * KMH,
            position: mem.f32s::<3>(car + POSITION),
            roll: degrees(roll),
            pitch: degrees(pitch),
            yaw: degrees(yaw),
            velocities: motion(v, w.map(degrees), 1.0),
            accelerations: motion(a, b.map(degrees), G),
            engine: Engine {
                rpm: mem.f32(engine + STATE_ENGINE_SPEED) * RPM,
                radiator_coolant_temperature: mem.f32(engine + ENGINE_RADIATOR_COOLANT),
                engine_coolant_temperature: mem.f32(engine + ENGINE_COOLANT),
                engine_temperature: mem.f32(engine + ENGINE_TEMPERATURE),
            },
            suspension: std::array::from_fn(|i| suspension(mem, car, i as u32)),
        },
    }
}

/// Wheel `i`'s block: its strut, the car's brake disc words for it and its tyre.
fn suspension(mem: &Mem, car: u32, i: u32) -> Suspension {
    let wheel = mem.u32(car + CAR_WHEELS + 4 * i);
    let brake = |at: u32| mem.u32(car + at + BRAKE_STRIDE * i) as f64 as f32;
    let mut out = Suspension {
        brake_layer_temperature: brake(CAR_BRAKE_LAYER_TEMPERATURE) / 1.0e6,
        brake_temperature: brake(CAR_BRAKE_TEMPERATURE) / 1.0e6,
        brake_wear: brake(CAR_BRAKE_WEAR) * BRAKE_WEAR_SCALE,
        ..Suspension::default()
    };
    if wheel == 0 {
        return out;
    }
    let controller = mem.controller(wheel + STRUT);
    out = Suspension {
        spring_deflection: mem.f32(wheel + STRUT_LENGTH),
        rollbar_force: mem.f32(wheel + WHEEL_ROLLBAR_FORCE),
        spring_force: mem.f32(wheel + WHEEL_SPRING_FORCE),
        damper_force: mem.f32(wheel + WHEEL_DAMPER_FORCE),
        strut_force: mem.f32(wheel + WHEEL_STRUT_FORCE),
        helper_spring_active: controller.mode,
        damper_damage: mem.fade_state(mem.fade_at(wheel + STRUT)).amount,
        piston_velocity: mem.f32(wheel + WHEEL_STRUT_VELOCITY),
        ..out
    };
    let scratch = wheel + TYRE_SCRATCH;
    // The tyre's state, setup, record and wear blocks (the scratch's pointers), all built by the car's load.
    if [0x10, 0x14, 0x410, 0x414].iter().all(|at| mem.u32(scratch + at) != 0) {
        let frame = mem.load_frame(scratch);
        let state = &frame.state;
        out.tyre = Tyre {
            pressure: state.air_pressure,
            temperature: mem.f32(mem.u32(scratch + TYRE_STATE) + STATE_INNER_AIR_TEMPERATURE),
            carcass_temperature: state.carcass_temperature,
            tread_temperature: state.tread_temperature,
            current_segment: state.sector,
            segments: std::array::from_fn(|k| (state.surface_temperature[k], frame.scratch.wear[k])),
        };
    }
    out
}

/// NGP's motion block from car-axis vectors (`x` left, `y` back, `z` up): forward, right and up, the linear ones
/// over `scale`.
fn motion(linear: [f32; 3], angular: [f32; 3], scale: f32) -> Motion {
    Motion {
        surge: -linear[1] / scale,
        sway: -linear[0] / scale,
        heave: linear[2] / scale,
        roll: -angular[1],
        pitch: -angular[0],
        yaw: angular[2],
    }
}

fn degrees(radians: f32) -> f32 {
    (f64::from(radians) * DEGREES) as f32
}

/// The rotation matrix of the quaternion `[x, y, z, w]` (`rbr_recording::pose::CarPose::rotate`'s): car to world.
fn matrix([x, y, z, w]: [f32; 4]) -> [[f32; 3]; 3] {
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// A world vector in the car's axes.
fn to_car(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let m = matrix(q);
    std::array::from_fn(|j| m[0][j] * v[0] + m[1][j] * v[1] + m[2][j] * v[2])
}

/// A car-axis vector in the world's.
fn to_world(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    matrix(q).map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// The angle from `u` to `a`, signed by the side of `n` their cross product is on (NGP's `FUN_1002e4e0`, x87).
fn signed_angle(u: [f32; 3], a: [f32; 3], n: [f32; 3]) -> f32 {
    let (u, a, n) = (u.map(f64::from), a.map(f64::from), n.map(f64::from));
    let cross = [u[1] * a[2] - u[2] * a[1], u[2] * a[0] - u[0] * a[2], u[0] * a[1] - u[1] * a[0]];
    let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    let angle = dot(cross, cross).sqrt().atan2(dot(u, a));
    (if dot(cross, n) < 0.0 { -angle } else { angle }) as f32
}

/// Roll, pitch and yaw, rad (NGP's `FUN_100b6b30`): roll and pitch from the world's down and up in the car's axes,
/// yaw from the car's `+y` (its back) in the world's.
fn angles(q: [f32; 4]) -> (f32, f32, f32) {
    const QUARTER: f32 = std::f32::consts::FRAC_PI_2;
    let roll = signed_angle([1.0, 0.0, 0.0], to_car(q, [0.0, 0.0, -1.0]), [0.0, 1.0, 0.0]) - QUARTER;
    let pitch = signed_angle([0.0, 1.0, 0.0], to_car(q, [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]) - QUARTER;
    let yaw = signed_angle(to_world(q, [0.0, 1.0, 0.0]), [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    (roll, pitch, yaw)
}

/// The sender: a non-blocking UDP socket to one address, and the driving steps since the stage's start.
pub struct Telemetry {
    socket: UdpSocket,
    to: SocketAddr,
    steps: u32,
    /// The last send failed: the next failure prints nothing.
    failing: bool,
}

impl Telemetry {
    pub fn open(to: SocketAddr) -> std::io::Result<Self> {
        let any: SocketAddr = if to.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { ([0u16; 8], 0).into() };
        let socket = UdpSocket::bind(any)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, to, steps: 0, failing: false })
    }

    /// The stage starts over.
    pub fn restart(&mut self) {
        self.steps = 0;
    }

    /// After a frame of `driving` driving steps: its packet.
    pub fn send(&mut self, mem: &Mem, game: &Game, driving: u32, frame: &Frame) {
        self.steps += driving;
        let bytes = packet(mem, game, frame, self.steps).to_bytes();
        match self.socket.send_to(&bytes, self.to) {
            Ok(_) => self.failing = false,
            Err(e) if !self.failing => {
                self.failing = true;
                eprintln!("rbr-standalone: --telemetry-udp {}: {e}; the drive goes on", self.to);
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_at(bytes: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_packet_has_ngp_s_offsets_and_length() {
        let wheel = |k: u32| Suspension {
            spring_deflection: 100.0 + k as f32,
            helper_spring_active: k as i32 + 1,
            piston_velocity: 110.0 + k as f32,
            brake_wear: 120.0 + k as f32,
            tyre: Tyre {
                pressure: 130.0 + k as f32,
                current_segment: k + 4,
                segments: std::array::from_fn(|s| {
                    (200.0 + 10.0 * k as f32 + s as f32, 300.0 + 10.0 * k as f32 + s as f32)
                }),
                ..Tyre::default()
            },
            ..Suspension::default()
        };
        let packet = Packet {
            total_steps: 1234,
            stage: Stage { index: 41, progress: 1.5, race_time: 2.5, drive_line_location: 3.5, distance_to_end: 4.5 },
            control: Control {
                steering: -0.25,
                throttle: 0.75,
                clutch: 0.5,
                gear: 3,
                footbrake_pressure: 5.0,
                handbrake_pressure: 6.0,
                ..Control::default()
            },
            car: Car {
                index: 5,
                speed: 88.0,
                position: [10.0, 20.0, 30.0],
                yaw: 45.0,
                velocities: Motion { surge: 7.0, yaw: 8.0, ..Motion::default() },
                accelerations: Motion { surge: 9.0, yaw: 11.0, ..Motion::default() },
                engine: Engine { rpm: 6500.0, engine_temperature: 360.0, ..Engine::default() },
                suspension: std::array::from_fn(|k| wheel(k as u32)),
                ..Car::default()
            },
        };
        let b = packet.to_bytes();
        assert_eq!(b.len(), 664);
        assert_eq!(u32_at(&b, 0), 1234);
        assert_eq!(u32_at(&b, 4), 41);
        for (at, v) in [(8, 1.5), (12, 2.5), (16, 3.5), (20, 4.5), (24, -0.25), (28, 0.75), (40, 0.5), (48, 5.0)] {
            assert_eq!(f32_at(&b, at), v, "offset {at}");
        }
        assert_eq!(u32_at(&b, 44), 3);
        assert_eq!(f32_at(&b, 52), 6.0);
        assert_eq!(u32_at(&b, 56), 5);
        for (at, v) in [(60, 88.0), (64, 10.0), (68, 20.0), (72, 30.0), (84, 45.0), (88, 7.0), (108, 8.0)] {
            assert_eq!(f32_at(&b, at), v, "offset {at}");
        }
        for (at, v) in [(112, 9.0), (132, 11.0), (136, 6500.0), (148, 360.0)] {
            assert_eq!(f32_at(&b, at), v, "offset {at}");
        }
        // The wheel blocks: left front at 152, right front 280, left rear 408, right rear 536.
        for (k, base) in [152, 280, 408, 536].into_iter().enumerate() {
            let k32 = k as u32;
            assert_eq!(f32_at(&b, base), 100.0 + k as f32);
            assert_eq!(u32_at(&b, base + 20), k32 + 1);
            assert_eq!(f32_at(&b, base + 28), 110.0 + k as f32);
            assert_eq!(f32_at(&b, base + 40), 120.0 + k as f32);
            assert_eq!(f32_at(&b, base + 44), 130.0 + k as f32);
            assert_eq!(u32_at(&b, base + 60), k32 + 4);
            assert_eq!(f32_at(&b, base + 64), 200.0 + 10.0 * k as f32);
            assert_eq!(f32_at(&b, base + 124), 307.0 + 10.0 * k as f32);
        }
        // The last field, the right rear tyre's eighth sector's wear.
        assert_eq!(f32_at(&b, 660), 337.0);
    }

    #[test]
    fn a_level_car_heading_along_x_has_no_roll_or_pitch() {
        let degrees = |(r, p, y): (f32, f32, f32)| (degrees(r), degrees(p), degrees(y));
        // The identity: the car's nose (-y) points along world -y, its back (+y) along +y, 90 degrees from +x.
        let (roll, pitch, yaw) = degrees(angles([0.0, 0.0, 0.0, 1.0]));
        assert!(roll.abs() < 1e-4 && pitch.abs() < 1e-4, "{roll} {pitch}");
        assert!((yaw.abs() - 90.0).abs() < 1e-4, "{yaw}");
        // A quarter turn about z turns the back from +y to -x: yaw 180.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let (roll, pitch, yaw) = degrees(angles([0.0, 0.0, h, h]));
        assert!(roll.abs() < 1e-4 && pitch.abs() < 1e-4 && (yaw.abs() - 180.0).abs() < 1e-3, "{roll} {pitch} {yaw}");
    }

    #[test]
    fn the_car_axes_give_surge_forward_and_sway_right() {
        // The car drives along its -y and its +x is left.
        let m = motion([-1.0, -2.0, 3.0], [0.1, 0.2, 0.3], 1.0);
        assert_eq!((m.surge, m.sway, m.heave), (2.0, 1.0, 3.0));
        assert_eq!((m.roll, m.pitch, m.yaw), (-0.2, -0.1, 0.3));
    }
}
