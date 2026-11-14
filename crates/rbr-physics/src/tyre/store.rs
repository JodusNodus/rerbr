//! The tyre's store (`crate::store`): a tyre's frame by a handle, and the setup's surface tables. The tyre's functions
//! load the frame, run on it and store it; what they read and write through their callers' pointers is the callers'
//! (the bindings'). `layout::TyreView` implements it over the typed state.

use super::layout::{Frame, Outside};
use super::surface::SurfaceInput;

pub trait TyreStore {
    /// A tyre: what the store tells its tyres apart by.
    type Tyre: Copy;

    /// The tyre's frame: its state, with its setup and the wheel's contact angle (read only).
    fn load_frame(&self, tyre: Self::Tyre) -> Frame;
    /// Stores the frame's state (the setup and the contact angle are not written).
    fn store_frame(&mut self, tyre: Self::Tyre, frame: &Frame);
    /// The surface friction's inputs for the two surface classes at the indices `classes`, with their `wetness`: the
    /// setup's base frictions, wet slopes and wear factors at those indices, its design wetness, and the wear of the
    /// sector the state names. `Outside` for a sector past the wear table.
    fn load_surface_input(
        &self,
        tyre: Self::Tyre,
        classes: [u32; 2],
        wetness: [f32; 2],
    ) -> Result<SurfaceInput, Outside>;
}
