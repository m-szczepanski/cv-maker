//! Error types (plan 1.2.2). Converted to `miette`-rendered diagnostics at the CLI
//! boundary (ADR-0018).
//!
//! The internal [`Error`] enum stays `thiserror`-based; user-facing *input* problems are
//! collected as [`Diag`]s (message + labeled span + help), which carry their own source
//! text so the renderer can show snippets.

use miette::{Diagnostic, LabeledSpan, NamedSource, Severity};

pub type Result<T> = std::result::Result<T, Error>;

/// One user-facing diagnostic: a message, optionally pointing into the YAML source, with
/// an optional help line (ADR-0018). Carries the source text so the renderer can label it.
#[derive(Debug, Clone)]
pub struct Diag {
    pub message: String,
    pub labels: Vec<LabeledSpan>,
    pub help: Option<String>,
    pub severity: Severity,
    pub source: Option<NamedSource<String>>,
}

impl Diag {
    /// An error with a labeled span in the given source.
    pub fn error(
        message: impl Into<String>,
        label: impl Into<String>,
        span: (usize, usize),
        source: NamedSource<String>,
    ) -> Self {
        Self {
            message: message.into(),
            labels: vec![LabeledSpan::new(
                Some(label.into()),
                span.0,
                span.1.saturating_sub(span.0),
            )],
            help: None,
            severity: Severity::Error,
            source: Some(source),
        }
    }

    /// Convert a typed-parse (serde) error into a span-labeled diagnostic (ADR-0014).
    ///
    /// The location comes from the error itself; "unknown field/variant" mistakes get a
    /// "did you mean" hint by suggesting against the list of accepted names embedded in
    /// the serde message (spec §5 quality bar).
    pub fn from_yaml_error(source: serde_yaml_ng::Error, yaml: &str, name: &str) -> Self {
        let message = source.to_string();
        let span = source.location().map_or(0, |l| l.index());
        let hint = unknown_key_hint(&message);
        let diag = Diag::error(
            message,
            "here",
            (span, span + 1),
            NamedSource::new(name, yaml.to_owned()),
        );
        match hint {
            Some(help) => diag.with_help(help),
            None => diag,
        }
    }

    /// Attach a help line ("did you mean …?", next steps).
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// A plain message without a span (no source attached).
    pub fn plain(message: impl Into<String>, severity: Severity) -> Self {
        Self {
            message: message.into(),
            labels: Vec::new(),
            help: None,
            severity,
            source: None,
        }
    }
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Diag {}

/// Extract a "did you mean" hint from a serde `unknown field` / `unknown variant`
/// message by suggesting against the names the message lists after "expected one of".
/// Returns `None` when the message does not have that shape or nothing is close.
fn unknown_key_hint(message: &str) -> Option<String> {
    let after_marker = message
        .split_once("unknown field `")
        .or_else(|| message.split_once("unknown variant `"))?
        .1;
    // `str::split` yields at least one piece; the first is the quoted name.
    let unknown = after_marker.split('`').next().unwrap_or_default();
    let expected_list = message.split_once("expected one of ")?.1;
    let expected: Vec<String> = expected_list
        .split("`, `")
        .map(|part| part.trim_matches('`').trim_end_matches('.').to_owned())
        .collect();
    let best = crate::suggest::closest_match(unknown, &expected)?;
    Some(format!("did you mean '{best}'?"))
}

impl Diagnostic for Diag {
    fn severity(&self) -> Option<Severity> {
        Some(self.severity)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            None
        } else {
            Some(Box::new(self.labels.iter().cloned()))
        }
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn std::fmt::Display + 'a>)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.source.as_ref().map(|s| s as &dyn miette::SourceCode)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A YAML input file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    /// A YAML file could not be parsed or deserialized into the model. `context` is the
    /// `serde_yaml_ng` message, which includes the source line/column when known;
    /// commands convert this into a span-labeled [`Diag`] before reporting (ADR-0014).
    #[error("{context}")]
    Yaml {
        context: String,
        source: serde_yaml_ng::Error,
    },

    /// Semantic validation failed or produced errors: the collected diagnostics
    /// (already carrying their source, ADR-0014/0018).
    #[error("invalid CV input")]
    Input { diagnostics: Vec<Diag> },

    /// A value has the right shape but invalid content (used by strict value parsing).
    #[error("invalid value {value:?}: expected {expected}")]
    InvalidValue { value: String, expected: String },

    /// A command was invoked with a value it cannot act on (an unknown `--template`,
    /// a path with no file name).
    #[error("{0}")]
    Unsupported(String),

    /// A template folder or built-in asset could not be loaded (plan 5.3, ADR-0027):
    /// missing folder, missing `template.typ` or `manifest.yaml`, invalid or unusable
    /// manifest, non-UTF-8 source. `name` is the reference as the user wrote it, so the
    /// message always matches the typed value.
    #[error("template '{name}': {message}")]
    TemplateLoad { name: String, message: String },

    /// An internal invariant failed (unexpected serialization failure, malformed built-in
    /// assets). User-visible so it is distinguishable from input errors; never silently
    /// swallowed.
    #[error("internal error: {0}")]
    Internal(String),

    /// Writing the output PDF failed.
    #[error("could not write {path}: {message}")]
    Write { path: String, message: String },

