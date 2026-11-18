//! Assetto Corsa tracks as BTB stages. The readers of a track's KN5 models, AI line and data files (`assetto`), the
//! BTB writing they share with other converters (`btb`), and the interface a launcher or a command line calls to list
//! a track's layouts and convert one (`import`). Source game files are read in place and never copied into the
//! repository; `notes/assetto.md` has the verified findings.
pub mod assetto;
pub mod btb;
pub mod import;
