//! Template manifest parsing (spec §6): `manifest.yaml` inside a template folder.

use serde::Deserialize;

use crate::error::{Error, Result};

/// Design groups a template can declare support for (spec §6 manifest `supported`).
/// Tool-level fields (`template`, `fit_one_page`, `fit_floor`, `font_paths`) are not
/// groups: the tool consumes them, so a template never ignores them.
pub const DESIGN_GROUPS: [&str; 6] = ["page", "margins", "fonts", "sizes", "spacing", "colors"];

/// Template manifest schema versions this tool understands. Kept in step with the
/// CV-level schema version check in `src/validate/` (both are the data contract of
/// spec §4, and it is version 1 until spec changes it).
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

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

    /// Reject manifests that cannot describe a loadable template (plan 5.4): empty
    /// identity fields or a schema version this tool does not understand. The message
    /// is a fragment; loaders prefix it with the template folder context.
    pub fn check_usable(&self) -> std::result::Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("must declare a non-empty `name`".to_owned());
        }
        if self.description.trim().is_empty() {
            return Err("must declare a non-empty `description`".to_owned());
        }
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(format!(
                "targets schema version {}, but this tool uses version {SUPPORTED_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, description: &str, schema_version: u32) -> Manifest {
        Manifest {
            name: name.to_owned(),
            description: description.to_owned(),
            schema_version,
            supported: vec![],
        }
    }

    #[test]
    fn usable_manifests_pass_the_check() {
        assert!(manifest("t", "d", 1).check_usable().is_ok());
    }

    #[test]
    fn blank_identity_fields_are_rejected() {
        assert_eq!(
            manifest("  ", "d", 1)
                .check_usable()
                .expect_err("blank name"),
            "must declare a non-empty `name`"
        );
        assert_eq!(
            manifest("t", "", 1)
                .check_usable()
                .expect_err("blank description"),
            "must declare a non-empty `description`"
        );
    }

    #[test]
    fn foreign_schema_versions_are_rejected_with_both_numbers_named() {
        let problem = manifest("t", "d", 2)
            .check_usable()
            .expect_err("version 2 is not understood");
        assert!(
            problem.contains("version 2") && problem.contains("version 1"),
            "got: {problem}"
        );
    }

    #[test]
    fn manifest_yaml_is_parsed_strictly() {
        let error =
            Manifest::from_yaml_str("name: t\ndescription: d\nschema_version: 1\nnope: 1\n")
                .expect_err("unknown keys are rejected");
        assert!(error.to_string().contains("nope"), "got: {error}");
    }
}
