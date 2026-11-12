//! Error types (plan 1.2.2). Converted to `miette` diagnostics at the CLI boundary in M2.

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A YAML input file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    /// A YAML file could not be parsed or deserialized into the model.
    #[error("{context}")]
    Yaml {
        context: String,
        source: serde_yaml_ng::Error,
    },

    /// A value has the right shape but invalid content (used by strict value parsing).
    #[error("invalid value {value:?}: expected {expected}")]
    InvalidValue { value: String, expected: String },

    /// A command was invoked with an unsupported value (e.g. unknown template name in M1).
    #[error("{0}")]
    Unsupported(String),

    /// Rendering failed (template errors, PDF export, fonts). `RenderError::Template`
    /// renders as "template error: …" so template problems stay distinct from user YAML
    /// errors (spec §5, plan 1.6.4).
    #[error(transparent)]
    Render(#[from] crate::render::RenderError),
}

impl Error {
    /// Wrap a `serde_yaml_ng` error, appending the offending source line/column when the
    /// error carries a location. Full span-labeled diagnostics arrive in M2.
    pub fn yaml(source: serde_yaml_ng::Error, _yaml: &str) -> Self {
        let location = source.location();
        let context = match location {
            Some(loc) => format!("at line {}, column {}", loc.line(), loc.column()),
            None => String::new(),
        };
        let mut msg = source.to_string();
        if !context.is_empty() {
            msg = format!("{msg} ({context})");
        }
        Error::Yaml {
            context: msg,
            source,
        }
    }

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

/// Render an error for human consumption at the CLI boundary.
/// M2 replaces this with `miette`-rendered diagnostics (plan 1.2.2).
pub fn report(err: &Error) -> String {
    match err {
        Error::Yaml { context, .. } => format!("invalid CV YAML: {context}"),
        other => other.to_string(),
    }
}
