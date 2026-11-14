//! `cv templates` (plan 5.5): list the built-in templates with their manifest
//! descriptions (spec §3). Custom folders are not listed: they are addressed by path
//! in the YAML, and there is no registry to enumerate (ADR-0027).

use crate::error::Result;
use crate::templates;

pub fn run() -> Result<()> {
    // A listed built-in that fails to load is a defect in the shipped assets, reported
    // as the typed internal error `load_builtin` builds rather than assumed away here.
    let loaded: Vec<templates::Template> = templates::builtin_names()
        .iter()
        .map(|name| templates::load_builtin(name))
        .collect::<Result<_>>()?;
    let width = loaded
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or_default();
    // Essential output, like `cv fonts`: kept even under `--quiet` (ADR-0012/0021).
    for template in &loaded {
        super::out_line(&format!(
            "{:<width$}  {}",
            template.name,
            template.description,
            width = width
        ));
    }
    Ok(())
}
