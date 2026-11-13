//! `cv` library crate.
//!
//! The binary (`main.rs`) is a thin wrapper; tests and future milestones (fit loop,
//! watch mode) use the library API directly. Exposing modules here also lets integration
//! tests exercise parsing/rendering without shelling out to the binary.

pub mod cli;
pub mod commands;
pub mod error;
pub mod model;
pub mod render;
pub mod suggest;
pub mod templates;
pub mod validate;
