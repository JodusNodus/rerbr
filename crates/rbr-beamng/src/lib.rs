//! BeamNG levels converted to BTB stage folders on the player's machine, from the player's own copy of a level mod.
//! Nothing of a level is ever part of ReRBR: the readers here take a level zip and the conversion writes a stage
//! folder the existing BTB path plays (`rbr_btb`). Findings: `notes/beamng.md`.

pub mod cdae;
pub mod convert;
pub mod course;
pub mod cutout;
pub mod json;
pub mod level;
pub mod materials;
pub mod msgpack;
pub mod pacenotes;
pub mod race;
pub mod road;
pub mod textures;
pub mod world;
pub mod xwrite;
pub mod zipfs;
