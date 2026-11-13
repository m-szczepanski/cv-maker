//! Typst integration (spec §9) — the only module allowed to import Typst types.
//!
//! Public interface (spec §9):
//! ```text
//! RenderRequest → render() → RenderOutput { pdf, page_count, warnings }
//! ```
//!
//! Everything Typst-flavored ([`Engine`], [`PdfOptions`], font books) is private to this
//! module. Compilation is fully in memory: template sources, the data file, and fonts are
//! virtual files; nothing is written to disk.
//!
//! Implementation notes (verified against the pinned versions' sources, plan 1.4.1):
//! - `typst-as-lib 0.17.0` (`TypstEngine`) wraps the Typst `World`; it supports typst 0.15.
//! - `typst-pdf 0.15.1`: with `PdfOptions::timestamp: None` and the template setting
//!   `document(date: none)`, no creation date is written → byte-identical output.
//! - `tagged: true` (the typst-pdf default) gives a tagged, accessibility-friendly PDF;
//!   recorded for the §14 open question in docs/decisions.md.

pub mod data;
pub mod fonts;

use std::path::PathBuf;

use typst::foundations::{Dict, IntoValue, Smart};
use typst_as_lib::{TypstEngine, TypstTemplateCollection, typst_kit_options::TypstKitFontOptions};
use typst_pdf::{PdfOptions, PdfStandards};

use crate::templates::Template;

pub use data::{FitData, page_name};
pub use fonts::{FontListing, FontSource, available_fonts, bundled_fonts};

/// Everything needed to compile a CV once (plan 1.4.2).
pub struct RenderRequest {
    /// Loaded template (main source + extra sources).
    pub template: Template,
    /// The fully resolved `data.yaml` (see [`data::to_yaml`]).
    pub data_yaml: String,
    /// User font directories (`design.font_paths` + `--font-path`).
    pub user_font_dirs: Vec<PathBuf>,
}

/// One compile's result (spec §9).
#[derive(Debug)]
pub struct RenderOutput {
    pub pdf: Vec<u8>,
    pub page_count: usize,
    /// Typst warnings (e.g. unknown font family). Empty when the compile was clean.
    pub warnings: Vec<String>,
}

/// Errors from the render layer (spec §5: template errors are labeled separately from
/// user errors).
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The Typst compiler rejected the template or the data — a template error, not a
    /// user YAML error.
    #[error("template error: {message}")]
    Template { message: String },

    #[error("PDF export failed: {0}")]
    Pdf(String),

    #[error("font error: {0}")]
    Font(String),
}

/// Fit-loop scale factors (spec §7), passed to the template through `sys.inputs.fit`.
/// M1 always passes the identity; the fit loop (M4) drives these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitParams {
    pub font_scale: f64,
    pub spacing_scale: f64,
    pub margin_scale: f64,
}

impl Default for FitParams {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            spacing_scale: 1.0,
            margin_scale: 1.0,
        }
    }
}

impl From<FitParams> for FitData {
    fn from(p: FitParams) -> Self {
        FitData {
            font_scale: p.font_scale,
            spacing_scale: p.spacing_scale,
            margin_scale: p.margin_scale,
        }
    }
}

/// One-shot render (spec §9). The fit loop (M4) will hold an [`Engine`] across iterations.
pub fn render(req: &RenderRequest) -> std::result::Result<RenderOutput, RenderError> {
    Engine::build(req)?.compile(FitParams::default())
}

/// A built compiler environment: fonts and template sources loaded once, reused for every
/// compile of the same build (plan 1.4.4). Typst types are contained in this module.
pub struct Engine {
    engine: TypstEngine<TypstTemplateCollection>,
    options: PdfOptions,
}

impl Engine {
    /// Load fonts (user dirs → bundled → system) and template sources. Heaviest step;
    /// done once per build.
    pub fn build(req: &RenderRequest) -> std::result::Result<Self, RenderError> {
        let mut sources: Vec<(&str, String)> =
            vec![("template.typ", req.template.main_source.clone())];
        sources.extend(
            req.template
                .extra_sources
                .iter()
                .map(|(p, c)| (p.as_str(), c.clone())),
        );

        let mut font_items: Vec<Vec<u8>> = fonts::collect_font_dir_files(&req.user_font_dirs)
            .map_err(|e| RenderError::Font(e.to_string()))?;
        font_items.extend(
            fonts::bundled_fonts()
                .iter()
                .map(|(_, bytes)| bytes.to_vec()),
        );

        let engine = TypstEngine::builder()
            .with_static_source_file_resolver(sources)
            .with_static_file_resolver([("data.yaml", req.data_yaml.as_bytes())])
            .fonts(font_items)
            .search_fonts_with(TypstKitFontOptions::new())
            .build();

        let options = PdfOptions {
            // Stable identity derived from title+author (deterministic across builds).
            ident: Smart::Auto,
            creator: Smart::Custom(Some(format!("cv {}", env!("CARGO_PKG_VERSION")))),
            // Template sets `document(date: none)` → no timestamp in the PDF.
            timestamp: None,
            page_ranges: None,
            standards: PdfStandards::default(),
            // Tagged PDF for baseline accessibility (typst-pdf default, spec §14).
            tagged: true,
            pretty: false,
        };

        Ok(Self { engine, options })
    }

