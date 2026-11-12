//! Data model (spec §4): content (`cv`) and design blocks as serde structs.
//!
//! Every struct uses `deny_unknown_fields` so user mistakes surface as errors instead of
//! being silently ignored. All JSON Schema derives land here too (plan 1.1.4), even though
//! `cv schema` is only exposed in M2.

pub mod content;
pub mod design;
pub mod value;

pub use content::{Contact, Cv, EntryItem, KeyValueItem, Link, Section, SectionBody};
pub use design::{Colors, Design, FitFloor, Fonts, Margins, Page, Sizes, Spacing};
pub use value::{Color, Length, LengthUnit, LineHeight};

#[cfg(test)]
mod tests;

use crate::error::{Error, Result};
use serde::Deserialize;

/// The top-level YAML document (spec §4): `schema`, `cv`, `design`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Schema version. Only `1` is understood (semantic check enforces this).
    pub schema: u32,

    /// The CV content.
    pub cv: Cv,

    /// Presentation settings. Optional; missing values fall back to Rust-defined defaults.
    #[serde(default)]
    pub design: Design,
}

impl Document {
    /// Parse a YAML string into a [`Document`].
    pub fn from_yaml_str(yaml: &str) -> Result<Self> {
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(yaml).map_err(|e| Error::yaml(e, yaml))?;
        serde_yaml_ng::from_value(value).map_err(|e| Error::yaml(e, yaml))
    }
}