    /// Rendering failed (template errors, PDF export, fonts). `RenderError::Template`
    /// renders as "template error: …" so template problems stay distinct from user YAML
    /// errors (spec §5, plan 1.6.4).
    #[error(transparent)]
    Render(#[from] crate::render::RenderError),
}

impl Error {
    pub fn invalid_value(value: &str, expected: &str) -> Self {
        Error::InvalidValue {
            value: value.to_owned(),
            expected: expected.to_owned(),
        }
    }
}

impl From<serde_yaml_ng::Error> for Error {
    fn from(source: serde_yaml_ng::Error) -> Self {
        Error::Yaml {
            context: source.to_string(),
            source,
        }
    }
}

/// Width every report is wrapped at, so diagnostics render identically regardless of the
/// terminal (ADR-0018; snapshots depend on this).
pub const REPORT_WIDTH: usize = 100;

/// Render one diagnostic with the graphical handler (ADR-0018).
///
/// The theme is always unicode-without-color so output is deterministic and safe to
/// redirect; the `is_tty` flag selects the colored variant for interactive use.
pub fn render_diag(diag: &Diag, is_tty: bool) -> String {
    let theme = if is_tty {
        miette::GraphicalTheme::unicode()
    } else {
        miette::GraphicalTheme::unicode_nocolor()
    };
    let handler = miette::GraphicalReportHandler::new_themed(theme).with_width(REPORT_WIDTH);
    let mut out = String::new();
    // `render_report` fails only through its writer, and writing to a `String` cannot
    // fail; the discarded result is the `fmt::Error` the signature carries anyway.
    let _ = handler.render_report(&mut out, diag);
    out
}

/// Render an error for human consumption at the CLI boundary (plan 1.2.2).
pub fn report(err: &Error, is_tty: bool) -> String {
    match err {
        Error::Input { diagnostics } => diagnostics
            .iter()
            .map(|d| render_diag(d, is_tty))
            .collect::<Vec<_>>()
            .join("\n"),
        other => format!("Error: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labeled_diagnostics_render_with_snippet_and_help() {
        let diag = Diag::error(
            "unknown font \"Inte\"",
            "this font family",
            (10, 14),
            NamedSource::new("cv.yaml", "design:\n  body: Inte\n".to_owned()),
        )
        .with_help("did you mean \"Inter\"?");
        let rendered = render_diag(&diag, false);
        assert!(rendered.contains("unknown font"), "got: {rendered}");
        assert!(rendered.contains("cv.yaml"), "source name shown");
        assert!(rendered.contains("did you mean"), "help shown");
    }

    #[test]
    fn plain_diagnostics_render_without_a_snippet_and_support_the_tty_theme() {
        let diag = Diag::plain("could not write cv.pdf", Severity::Error);
        assert!(diag.labels().is_none(), "plain diagnostics carry no labels");
        let plain = render_diag(&diag, false);
        assert!(!plain.contains("cv.yaml"), "no source, no snippet");
        assert!(plain.contains("could not write cv.pdf"));

        let tty = render_diag(&diag, true);
        assert!(
            tty.contains("could not write cv.pdf"),
            "tty theme renders too"
        );
    }

    #[test]
    fn source_without_labels_renders_header_only() {
        // The shape validation produces when a check knows the value but the span scan
        // did not record it: a message over a source with no labeled range.
        let diag = Diag {
            message: "something is wrong".into(),
            labels: Vec::new(),
            help: None,
            severity: Severity::Error,
            source: Some(NamedSource::new("cv.yaml", "schema: 1\n".to_owned())),
        };
        assert!(diag.labels().is_none());
        let rendered = render_diag(&diag, false);
        assert!(rendered.contains("something is wrong"), "got: {rendered}");
    }

    #[test]
    fn input_errors_render_all_diagnostics() {
        let error = Error::Input {
            diagnostics: vec![
                Diag::plain("first", Severity::Error),
                Diag::plain("second", Severity::Error),
            ],
        };
        let rendered = report(&error, false);
        assert!(
            rendered.contains("first") && rendered.contains("second"),
            "got: {rendered}"
        );
    }

    /// The hint is extracted from serde's own message shapes; every other shape
    /// stays hint-less rather than inventing one.
    #[test]
    fn hints_come_only_from_messages_that_list_the_expected_names() {
        assert_eq!(
            unknown_key_hint("unknown field `nmae`, expected one of `name`, `headline`").as_deref(),
            Some("did you mean 'name'?"),
            "the field spelling a user meant"
        );
        assert_eq!(
            unknown_key_hint(
                "unknown variant `entrie`, expected one of `entries`, `key_values`, `bullets`, `text`"
            )
            .as_deref(),
            Some("did you mean 'entries'?"),
            "a section type is reported as a variant list"
        );
        assert_eq!(
            unknown_key_hint("unknown field `nmae`, expected `one field`"),
            None,
            "no 'expected one of' list means nothing to suggest against"
        );
        assert_eq!(
            unknown_key_hint("invalid type: integer, expected a string"),
            None,
            "an unrelated message is not a name mistake"
        );
        assert_eq!(
            unknown_key_hint("unknown field `xyzzyq`, expected one of `name`, `headline`"),
            None,
            "a typo with no close match must not guess"
        );
    }
}
