//! Command handlers: `init`, `build` (M1); `check`, `schema` (M2); `fonts` (M3).

pub mod build;
pub mod check;
pub mod fonts;
pub mod init;
pub mod schema;

use std::path::{Path, PathBuf};

use miette::Severity;

use crate::error::{Diag, Error, Result};
use crate::model::Document;
use crate::validate::ValidationResult;

/// A YAML file fully loaded: source text, typed model, and its validation results.
pub struct LoadedCv {
    pub yaml: String,
    pub document: Document,
    pub validation: ValidationResult,
    /// `--font-path` directories plus `design.font_paths` resolved against the input file,
    /// in ADR-0007 priority order (user intent first).
    pub font_dirs: Vec<PathBuf>,
}

/// Read, parse and semantically validate a CV YAML file (plan 2.1/2.2).
///
/// Parse errors and semantic errors both surface as span-labeled diagnostics
/// (Error::Input); warnings ride along in [`LoadedCv::validation`] and never fail.
pub fn load_cv(file: &Path, font_paths: &[PathBuf]) -> Result<LoadedCv> {
    let yaml = std::fs::read_to_string(file).map_err(|source| Error::Io {
        path: file.display().to_string(),
        source,
    })?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());

    let document = match Document::from_yaml_str(&yaml) {
        Ok(document) => document,
        Err(Error::Yaml { source, .. }) => {
            return Err(Error::Input {
                diagnostics: vec![Diag::from_yaml_error(source, &yaml, &name)],
            });
        }
        // `from_yaml_str` maps every parse failure to `Error::Yaml`, so `other` is
        // unreachable for a successfully-read file.
        Err(other) => return Err(other),
    };

    let mut font_dirs: Vec<PathBuf> = font_paths.to_vec();
    for rel in &document.design.font_paths {
        font_dirs.push(resolve_relative(file, rel));
    }

    let validation = crate::validate::validate(&yaml, &name, &document, &font_dirs);
    if !validation.is_ok() {
        return Err(Error::Input {
            diagnostics: validation.errors,
        });
    }
    Ok(LoadedCv {
        yaml,
        document,
        validation,
        font_dirs,
    })
}

/// Print validation warnings (if any) to stderr (ADR-0015: warnings never fail).
pub fn report_warnings(warnings: &[Diag]) {
    for warning in warnings {
        eprint!("{}", crate::error::render_diag(warning, false));
    }
}

/// The fit-overflow warning at the floor (spec §7 step 4, ADR-0026): plain text —
/// the cause is content volume, not one YAML value, so there is no span to label.
pub fn report_overflow(output: &crate::render::RenderOutput) {
    let message = format!(
        "content still overflows at the fit floor: about {} lines spill onto {} extra page(s); cut content (e.g. bullets or paragraphs) or relax design.fit_floor / margins",
        output.overflow_lines,
        output.page_count.saturating_sub(1),
    );
    eprint!(
        "{}",
        crate::error::render_diag(&Diag::plain(message, Severity::Warning), false)
    );
}

/// `design.font_paths` entries are relative to the YAML file's directory (spec §4 example).
fn resolve_relative(input: &Path, rel: &str) -> PathBuf {
    let base = input.parent().unwrap_or(Path::new("."));
    base.join(rel)
}
