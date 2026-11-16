//! The stage's readers (the drive line's and the material table's bindings): they run on the typed stage the host
//! holds over a step ([`crate::Host::typed`]), else on the image (the per-call path: the image is the stage's, which
//! a step does not write).

use rbr_physics::mem::Mem;
use rbr_physics::stage::layout::StageView;

use crate::Host;

/// Runs `run` on the stage's store: the typed stage while the host holds it, the image for the rest.
pub(crate) fn on_stage<T>(host: &dyn Host, mem: &Mem, run: impl FnOnce(&StageView) -> T) -> T {
    let typed = host.typed();
    run(&StageView::new(Some(&*typed.stage), mem))
}
