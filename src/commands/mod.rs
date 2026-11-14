//! Command handlers, one per CLI subcommand (spec §3): `init`, `build`, `watch`,
//! `check`, `templates`, `schema`, `fonts`. They share [`load_cv`] (read, parse,
//! validate) and the output rules, so the commands cannot drift apart.

pub mod build;
pub mod check;
pub mod fonts;
pub mod init;
pub mod schema;
pub mod templates;
pub mod watch;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use miette::Severity;

use crate::error::{Diag, Error, Result};
use crate::model::Document;
use crate::templates::Template;
use crate::validate::Outcome;

/// A YAML file fully loaded: source text, typed model, the template it resolved to,
/// the warnings the checks found, and the user font files.
pub struct LoadedCv {
    pub yaml: String,
    pub document: Document,
    /// The validated template (`design.template` resolved): the render never loads a
    /// second time what `cv check` already judged (ADR-0027).
    pub template: Template,
    /// Non-fatal diagnostics (ADR-0015).
    pub warnings: Vec<Diag>,
    /// The user font files — `--font-path` directories plus `design.font_paths`
    /// resolved against the input file, in ADR-0007 priority order (user intent
    /// first). Read once here and reused by the validation and the render.
    pub font_items: Vec<Vec<u8>>,
}

/// Read, parse and semantically validate a CV YAML file (plan 2.1/2.2).
///
/// Parse errors and semantic errors both surface as span-labeled diagnostics
/// (Error::Input); warnings ride along in [`LoadedCv::warnings`] and never fail.
pub fn load_cv(file: &Path, font_paths: &[PathBuf]) -> Result<LoadedCv> {
    let yaml = std::fs::read_to_string(file).map_err(|source| Error::Io {
        path: file.display().to_string(),
        source,
    })?;
    let name = file
        .file_name()
        .unwrap_or(file.as_os_str())
        .to_string_lossy()
        .into_owned();

    // The typed parse owns the error shape: every failure here is a YAML problem
    // with a source location, so it always becomes a span-labeled diagnostic.
    let document = Document::parse_yaml(&yaml).map_err(|source| Error::Input {
        diagnostics: vec![Diag::from_yaml_error(source, &yaml, &name)],
    })?;

    let mut font_dirs: Vec<PathBuf> = font_paths.to_vec();
    for rel in &document.design.font_paths {
        font_dirs.push(resolve_relative(file, rel));
    }

    let base_dir = file.parent().unwrap_or(Path::new("."));
    match crate::validate::validate(&yaml, &name, &document, &font_dirs, base_dir) {
        Outcome::Invalid(errors) => Err(Error::Input {
            diagnostics: errors,
        }),
        Outcome::Valid {
            template,
            font_items,
            warnings,
        } => Ok(LoadedCv {
            yaml,
            document,
            template,
            warnings,
            font_items,
        }),
    }
}

/// Write one line of essential output to stdout.
///
/// Failures here are not the command's business: `cv fonts | head -1` closes the pipe
/// after the first line, and hanging up is the reader's choice, not an error to report
/// (or a panic to show).
pub fn out_line(text: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{text}");
}

/// One line to stderr, best-effort for the same reason as [`out_line`].
pub fn err_line(text: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// A pre-formatted diagnostic to stderr, best-effort: `cv check cv.yaml 2>&1 | head -1`
/// closes the pipe while a file with many problems still has kilobytes of report to
/// write, and Rust turns that `EPIPE` into a panic inside `eprint!`. Every diagnostic
/// goes through here so a hung-up reader stops the writing instead of crashing the run.
pub fn err_text(text: &str) {
    use std::io::Write;
    let _ = write!(std::io::stderr(), "{text}");
}

/// Print validation warnings (if any) to stderr (ADR-0015: warnings never fail).
pub fn report_warnings(warnings: &[Diag]) {
    let is_tty = std::io::stderr().is_terminal();
    for warning in warnings {
        err_text(&crate::error::render_diag(warning, is_tty));
    }
}

/// The fit-overflow warning at the floor (spec §7 step 4, ADR-0026): plain text —
/// the cause is content volume, not one YAML value, so there is no span to label.
/// The spill is measured against the target page count (`design.pages`).
pub fn report_overflow(output: &crate::render::RenderOutput, target_pages: usize) {
    let message = format!(
        "content still overflows at the fit floor: about {} lines spill onto {} extra page(s); cut content (e.g. bullets or paragraphs), raise design.pages, or relax design.fit_floor / margins",
        output.overflow_lines,
        output.page_count.saturating_sub(target_pages),
    );
    let is_tty = std::io::stderr().is_terminal();
    err_text(&crate::error::render_diag(
        &Diag::plain(message, Severity::Warning),
        is_tty,
    ));
}

/// `design.font_paths` entries are relative to the YAML file's directory (spec §4 example).
fn resolve_relative(input: &Path, rel: &str) -> PathBuf {
    let base = input.parent().unwrap_or(Path::new("."));
    base.join(rel)
}
