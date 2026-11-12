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
pub use fonts::{bundled_family_names, bundled_fonts};

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
#[derive(Debug, Clone, Copy)]
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

        let mut font_items: Vec<Vec<u8>> = Vec::new();
        for (path, bytes) in fonts::collect_font_dir_files(&req.user_font_dirs)
            .map_err(|e| RenderError::Font(e.to_string()))?
        {
            // Keep the path in the error message context if parsing ever fails.
            let _ = path;
            font_items.push(bytes);
        }
        for (_, bytes) in fonts::bundled_fonts() {
            font_items.push(bytes.to_vec());
        }

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
            .map(|w| format!("typst warning: {}", w.message))
            .collect();

        let doc = warned.output.map_err(|e| RenderError::Template {
            message: format_typst_as_lib_error(&e),
        })?;

        let pdf = typst_pdf::pdf(&doc, &self.options)
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
