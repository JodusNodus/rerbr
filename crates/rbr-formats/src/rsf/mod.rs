//! RallySimFans (RSF) installs: the community stages (`stages`) and the NGP car packs (`cars`), read in place,
//! and a drive's force-feedback and wheel settings (`settings`).

mod cars;
mod json;
pub mod settings;
mod stages;

pub use cars::*;
pub use stages::*;
