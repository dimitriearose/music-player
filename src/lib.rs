//! `am` as a library, so other programs can embed it. homebase does: it shows
//! am's screen as its music module.
//!
//! The `am` program itself (`main.rs`) is a thin wrapper around this: it
//! starts the worker, then loops "apply updates → draw → handle a key".
//!
//! Rust note: a package can have both a library (`src/lib.rs`) and a program
//! (`src/main.rs`). The program uses the library like any other crate, by
//! its name: `use am::app::App;`.

pub mod app;
pub mod art;
pub mod check;
pub mod config;
pub mod library;
pub mod music;
pub mod popup;
pub mod shuffle;
pub mod ui;
pub mod worker;
