//! `cv` library crate.
//!
//! The binary (`main.rs`) is a thin wrapper over this API: integration tests and the
//! commands themselves use the same entry points, so nothing has to shell out to the
//! binary to render or validate a CV.

pub mod cli;
pub mod commands;
pub mod error;
pub mod fit;
pub mod markdown;
pub mod model;
pub mod render;
pub mod suggest;
pub mod templates;
pub mod validate;
