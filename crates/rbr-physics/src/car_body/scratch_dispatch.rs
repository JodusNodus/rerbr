//! The dispatch to the car's scratch sim at the end of the car's step reset.

use crate::rigid_body::host::EntityScratchTailHost;

/// The call into the scratch sim's second method, after the scratch block has been reset.
pub trait ScratchDispatchCalls: EntityScratchTailHost {
    fn scratch_slot1(&mut self, scratch: u32) -> u32;
}

/// Tail-dispatches to the car's scratch sim.
pub fn dispatch_scratch_slot1(calls: &mut impl ScratchDispatchCalls, scratch: u32) -> u32 {
    calls.scratch_slot1(scratch)
}
