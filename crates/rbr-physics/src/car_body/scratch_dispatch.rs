//! The dispatch to the car's scratch sim at the end of the car's step reset.

use crate::rigid_body::host::EntityScratchTailHost;

/// The call into the scratch's method that runs after the scratch block has been reset (named by use: its second
/// method).
pub trait ScratchDispatchCalls: EntityScratchTailHost {
    fn scratch_after_reset(&mut self, scratch: u32) -> u32;
}

/// Tail-dispatches to the car's scratch.
pub fn dispatch_scratch_after_reset(calls: &mut impl ScratchDispatchCalls, scratch: u32) -> u32 {
    calls.scratch_after_reset(scratch)
}
