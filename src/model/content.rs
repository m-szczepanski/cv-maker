//! Content model: everything under the `cv` key (spec §4).

use serde::{Deserialize, Serialize};

/// The CV content.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cv {
    /// Full name. Used for the document title/author metadata too (spec §9).
    pub name: String,

    /// Short role statement under the name.
    #[serde(default)]
    pub headline: Option<String>,

    /// Where the person is based.
    #[serde(default)]
    pub location: Option<String>,

    /// Email, phone, and links.
    #[serde(default)]
    pub contact: Option<Contact>,

    /// Short professional summary paragraph.
    #[serde(default)]
    pub summary: Option<String>,

    /// GDPR consent statement, rendered as small muted text at the bottom of the
    /// last page. Literal text (no Markdown), the user's own wording. Defaults
    /// to [`DEFAULT_GDPR`]; `gdpr: null` opts out of the footer entirely
    /// (ADR-0045).
    #[serde(default = "default_gdpr")]
    pub gdpr: Option<String>,

    /// Ordered content sections. An empty list is valid.
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// The consent clause assumed when `cv.gdpr` is absent — the standard wording
/// for a recruitment application under Regulation (EU) 2016/679 (ADR-0045).
/// Users who need different wording (or none) set the field.
pub const DEFAULT_GDPR: &str = "I hereby give consent for my personal data included in my application to be processed for the purposes of the recruitment process in accordance with Regulation (EU) 2016/679 of the European Parliament and of the Council of 27 April 2016 (GDPR).";

fn default_gdpr() -> Option<String> {
    Some(DEFAULT_GDPR.to_string())
}

impl Cv {
    /// All `(index, url)` pairs from the contact links, in order — for URL validation.
    pub fn links(&self) -> Vec<(usize, &str)> {
        self.contact
            .as_ref()
            .and_then(|c| c.links.as_ref())
            .map(|links| {
                links
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (i, l.url.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every text that carries the inline Markdown subset (ADR-0016): the summary, the
    /// bullets of `entries` items, and the items of `bullets` and `text` sections —
    /// each with the path the span scan addresses it by, so a problem found in the
    /// parsed tokens can be labeled in the source.
    pub fn markdown_texts(&self) -> Vec<MarkdownText<'_>> {
        let mut fields = Vec::new();
        if let Some(summary) = &self.summary {
            fields.push(MarkdownText {
                span_path: "cv.summary".to_owned(),
                text: summary,
            });
        }
        for (section, item) in self.sections.iter().enumerate() {
            match &item.body {
                SectionBody::Entries { items } => {
                    for (index, entry) in items.iter().enumerate() {
                        for (bullet, text) in entry.bullets.iter().flatten().enumerate() {
                            fields.push(MarkdownText {
                                span_path: format!(
                                    "cv.sections[{section}].items[{index}].bullets[{bullet}]"
                                ),
                                text,
                            });
                        }
                    }
                }
                SectionBody::Bullets { items } | SectionBody::Text { items } => {
                    for (index, text) in items.iter().enumerate() {
                        fields.push(MarkdownText {
                            span_path: format!("cv.sections[{section}].items[{index}]"),
                            text,
                        });
                    }
                }
                SectionBody::KeyValues { .. } => {}
            }
        }
        fields
    }
}

/// One Markdown-enabled text field and where it lives in the source (see
/// [`Cv::markdown_texts`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownText<'a> {
    pub span_path: String,
    pub text: &'a str,
}

/// Contact details.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub links: Option<Vec<Link>>,
}

/// A labeled external link.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// One content section: a title plus type-specific items (spec §4 content model).
///
/// Note: `deny_unknown_fields` is deliberately omitted here because it is incompatible
/// with `#[serde(flatten)]` (the tag key `type` is consumed by [`SectionBody`]). Unknown
/// keys are still rejected: the remaining map is forwarded to the tagged enum, whose
/// variant structs carry `deny_unknown_fields`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Section {
    pub title: String,
    #[serde(flatten)]
    pub body: SectionBody,
}

/// The closed set of section types for v1 (spec §4).
///
/// Modeled as an internally tagged enum keyed on the YAML `type` field so the schema shows
/// exactly the four allowed values. Unknown keys inside a variant are rejected by the
/// variant structs' `deny_unknown_fields`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SectionBody {
    /// heading/org/location/dates/bullets items (experience, education).
    Entries {
        #[serde(default)]
        items: Vec<EntryItem>,
    },
    /// key/value rows (skills).
    KeyValues {
        #[serde(default)]
        items: Vec<KeyValueItem>,
    },
    /// Flat bullet list.
    Bullets {
        #[serde(default)]
        items: Vec<String>,
    },
    /// Paragraph text. Each item is one paragraph.
    Text {
        #[serde(default)]
        items: Vec<String>,
    },
}

/// One `entries` item.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryItem {
    /// Role or degree title.
    pub heading: String,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    /// Free-form string (spec §4: no locale-aware date handling).
    #[serde(default)]
    pub dates: Option<String>,
    #[serde(default)]
    pub bullets: Option<Vec<String>>,
}

/// One `key_values` item.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyValueItem {
    pub key: String,
    pub value: String,
}
