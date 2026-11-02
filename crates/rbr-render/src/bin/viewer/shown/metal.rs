//! When each presented frame went on screen (the Metal drawable's presented handler). wgpu keeps
//! the drawable to itself, so the surface's layer becomes a subclass of its class whose `nextDrawable` keeps the one
//! it gives for [`Shown::ask`]. The handler's time is the host time [`now_ns`] reads too (`CACurrentMediaTime`).

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, ProtocolObject, Sel};
use objc2::{msg_send, sel};
use objc2_metal::MTLDrawable;
use objc2_quartz_core::{CACurrentMediaTime, CAMetalDrawable, CAMetalLayer};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use winit::window::Window;

use super::Feedback;

/// The flags: vsync when the layer syncs to the display (fifo). Metal does not say whether a frame was scanned out
/// directly, so `ZERO_COPY` is never set.
pub const VSYNC: u32 = 1;
pub const ZERO_COPY: u32 = 8;

thread_local! {
    /// The drawable the layer gave last, until its frame asks for its feedback.
    static DRAWABLE: RefCell<Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>> = const { RefCell::new(None) };
}

/// The subclass, of the first layer's class (raw-window-metal's `CAMetalLayer`).
static SUBCLASS: OnceLock<&'static AnyClass> = OnceLock::new();

/// The subclass's `nextDrawable`: its superclass's, kept. One not asked for is let go first, not to hold one of the
/// layer's few.
extern "C-unwind" fn next_drawable(layer: &AnyObject, _: Sel) -> *mut AnyObject {
    drop(DRAWABLE.take());
    let superclass = SUBCLASS.get().and_then(|class| class.superclass()).expect("made before a layer has the method");
    // Safety: the superclass's method, which returns an autoreleased drawable or nil.
    let drawable: *mut AnyObject = unsafe { msg_send![super(layer, superclass), nextDrawable] };
    DRAWABLE.set(unsafe { Retained::retain(drawable.cast()) });
    drawable
}

/// The surface's layer, made the subclass ([`next_drawable`]), and the presented handlers' feedback.
pub struct Shown {
    layer: Retained<CAMetalLayer>,
    send: Sender<(u64, Feedback)>,
    feedback: Receiver<(u64, Feedback)>,
}

impl Shown {
    /// The feedback for `surface`, if it is a Metal surface.
    pub fn new(_: &Window, surface: &wgpu::Surface) -> Result<Shown, String> {
        // Safety: the hal surface is only read, for its layer.
        let hal = unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }.ok_or("not a Metal surface")?;
        let layer = hal.render_layer().lock().clone();
        let class = layer.class();
        let subclass = *SUBCLASS.get_or_init(|| {
            let mut builder = ClassBuilder::new(c"RbrShownMetalLayer", class).expect("the class name is free");
            // Safety: `nextDrawable`'s types (a drawable or nil, autoreleased).
            unsafe { builder.add_method(sel!(nextDrawable), next_drawable as extern "C-unwind" fn(_, _) -> _) };
            builder.register()
        });
        if subclass.superclass() != Some(class) {
            return Err(format!("the surface's layer is a {:?}", class.name()));
        }
        // Safety: a subclass of the layer's class with no instance variables, whose one method returns its
        // superclass's.
        unsafe { AnyObject::set_class(&layer, subclass) };
        let (send, feedback) = mpsc::channel();
        Ok(Shown { layer, send, feedback })
    }

    /// Asks for the feedback of the drawable acquired last (the present that follows) as frame `frame`.
    pub fn ask(&mut self, frame: u64) {
        let Some(drawable) = DRAWABLE.take() else { return };
        let flags = if self.layer.displaySyncEnabled() { VSYNC } else { 0 };
        let send = self.send.clone();
        let handler = RcBlock::new(move |drawable: NonNull<ProtocolObject<dyn MTLDrawable>>| {
            // Safety: Metal passes the presented drawable.
            let at = unsafe { drawable.as_ref() }.presentedTime();
            let feedback = if at > 0.0 {
                Feedback::Shown { at: super::instant((at * 1e9) as u64, now_ns()), flags }
            } else {
                Feedback::Discarded
            };
            let _ = send.send((frame, feedback));
        });
        // Safety: Metal copies the block, which only sends on a channel.
        unsafe { drawable.addPresentedHandler(RcBlock::as_ptr(&handler)) };
    }

    /// The feedback that came since the last call.
    pub fn take(&mut self) -> Vec<(u64, Feedback)> {
        self.feedback.try_iter().collect()
    }
}

/// The host time now, ns.
fn now_ns() -> u64 {
    (CACurrentMediaTime() * 1e9) as u64
}
