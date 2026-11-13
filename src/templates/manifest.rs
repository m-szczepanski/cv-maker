//! Template manifest parsing (spec §6): `manifest.yaml` inside a template folder.

use serde::Deserialize;

use crate::error::{Error, Result};

/// A template's `manifest.yaml`.
///
/// `supported` lists the `design.*` fields the template honors. In M3, `cv check` warns when
/// the YAML uses design fields the template declared unsupported (spec §6). Tool-level
/// fields (`template`, `fit_one_page`, `fit_floor`, `font_paths`) are not listed because
/// templates never render them directly.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    /// Schema version this template targets.
    pub schema_version: u32,
    /// Design fields the template honors.
    #[serde(default)]
    pub supported: Vec<String>,
}

impl Manifest {
    pub fn from_yaml_str(yaml: &str) -> Result<Self> {
        serde_yaml_ng::from_str(yaml).map_err(Error::from)
    }
}
