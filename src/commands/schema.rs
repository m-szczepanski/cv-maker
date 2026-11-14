//! `cv schema` (plan 2.3): print the derived JSON Schema to stdout (never hand-written).

use crate::error::Result;

/// The schema as a pretty JSON string — shared by `cv schema` and `cv init` (which
/// writes `cv.schema.json` next to the starter) and by the snapshot tests.
pub fn schema_json() -> String {
    let schema = schemars::schema_for!(crate::model::Document);
    // Every model type is JSON-representable by construction, so a derived schema
    // always serializes; a failure here would be a `cv` bug, not user input (rule 14).
    serde_json::to_string_pretty(&schema).expect("a derived JSON Schema always serializes")
}

pub fn run() -> Result<()> {
    crate::commands::out_line(&schema_json());
    Ok(())
}
