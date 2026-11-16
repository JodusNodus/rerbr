//! The car's sensor and signal composite, run each step. The order is the original's: the drive, passenger, engine and
//! light updates, the mounted-body loop, the car's vectors copied into the frame (the ground probe, the world placement
//! of the body frame, the wheel loop and the two state emits), the sensor sampler and its emit, the drive line
//! position's emit, the clamped speed's emit, the walk and the direction into the frame, the keeper branch, the damage
//! object's update and the engine's last update. `frame` is the base of the step's locals
//! (`layout::publish::PublishFrame`), whose addresses the calls take. The externals are answered by `PublishCalls`.

use crate::mem::Mem;

/// The calls the publish step makes out of its own body. The test answers them from the original's records.
pub trait PublishCalls<S = Mem> {
    /// A call on `object` with its arguments, returning the result word. The virtual targets go through here too.
    fn call(&mut self, mem: &mut S, callee: u32, object: u32, args: &[u32]) -> u32;
    /// A signal emit on the emitter `signal` with the payload pointer `payload`.
    fn emit(&mut self, mem: &mut S, callee: u32, signal: u32, payload: u32);
}
