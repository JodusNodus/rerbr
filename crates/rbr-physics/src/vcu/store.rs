//! The VCU's stores (`crate::store`): the unit's typed state, its definition and the objects its calls go to
//! ([`VcuStore`]), and the drive's blocks it reads and writes ([`DriveBlocks`]). The drive's blocks are the drive's
//! objects: its own step and setters read and write them in the same step, so they stay on the image until the drive's
//! state is typed. `layout.rs` implements both on `VcuView`, and the drive's blocks on `Mem`.

use super::layout::{Def, GearRatios, Gearbox, Outputs, Scratch, SimParts, State};

/// One unit: its scratch's and state's changing words, its definition (read only: the image's) and the objects its
/// calls go to (the image's pointers).
pub trait VcuStore {
    fn def(&self) -> Def;
    fn scratch(&self) -> Scratch;
    fn store_scratch(&mut self, value: &Scratch);
    fn state(&self) -> State;
    fn store_state(&mut self, value: &State);
    /// The car's engine, drive and lights.
    fn parts(&self) -> SimParts;
    /// The state's car, which the car speed is read on.
    fn car(&self) -> u32;
}

/// The drive's blocks, by the drive's address: its gear fields, its gear count and ratios, and the outputs the step's
/// drive setters write.
pub trait DriveBlocks {
    fn gearbox(&self, drive: u32) -> Gearbox;
    fn store_gearbox(&mut self, drive: u32, value: &Gearbox);
    /// Gear count of the drive's own definition, which the gear setter clamps to.
    fn gear_count(&self, drive: u32) -> u32;
    fn gear_ratios(&self, drive: u32) -> GearRatios;
    fn outputs(&self, drive: u32) -> Outputs;
    /// Writes the outputs back. Of the locks only the torques change; the ratios and the maximum torques are inputs.
    fn store_outputs(&mut self, drive: u32, value: &Outputs);
}
