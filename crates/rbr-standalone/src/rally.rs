//! A championship stage's ends for the rally flow (`rbr_championship::rally`): the damage the car carries out of the
//! stage, whether it can still drive and the stage's time, read from the physics memory after the run, the race
//! flow's end (`Game::end_now`), and the files the launcher passes between stages (ours: `--damage-in`,
//! `--damage-out`, `--result-out`).

use std::path::Path;

use rbr_championship::StageReport;
use rbr_gameplay::car_damage::CarriedDamage;
use rbr_gameplay::flow::StageEnd;
use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::car_body::{damage, layout::DAMAGE_HANDLER};
use rbr_physics::mem::Mem;

/// The stage's end as the rally takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct StageOut {
    pub damage: CarriedDamage,
    pub result: StageReport,
}

/// The car's damage handler (`car + 0x9f4`).
fn handler(mem: &Mem) -> u32 {
    mem.u32(mem.u32(CAR_PTR) + DAMAGE_HANDLER)
}

/// `CCarDamageHandler::Test3B20` on the car in `mem`: the engine runs and two wheels are on.
pub fn drivable(mem: &Mem) -> bool {
    damage::drivable(mem, handler(mem))
}

/// The end of the stage: the car in `mem` and the race's `end`.
pub fn stage_out(mem: &Mem, end: StageEnd, false_start: bool) -> StageOut {
    let h = handler(mem);
    let result = StageReport { end, drivable: damage::drivable(mem, h), false_start };
    StageOut { damage: CarriedDamage::read(|at| mem.u32(h + at)), result }
}

/// Reads a carried-damage file.
pub fn read_damage(path: &Path) -> Result<CarriedDamage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    CarriedDamage::from_bytes(&bytes).ok_or(format!("{}: not a carried-damage file", path.display()))
}

/// Writes the damage file and the result ([`StageReport::to_text`]).
pub fn write(out: &StageOut, damage_path: Option<&Path>, result_path: Option<&Path>) -> Result<(), String> {
    if let Some(path) = damage_path {
        rbr_formats::saves::write_atomic(path, &out.damage.to_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(path) = result_path {
        rbr_formats::saves::write_atomic(path, out.result.to_text().as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
