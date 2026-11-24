//! DiRT Rally stages as BTB stages for ReRBR. Readers of the player's own DiRT Rally install (the Feral Linux port's
//! loose files: gzip'd PSSG scenes, Codemasters binary XML, JPAK collision), and the import of one route into a BTB stage
//! with the BTB writing `rbr_ac_tracks::btb` shares. Game files are read in place and never copied into the repository;
//! `notes/dirt-rally.md` has the verified findings.
//!
//! - `install`: the data folder and its routes (`tracks/locations/<location>/<area>/route_<n>`);
//! - `pssg`, `mesh`, `material`, `texture`: the drawn world;
//! - `objects`, `trees`: the props and trees placed in it (`objects.ens`, `trees.bin`);
//! - `binxml`, `route`: the course, timing, calls and names;
//! - `jpak`, `collision`, `surface`: the driven surface and its RBR ids;
//! - `frame`: DiRT Rally's axes to the stage's;
//! - `stage`, `import`, `cli`: the conversion and its interfaces.
pub mod added_car;
pub mod binxml;
pub mod car;
pub mod car_maps;
pub mod car_pack;
pub mod cli;
pub mod collision;
pub mod frame;
pub mod gauges;
pub mod import;
pub mod install;
pub mod jpak;
pub mod material;
pub mod mesh;
pub mod objects;
pub mod pssg;
pub mod route;
pub mod shade;
pub mod stage;
pub mod surface;
pub mod texture;
pub mod trees;
pub mod vista;
