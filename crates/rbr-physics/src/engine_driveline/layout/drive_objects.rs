//! `CDrive`'s lanes, definition and state as the accessors see them.

use crate::mem::Mem;
use crate::store::{Raw, Value};

/// An NTweak value with a dirty flag: the flag at +8 (set by every write), the value at +0xc.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lane {
    pub dirty: u32,
    pub value: f32,
}

impl Lane {
    pub fn load<R: Raw + ?Sized>(mem: &R, at: u32) -> Self {
        Lane { dirty: mem.u32(at + 0x08), value: mem.f32(at + 0x0c) }
    }

    pub fn store<R: Raw + ?Sized>(&self, mem: &mut R, at: u32) {
        mem.set_u32(at + 0x08, self.dirty);
        mem.set_f32(at + 0x0c, self.value);
    }

    /// A write: the dirty flag is set and the value replaced.
    pub fn set(&mut self, value: f32) {
        self.dirty = 1;
        self.value = value;
    }

    /// A write of the value's bits (the original passes an int that holds a float's bits).
    pub fn set_bits(&mut self, bits: u32) {
        self.set(f32::from_bits(bits));
    }
}

/// As a table field (`object!`) a lane is its two words from the dirty word (`Lane::load` takes the tweak's base).
impl Value for Lane {
    const LEN: u32 = 8;
    fn load(mem: &Mem, at: u32) -> Self {
        Lane { dirty: mem.u32(at), value: mem.f32(at + 4) }
    }
    fn store(self, mem: &mut Mem, at: u32) {
        mem.set_u32(at, self.dirty);
        mem.set_f32(at + 4, self.value);
    }
    fn word(&self, k: u32) -> Option<u32> {
        [self.dirty, self.value.to_bits()].get(k as usize).copied()
    }
    fn set_word(&mut self, k: u32, value: u32) -> bool {
        match k {
            0 => self.dirty = value,
            1 => self.value = f32::from_bits(value),
            _ => return false,
        }
        true
    }
}

/// The same header as `Lane`, with an integer value (the commanded gear).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntLane {
    pub dirty: u32,
    pub value: i32,
}

impl IntLane {
    pub fn load<R: Raw + ?Sized>(mem: &R, at: u32) -> Self {
        IntLane { dirty: mem.u32(at + 0x08), value: mem.u32(at + 0x0c) as i32 }
    }

    pub fn store<R: Raw + ?Sized>(&self, mem: &mut R, at: u32) {
        mem.set_u32(at + 0x08, self.dirty);
        mem.set_u32(at + 0x0c, self.value as u32);
    }

    /// A write: the dirty flag is set and the value replaced.
    pub fn set(&mut self, value: i32) {
        self.dirty = 1;
        self.value = value;
    }
}

/// As a table field, from the dirty word (see [`Lane`]'s).
impl Value for IntLane {
    const LEN: u32 = 8;
    fn load(mem: &Mem, at: u32) -> Self {
        IntLane { dirty: mem.u32(at), value: mem.u32(at + 4) as i32 }
    }
    fn store(self, mem: &mut Mem, at: u32) {
        mem.set_u32(at, self.dirty);
        mem.set_u32(at + 4, self.value as u32);
    }
    fn word(&self, k: u32) -> Option<u32> {
        [self.dirty, self.value as u32].get(k as usize).copied()
    }
    fn set_word(&mut self, k: u32, value: u32) -> bool {
        match k {
            0 => self.dirty = value,
            1 => self.value = value as i32,
            _ => return false,
        }
        true
    }
}

/// The drive's input lanes (`CDrive`): the throttle (`Set44A0`), the brake (`Set30`), the brake torque (`Set30`'s scaled
/// copy of the brake) and the clutch slip (`Set4820`). Offsets in [`super::DriveObj`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drive {
    pub throttle_in: Lane,
    pub brake_in: Lane,
    pub brake_torque: Lane,
    pub clutch_slip: Lane,
}

/// `CDrive`'s object pointers (rbr-recomp `notes/physics/engine-driveline.md` 2) and its definition's members
/// (rbr-recomp `notes/physics/renames/engine-driveline.csv`).
pub(super) const DRIVE_DEF_PTR: u32 = 0x04;
pub(super) const DRIVE_STATE_PTR: u32 = 0x08;
pub(super) const DRIVE_DAMAGE_PTR: u32 = 0x170;
pub(super) const DRIVE_DEF_MAX_FOOTBRAKE_PRESSURE: u32 = 0x620;
pub(super) const DRIVE_DEF_GEAR_COUNT: u32 = 0x654;

/// The drive's definition (`CDrive::CDef`, `this+4`): the fields the accessors read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveDef {
    /// Scale of the brake torque (`Set30`), `+0x620` (`MaxFootbrakePressure`).
    pub max_footbrake_pressure: f32,
    /// Number of gears (`NumberOfGears`), `+0x654`.
    pub gear_count: u32,
}

impl DriveDef {
    /// The definition at `at` (the image's: a step does not write it).
    pub fn load(mem: &Mem, at: u32) -> Self {
        DriveDef {
            max_footbrake_pressure: mem.f32(at + DRIVE_DEF_MAX_FOOTBRAKE_PRESSURE),
            gear_count: mem.u32(at + DRIVE_DEF_GEAR_COUNT),
        }
    }
}

/// The drive's state (`CDrive`'s `this+8`, the car's drive state): the fields the accessors read or write.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveState {
    /// Drive speed key, `+0x384` (its meaning is not known).
    pub drive_speed_key: f32,
    /// Engine speed, rad/s (`+0x45c`).
    pub engine_speed: f32,
    /// The gear (`+0x54c`, see the module README).
    pub gear: i32,
    /// The commanded gear (`+0x564` header, value at `+0x570`), written by `Set47B0`.
    pub commanded_gear: IntLane,
}

impl DriveState {
    /// The state at `at` as the image holds it: for the readers between steps (telemetry, the picture).
    pub fn load(mem: &Mem, at: u32) -> Self {
        use super::drive_typed::DriveStateObj as S;
        DriveState {
            drive_speed_key: S::drive_speed_key.load(mem, at),
            engine_speed: S::engine_speed.load(mem, at),
            gear: S::gear.load(mem, at) as i32,
            commanded_gear: S::commanded.load(mem, at),
        }
    }
}

/// Where the drive's objects are: the definition (`this+4`), the state (`this+8`) and the damage block (`this+0x170`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveObjects {
    pub def: u32,
    pub state: u32,
    pub damage: u32,
}

impl DriveObjects {
    /// The pointers of the drive at `drive` (the image's).
    pub fn at(mem: &Mem, drive: u32) -> Self {
        DriveObjects {
            def: mem.u32(drive + DRIVE_DEF_PTR),
            state: mem.u32(drive + DRIVE_STATE_PTR),
            damage: mem.u32(drive + DRIVE_DAMAGE_PTR),
        }
    }
}
