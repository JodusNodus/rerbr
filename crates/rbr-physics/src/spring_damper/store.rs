//! The spring-damper's stores (`crate::store`): the strut's load and store groups ([`StrutStore`]) and the damper
//! kernels' blocks, which live in the engine's and the tyre's objects ([`DamperBlocks`]). `layout.rs` implements both:
//! the strut store on `StrutView`, the blocks on every word store.

use super::suspension::Damping;
use super::{
    BlendTerms, ClampParameters, Controller, DamperSignal, Definition, FadeState, ForceTerms, HeatAccumulator,
    HeatCapacity, Relaxation, UpdateTerms, WeightedValue,
};
use crate::store::Raw;

/// The struts: each one's definition, controller, effective damping and fade, by a handle. The step's out pointers
/// are its caller's (the binding's).
pub trait StrutStore {
    /// A strut: what the store tells its struts apart by (on `Mem`, its controller's address).
    type Strut: Copy;

    /// The strut's definition (read only: its setup copy is the image's).
    fn definition(&self, strut: Self::Strut) -> Definition;

    /// The three effective damping values.
    fn damping(&self, strut: Self::Strut) -> Damping;
    fn store_damping(&mut self, strut: Self::Strut, value: Damping);
    /// The damper signal's owner id (the owner wheel's index).
    fn owner_id(&self, strut: Self::Strut) -> u32;

    fn controller(&self, strut: Self::Strut) -> Controller;
    /// Stores the words the step changes: the mode, the hold and the three velocities.
    fn store_step_state(&mut self, strut: Self::Strut, value: Controller);
    fn store_signal(&mut self, strut: Self::Strut, value: DamperSignal);

    fn fade(&self, strut: Self::Strut) -> FadeState;
    fn store_fade(&mut self, strut: Self::Strut, value: FadeState);
}

/// The damper kernels' blocks (in the engine's and the tyre's objects). Every word store has them, at their addresses
/// (`layout.rs`): a typed store's view routes the words it holds.
pub trait DamperBlocks: Raw {
    fn load_update_terms(&self, at: u32) -> UpdateTerms;
    fn store_update_terms(&mut self, at: u32, terms: UpdateTerms);
    fn load_blend_terms(&self, at: u32) -> BlendTerms;
    fn store_blend_terms(&mut self, at: u32, value: BlendTerms);
    fn load_relaxation(&self, at: u32) -> Relaxation;
    fn store_relaxation(&mut self, at: u32, value: Relaxation);
    fn load_force_terms(&self, at: u32) -> ForceTerms;
    fn store_force_terms(&mut self, at: u32, value: ForceTerms);
    fn load_heat_capacity(&self, at: u32) -> HeatCapacity;
    fn load_heat_accumulator(&self, at: u32) -> HeatAccumulator;
    fn store_heat_accumulator(&mut self, at: u32, value: HeatAccumulator);
    fn load_clamp_parameters(&self, at: u32) -> ClampParameters;
    fn load_weighted(&self, at: u32) -> WeightedValue;
    fn store_weighted(&mut self, at: u32, value: WeightedValue);
}