    /// Compile the document with the given fit scales (spec §9 interface).
    pub fn compile(&self, fit: FitParams) -> std::result::Result<RenderOutput, RenderError> {
        let warned = self
            .engine
            .compile_with_input("template.typ", fit_to_inputs(fit));
        let warnings: Vec<String> = warned
            .warnings
            .iter()
            .map(|w| w.message.to_string())
            .collect();
        let doc = warned.output.map_err(|e| RenderError::Template {
            message: format_typst_as_lib_error(&e),
        })?;

        let pdf = typst_pdf::pdf(&doc, &self.options)
            // PDF export of an already-compiled document only fails on options misuse
            // (invalid standard combo); the options are fixed constants here, so this is
            // not reachable from user input or template bugs.
            .map_err(|diags| RenderError::Pdf(format_diags(&diags)))?;

        Ok(RenderOutput {
            page_count: doc.pages().len(),
            pdf,
            warnings,
        })
    }
}

/// Build the `sys.inputs` dict the template reads: `sys.inputs.fit.font_scale` etc.
fn fit_to_inputs(fit: FitParams) -> Dict {
    let mut fit_dict = Dict::new();
    fit_dict.insert("font_scale".into(), fit.font_scale.into_value());
    fit_dict.insert("spacing_scale".into(), fit.spacing_scale.into_value());
    fit_dict.insert("margin_scale".into(), fit.margin_scale.into_value());
    let mut inputs = Dict::new();
    inputs.insert("fit".into(), fit_dict.into_value());
    inputs
}

/// Format Typst compiler diagnostics, one line per diagnostic plus hints (plan 1.6.4).
fn format_diags<'a>(diags: impl IntoIterator<Item = &'a typst::diag::SourceDiagnostic>) -> String {
    diags
        .into_iter()
        .map(|d| {
            let hints: Vec<String> = d.hints.iter().map(|h| h.v.to_string()).collect();
            let hints = if hints.is_empty() {
                String::new()
            } else {
                format!(" (hints: {})", hints.join("; "))
            };
            format!("{}{hints}", d.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_typst_as_lib_error(e: &typst_as_lib::TypstAsLibError) -> String {
    match e {
        typst_as_lib::TypstAsLibError::TypstSource(diags) => format_diags(diags),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn broken_template_request() -> RenderRequest {
        RenderRequest {
            template: crate::templates::Template {
                name: "test".into(),
                description: String::new(),
                supported_options: vec![],
                main_source: "#assert(false, message: \"boom\")".into(),
                extra_sources: vec![],
            },
            data_yaml: "cv: {}".into(),
            user_font_dirs: vec![],
        }
    }

    #[test]
    fn template_compile_errors_are_labeled_and_formatted_with_hints() {
        let error = render(&broken_template_request()).expect_err("broken template must fail");
        let message = error.to_string();
        assert!(
            message.contains("template error:"),
            "labeled as a template error, got: {message}"
        );
        assert!(
            message.contains("boom"),
            "carries the diagnostic: {message}"
        );
    }

    #[test]
    fn template_error_label_reaches_the_cli_report() {
        let error = render(&broken_template_request()).expect_err("broken template must fail");
        let rendered = crate::error::report(&Error::Render(error), false);
        assert!(
            rendered.contains("Error: template error:"),
            "got: {rendered}"
        );
    }

    #[test]
    fn fit_params_convert_into_fit_data() {
        let data = FitData::from(FitParams {
            font_scale: 0.9,
            spacing_scale: 0.8,
            margin_scale: 1.0,
        });
        assert_eq!(data.font_scale, 0.9);
        assert_eq!(data.spacing_scale, 0.8);
        assert_eq!(data.margin_scale, 1.0);
    }

    #[test]
    fn diagnostics_without_hints_format_to_the_bare_message() {
        let diag = typst::diag::SourceDiagnostic::error(
            typst::syntax::DiagSpan::detached(),
            "unknown variable",
        );
        assert_eq!(format_diags(std::iter::once(&diag)), "unknown variable");
    }

    #[test]
    fn diagnostics_with_hints_append_the_hint_list() {
        let mut diag = typst::diag::SourceDiagnostic::error(
            typst::syntax::DiagSpan::detached(),
            "unknown variable",
        );
        diag.hints.push(typst::syntax::Spanned::detached(
            "did you mean 'body'?".into(),
        ));
        assert_eq!(
            format_diags(std::iter::once(&diag)),
            "unknown variable (hints: did you mean 'body'?)"
        );
    }

    #[test]
    fn non_source_lib_errors_fall_back_to_display() {
        let error = typst_as_lib::TypstAsLibError::Unspecified("missing main file".into());
        assert_eq!(
            format_typst_as_lib_error(&error),
            "Unspecified: missing main file!",
            "non-diagnostic errors render through their Display impl"
        );
    }
}
