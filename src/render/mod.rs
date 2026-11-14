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
//! - `tagged: true` (the typst-pdf default) gives a tagged, accessibility-friendly PDF
//!   on PDF 1.7, with no PDF/A or PDF/UA validator — the §14 question, closed in
//!   ADR-0038.

pub mod data;
pub mod fonts;

use typst::foundations::{Dict, IntoValue, Smart};
use typst_as_lib::{TypstEngine, TypstTemplateCollection, typst_kit_options::TypstKitFontOptions};
use typst_pdf::{PdfOptions, PdfStandards};

use crate::templates::Template;

pub use data::page_name;
pub use fonts::{FontListing, FontSource, available_fonts, bundled_fonts};

/// Everything needed to compile a CV once (plan 1.4.2). The caller reads the user font
/// directories: their files are inputs, and an unreadable directory is an I/O error at
/// the point where the path came from the user, not a render failure.
pub struct RenderRequest {
    /// Loaded template (main source + extra sources).
    pub template: Template,
    /// The fully resolved `data.yaml` (see [`data::to_yaml`]).
    pub data_yaml: String,
    /// Font files from `design.font_paths` and `--font-path`, in that order.
    pub user_font_items: Vec<Vec<u8>>,
}

/// One compile's result (spec §9).
#[derive(Debug)]
pub struct RenderOutput {
    pub pdf: Vec<u8>,
    pub page_count: usize,
    /// Typst warnings (e.g. unknown font family). Empty when the compile was clean.
    pub warnings: Vec<String>,
    /// Shaped text runs on pages beyond the target page count (0 when there are
    /// none) — the "about N lines" figure of the fit-overflow warning (spec §7
    /// step 4, ADR-0026). One run is one style span within a line, so lines mixing
    /// styles (Markdown tokens) count slightly high; it is an approximation
    /// offered only in human text, never a contract value.
    pub overflow_lines: usize,
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
}

/// Fit-loop scale factors (spec §7), passed to the template through `sys.inputs.fit`
/// (never through `data.yaml`, ADR-0006). The fit loop (`src/fit.rs`) drives these;
/// the identity means "no scaling".
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

/// One-shot render (spec §9), used by tests. `cv build` compiles through
/// [`Engine`] directly so the fit loop can reuse it across iterations. The
/// one-shot path targets one page (`design.pages`, default 1).
pub fn render(req: &RenderRequest) -> std::result::Result<RenderOutput, RenderError> {
    Engine::build(req).compile(FitParams::default(), 1)
}

/// A built compiler environment: fonts and template sources loaded once, reused for every
/// compile of the same build (plan 1.4.4). Typst types are contained in this module.
pub struct Engine {
    engine: TypstEngine<TypstTemplateCollection>,
    options: PdfOptions,
}

impl Engine {
    /// Load fonts (user files → bundled → system) and template sources. Heaviest step;
    /// done once per build and reused for every fit-loop compile (plan 1.4.4).
    pub fn build(req: &RenderRequest) -> Self {
        let mut sources: Vec<(&str, String)> =
            vec![("template.typ", req.template.main_source.clone())];
        sources.extend(
            req.template
                .extra_sources
                .iter()
                .map(|(p, c)| (p.as_str(), c.clone())),
        );

        let mut font_items: Vec<Vec<u8>> = req.user_font_items.clone();
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

        Self { engine, options }
    }

    /// Compile the document with the given fit scales (spec §9 interface). Overflow
    /// runs are counted on pages beyond `target_pages` — the "about N lines" figure
    /// of the fit-overflow warning.
    pub fn compile(
        &self,
        fit: FitParams,
        target_pages: usize,
    ) -> std::result::Result<RenderOutput, RenderError> {
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
            // The exporter can refuse a document that compiled: `pdf.attach` with a
            // malformed MIME type, `pdf.embed` of a file that is not a PDF, an image
            // format it cannot write. Custom templates can call any Typst function, so
            // this is a real user-facing class, distinct from a compile failure.
            .map_err(|diags| RenderError::Pdf(format_diags(&diags)))?;

        let page_count = doc.pages().len();
        Ok(RenderOutput {
            pdf,
            page_count,
            warnings,
            overflow_lines: if page_count > target_pages {
                doc.pages()
                    .iter()
                    .skip(target_pages)
                    .map(|page| count_text_runs(&page.frame))
                    .sum()
            } else {
                0
            },
        })
    }
}

/// Count shaped text runs in a laid-out frame, descending into transform groups.
/// Typst builds one `Text` frame item per style span of a line (verified against
/// the pinned 0.15.1 `FrameItem` docs), so this is the overflow-lines proxy of
/// ADR-0026, computed where Typst types are allowed to live (spec §13.7).
fn count_text_runs(frame: &typst::layout::Frame) -> usize {
    frame
        .items()
        .map(|(_, item)| match item {
            typst::layout::FrameItem::Group(group) => count_text_runs(&group.frame),
            typst::layout::FrameItem::Text(_) => 1,
            _ => 0,
        })
        .sum()
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

    #[test]
    fn template_compile_errors_are_labeled_and_formatted_with_hints() {
        let error = render(&test_request("#assert(false, message: \"boom\")"))
            .expect_err("broken template must fail");
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
        let error = render(&test_request("#assert(false, message: \"boom\")"))
            .expect_err("broken template must fail");
        let rendered = crate::error::report(&Error::Render(error), false);
        assert!(
            rendered.contains("Error: template error:"),
            "got: {rendered}"
        );
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

    fn test_request(main_source: &str) -> RenderRequest {
        RenderRequest {
            template: crate::templates::Template {
                name: "test".into(),
                description: String::new(),
                supported_options: vec![],
                main_source: main_source.into(),
                extra_sources: vec![],
            },
            data_yaml: "cv: {}".into(),
            user_font_items: vec![],
        }
    }

    #[test]
    fn overflowing_compiles_count_text_runs_past_the_first_page() {
        let output = render(&test_request("#set page(height: 2cm)\n#lorem(400)"))
            .expect("plain typst source renders");
        assert!(output.page_count > 2, "got {} pages", output.page_count);
        assert!(
            output.overflow_lines >= output.page_count - 1,
            "every overflow page carries at least one text run, got {}",
            output.overflow_lines
        );
    }

    /// A document can compile fine and still be refused by the PDF exporter: a template
    /// that calls `pdf.attach` with a malformed MIME type is the cheapest case. That is
    /// its own error class — the Typst source was valid, the PDF was not — and a custom
    /// template can hit it, so the exporter's reason must reach the user (spec §5).
    #[test]
    fn a_document_the_pdf_exporter_refuses_is_an_export_error() {
        let error = render(&test_request(
            "#pdf.attach(\"data.yaml\", mime-type: \"not a mime type\")",
        ))
        .expect_err("the exporter must refuse a malformed mime type");
        let message = error.to_string();
        assert!(
            message.starts_with("PDF export failed:"),
            "typed as an export error, not a template error: {message}"
        );
        assert!(
            message.contains("mime type"),
            "the exporter's own reason is kept: {message}"
        );
    }

    #[test]
    fn single_page_compile_has_no_overflow_lines() {
        let output = render(&test_request("#set page(height: 5cm)\n#lorem(20)"))
            .expect("plain typst source renders");
        assert_eq!(output.page_count, 1);
        assert_eq!(output.overflow_lines, 0);
    }
}
