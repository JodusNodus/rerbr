//! Progress messages and cancellation for a long conversion, so a caller other than the CLI can drive one.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The error text of a cancelled conversion.
pub const CANCELLED: &str = "cancelled";

type Sink = Arc<dyn Fn(&str) + Send + Sync>;

/// Where a conversion reports to and how it is stopped. The default prints to stderr and never stops.
#[derive(Clone, Default)]
pub struct Control {
    log: Option<Sink>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Control {
    pub fn new(log: impl Fn(&str) + Send + Sync + 'static, cancel: Arc<AtomicBool>) -> Self {
        Self { log: Some(Arc::new(log)), cancel: Some(cancel) }
    }

    pub fn say(&self, message: &str) {
        match &self.log {
            Some(log) => log(message),
            None => eprintln!("rbr-ac-tracks: {message}"),
        }
    }

    /// `Err(CANCELLED)` once the caller has asked to stop.
    pub fn check(&self) -> Result<(), String> {
        match &self.cancel {
            Some(cancel) if cancel.load(Ordering::Relaxed) => Err(CANCELLED.into()),
            _ => Ok(()),
        }
    }
}
