//! `cv schema` (plan 2.3): print the derived JSON Schema to stdout (never hand-written).

use crate::error::Result;

pub fn run() -> Result<()> {
    let schema = schemars::schema_for!(crate::model::Document);
    // Serializing a freshly derived schemars document cannot fail: every model type is
    // JSON-representable by construction, so the Internal arm is unreachable.
    let json = serde_json::to_string_pretty(&schema)
        .map_err(|e| crate::error::Error::Internal(format!("schema serialization failed: {e}")))?;
    println!("{json}");
    Ok(())
}

/// The schema as a pretty JSON string — shared with `cv init` (writes `cv.schema.json`)
/// and the snapshot tests.
pub fn schema_json() -> Result<String> {
    let schema = schemars::schema_for!(crate::model::Document);
    // Same invariant as in `run`: derived schemas always serialize.
    serde_json::to_string_pretty(&schema)
        .map_err(|e| crate::error::Error::Internal(format!("schema serialization failed: {e}")))
}
