//! `cv templates` (plan 5.5): list the built-in templates with their manifest
//! descriptions (spec §3). Custom folders are not listed: they are addressed by path
//! in the YAML, and there is no registry to enumerate (ADR-0027).

use crate::error::Result;
use crate::templates;

pub fn run() -> Result<()> {
    // Every name in `builtin_names()` comes from the same embedded tree
    // `load_builtin` reads, so a listed name always loads: the `?` arm is
    // unreachable by construction (rule 21 invariant).
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
        println!(
            "{:<width$}  {}",
            template.name,
            template.description,
            width = width
        );
    }
    Ok(())
}
