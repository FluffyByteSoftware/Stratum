//! File:     stratum-tools/src/lib.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! The tools every part of Stratum shares.  This file only names them.  From
//! another crate they are `stratum_tools::scribe` and so on -- Cargo turns
//! the dash in the crate's name into an underscore.

// Rust note: `pub mod` is the old `mod` line from main.rs with `pub` in
// front, so other crates can see the file too.  Inside the files nothing
// changes.  `crate::scribe` still works, because this file is the crate's
// root now, the way main.rs used to be.
pub mod scribe;
pub mod constellations;
pub mod diskman;
pub mod security;
pub mod account;
pub mod fingerprinter;