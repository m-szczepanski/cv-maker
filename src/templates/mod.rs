//! Template loading (spec §6): built-in templates from embedded assets.
//!
//! Custom template folders arrive in M5; the [`Template`] shape is designed so a custom
//! loader can fill it from disk later without changing call sites.

pub mod manifest;

use include_dir::{Dir, DirEntry, include_dir};

use crate::error::{Error, Result};

/// All built-in template folders, embedded at compile time (spec §2, §10).
pub static BUILTIN_TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// A loaded template: its manifest plus the Typst sources that make it up.
#[derive(Debug, Clone)]
pub struct Template {
    /// Template name as declared in the manifest.
    pub name: String,
    /// One-line description from the manifest (used by `cv templates`).
    pub description: String,
    /// `design.*` fields this template honors (plan 3.5); drives the `cv check`
    /// warning for ignored fields (spec §6).
    pub supported_options: Vec<String>,
    /// Content of the entry-point `template.typ`.
    pub main_source: String,
    /// Additional Typst sources, as (virtual path, content) pairs (e.g. `lib/inline.typ`).
    pub extra_sources: Vec<(String, String)>,
}

/// Names of the built-in templates, sorted for stable output.
pub fn builtin_names() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_TEMPLATES
        .dirs()
        .filter_map(|d| d.path().file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Collect every file embedded under `templates/<name>/`, recursively.
/// Returns (path, contents) pairs with paths relative to `templates/`, sorted for
/// determinism.
fn embedded_template_files(name: &str) -> Vec<(&'static str, &'static [u8])> {
    fn walk<'a>(dir: &Dir<'a>, out: &mut Vec<(&'a str, &'a [u8])>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::File(f) => {
                    if let Some(p) = f.path().to_str() {
                        out.push((p, f.contents()));
                    }
                }
                DirEntry::Dir(d) => walk(d, out),
            }
        }
    }
    let prefix = format!("{name}/");
    let mut out: Vec<(&'static str, &'static [u8])> = Vec::new();
    walk(&BUILTIN_TEMPLATES, &mut out);
    out.retain(|(p, _)| p.starts_with(&prefix));
    out.sort_unstable_by_key(|(p, _)| *p);
    out
}

/// Load a built-in template by name from the embedded assets.
pub fn load_builtin(name: &str) -> Result<Template> {
    // Embedded file paths are relative to the `templates/` folder that include_dir embeds
    // (e.g. `compact/template.typ`), so we match by path prefix rather than using
    // `Dir::get_file` on the subdirectory (its lookup compares full paths).
    let prefix = format!("{name}/");
    let files = embedded_template_files(name);

    if files.is_empty() {
        return Err(unknown_template_error(name));
    }

    let find = |path: &str| -> Option<&'static [u8]> {
        files
            .iter()
            .find(|(p, _)| *p == format!("{prefix}{path}"))
            .map(|(_, c)| *c)
    };

    let utf8 = |bytes: &'static [u8], what: &str| -> Result<String> {
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            // Unreachable for shipped assets: embedded files are compiled in and are
            // valid UTF-8 by construction (checked by `cargo test` at least once).
            Error::Internal(format!(
                "built-in template '{name}' {what} is not valid UTF-8"
            ))
        })
    };

    let main_bytes = find("template.typ").ok_or_else(|| {
        // Unreachable: every template folder ships a template.typ (enforced by tests).
        Error::Internal(format!(
            "built-in template '{name}' has no template.typ (this is a bug in cv)"
        ))
    })?;
    let main_source = utf8(main_bytes, "template.typ")?;

    let mut extra_sources = Vec::new();
    for (path, contents) in &files {
        let virtual_path = path.strip_prefix(&prefix).expect("path starts with prefix");
        if virtual_path == "template.typ" {
            continue;
        }
        if std::path::Path::new(virtual_path).extension() != Some(std::ffi::OsStr::new("typ")) {
            continue;
        }
        extra_sources.push((virtual_path.to_owned(), utf8(contents, virtual_path)?));
    }

    let manifest_bytes = find("manifest.yaml").ok_or_else(|| {
        // Unreachable: every template folder ships a manifest.yaml (enforced by tests).
        Error::Internal(format!(
            "built-in template '{name}' has no manifest.yaml (this is a bug in cv)"
        ))
    })?;
    let manifest = manifest::Manifest::from_yaml_str(&utf8(manifest_bytes, "manifest.yaml")?)?;

    Ok(Template {
        name: manifest.name,
        description: manifest.description,
        supported_options: manifest.supported,
        main_source,
        extra_sources,
    })
}

fn unknown_template_error(name: &str) -> Error {
    let known = builtin_names();
    let hint = match crate::suggest::closest_match(name, &known) {
        Some(best) => format!("did you mean '{best}'?"),
        None => format!("available templates: {}", known.join(", ")),
    };
    Error::Unsupported(format!(
        "unknown template '{name}' ({hint}). Only built-in templates can be used so far; custom template folders arrive in M5"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names_contains_compact() {
        assert!(builtin_names().contains(&"compact".to_owned()));
    }

    #[test]
    fn load_builtin_compact_has_sources_and_manifest() {
        let template = load_builtin("compact").expect("compact must load");
        assert_eq!(template.name, "compact");
        assert!(
            !template.description.is_empty(),
            "manifest needs a description"
        );
        assert!(
            template.main_source.contains("data.yaml"),
            "template reads the data file"
        );
        assert!(
            template.main_source.contains("document("),
            "template sets document metadata"
        );
    }

    #[test]
    fn compact_template_ships_the_inline_helper() {
        let template = load_builtin("compact").expect("compact must load");
        assert!(
            template
                .extra_sources
                .iter()
                .any(|(path, _)| path == "lib/inline.typ"),
            "inline Markdown helper must load as an extra source"
        );
        assert!(
            template.main_source.contains("inline("),
            "template must render Markdown fields through the helper"
        );
    }

    #[test]
    fn unknown_template_gets_did_you_mean_hint() {
        let err = load_builtin("comapct").expect_err("typo'd name must fail");
        let msg = err.to_string();
        assert!(msg.contains("did you mean 'compact'"), "got: {msg}");
    }

    #[test]
    fn unknown_template_lists_available_names() {
        let err = load_builtin("nonexistent-template").expect_err("unknown name must fail");
        let msg = err.to_string();
        assert!(msg.contains("compact"), "got: {msg}");
    }
}
