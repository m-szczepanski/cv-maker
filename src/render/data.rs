//! Serialization of the fully resolved CV data into the template-facing `data.yaml`
//! (spec §6 contract).
//!
//! Design decisions recorded in docs/decisions.md:
//! - The data file is YAML (read by templates via Typst's built-in `yaml()`), not JSON.
//! - Lengths are serialized as `{value, unit}` objects because Typst has no string-to-length
//!   conversion; the user-facing YAML keeps the plain `"1.5cm"` string form.
//! - The tool never generates Typst source from user data; user text reaches templates only
//!   through this file.

use serde::Serialize;

use crate::markdown::MdString;
use crate::model::{Contact, Cv, Design, Page};

/// The document handed to templates: `cv` and the fully resolved `design`.
/// The fit-loop scales are **not** in this file — they arrive through
/// `sys.inputs.fit`, so refits never invalidate the data contract (ADR-0006).
///
/// The `cv` block is mirrored into template-facing structs: the Markdown-enabled fields
/// (summary, bullets, text items — ADR-0016) serialize as token lists, everything else
/// passes through verbatim.
#[derive(Debug, Serialize)]
pub struct DataFile<'a> {
    pub cv: CvData<'a>,
    pub design: DesignData<'a>,
}

impl<'a> DataFile<'a> {
    pub fn new(cv: &'a Cv, design: &'a Design) -> Self {
        Self {
            cv: CvData::from(cv),
            design: DesignData::from(design),
        }
    }
}

/// Template-facing `cv`: plain fields stay plain, Markdown fields become token lists.
#[derive(Debug, Serialize)]
pub struct CvData<'a> {
    pub name: &'a str,
    pub headline: Option<&'a str>,
    pub location: Option<&'a str>,
    pub contact: Option<&'a Contact>,
    pub summary: Option<MdString<'a>>,
    pub sections: Vec<SectionData<'a>>,
}

impl<'a> From<&'a Cv> for CvData<'a> {
    fn from(cv: &'a Cv) -> Self {
        Self {
            name: &cv.name,
            headline: cv.headline.as_deref(),
            location: cv.location.as_deref(),
            contact: cv.contact.as_ref(),
            summary: cv.summary.as_deref().map(MdString),
            sections: cv.sections.iter().map(SectionData::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SectionData<'a> {
    pub title: &'a str,
    #[serde(flatten)]
    pub body: SectionBodyData<'a>,
}

impl<'a> From<&'a crate::model::Section> for SectionData<'a> {
    fn from(section: &'a crate::model::Section) -> Self {
        Self {
            title: &section.title,
            body: SectionBodyData::from(&section.body),
        }
    }
}

/// The tagged section body, re-tagged so `data.yaml` keeps the same `type` key.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SectionBodyData<'a> {
    Entries {
        items: Vec<EntryItemData<'a>>,
    },
    KeyValues {
        items: Vec<&'a crate::model::KeyValueItem>,
    },
    Bullets {
        items: Vec<MdString<'a>>,
    },
    Text {
        items: Vec<MdString<'a>>,
    },
}

impl<'a> From<&'a crate::model::SectionBody> for SectionBodyData<'a> {
    fn from(body: &'a crate::model::SectionBody) -> Self {
        match body {
            crate::model::SectionBody::Entries { items } => Self::Entries {
                items: items.iter().map(EntryItemData::from).collect(),
            },
            crate::model::SectionBody::KeyValues { items } => Self::KeyValues {
                items: items.iter().collect(),
            },
            crate::model::SectionBody::Bullets { items } => Self::Bullets {
                items: items.iter().map(|b| MdString(b.as_str())).collect(),
            },
            crate::model::SectionBody::Text { items } => Self::Text {
                items: items.iter().map(|t| MdString(t.as_str())).collect(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EntryItemData<'a> {
    pub heading: &'a str,
    pub org: Option<&'a str>,
    pub location: Option<&'a str>,
    pub dates: Option<&'a str>,
    pub bullets: Option<Vec<MdString<'a>>>,
}

impl<'a> From<&'a crate::model::EntryItem> for EntryItemData<'a> {
    fn from(item: &'a crate::model::EntryItem) -> Self {
        Self {
            heading: &item.heading,
            org: item.org.as_deref(),
            location: item.location.as_deref(),
            dates: item.dates.as_deref(),
            bullets: item
                .bullets
                .as_ref()
                .map(|bullets| bullets.iter().map(|b| MdString(b.as_str())).collect()),
        }
    }
}

/// Design data as templates see it: no missing values, lengths structured, colors as hex.
#[derive(Debug, Serialize)]
pub struct DesignData<'a> {
    pub template: String,
    /// `"a4"` or `"letter"`.
    pub page: String,
    pub margins: MarginsData,
    pub fonts: FontsData<'a>,
    pub sizes: SizesData,
    pub spacing: SpacingData,
    pub colors: ColorsData,
    pub fit_one_page: bool,
    pub fit_floor: FitFloorData,
    pub font_paths: &'a [String],
}

impl<'a> From<&'a Design> for DesignData<'a> {
    fn from(d: &'a Design) -> Self {
        Self {
            template: d.template.clone(),
            page: page_name(d.page),
            margins: MarginsData {
                x: d.margins.x.to_data(),
                y: d.margins.y.to_data(),
            },
            fonts: FontsData {
                body: &d.fonts.body,
                heading: &d.fonts.heading,
                mono: &d.fonts.mono,
            },
            sizes: SizesData {
                name: d.sizes.name.to_data(),
                heading: d.sizes.heading.to_data(),
                body: d.sizes.body.to_data(),
                small: d.sizes.small.to_data(),
            },
            spacing: SpacingData {
                section_gap: d.spacing.section_gap.to_data(),
                entry_gap: d.spacing.entry_gap.to_data(),
                line_height: d.spacing.line_height.0,
            },
            colors: ColorsData {
                accent: d.colors.accent.to_hex(),
                text: d.colors.text.to_hex(),
                muted: d.colors.muted.to_hex(),
            },
            fit_one_page: d.fit_one_page,
            fit_floor: FitFloorData {
                min_body_size: d.fit_floor.min_body_size.to_data(),
                min_scale: d.fit_floor.min_scale,
            },
            font_paths: &d.font_paths,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MarginsData {
    pub x: crate::model::value::LengthData,
    pub y: crate::model::value::LengthData,
}

#[derive(Debug, Serialize)]
pub struct FontsData<'a> {
    pub body: &'a str,
    pub heading: &'a str,
    pub mono: &'a str,
}

#[derive(Debug, Serialize)]
pub struct SizesData {
    pub name: crate::model::value::LengthData,
    pub heading: crate::model::value::LengthData,
    pub body: crate::model::value::LengthData,
    pub small: crate::model::value::LengthData,
}

#[derive(Debug, Serialize)]
pub struct SpacingData {
    pub section_gap: crate::model::value::LengthData,
    pub entry_gap: crate::model::value::LengthData,
    /// Unitless line-height multiplier.
    pub line_height: f64,
}

#[derive(Debug, Serialize)]
pub struct ColorsData {
    /// Lowercase `#rrggbb`.
    pub accent: String,
    pub text: String,
    pub muted: String,
}

#[derive(Debug, Serialize)]
pub struct FitFloorData {
    pub min_body_size: crate::model::value::LengthData,
    pub min_scale: f64,
}

/// Lowercase page name used in `data.yaml` and in error messages.
pub fn page_name(page: Page) -> String {
    match page {
        Page::A4 => "a4".to_owned(),
        Page::Letter => "letter".to_owned(),
    }
}

/// Serialize the data file to a YAML string (deterministic: struct field order).
///
/// Our own structs always serialize — every field is a string, a float, or a sequence of
/// those, and `serde_yaml_ng` writes non-finite floats as `.nan`/`.inf` rather than
/// failing (a nonsense `design.fit_floor.min_scale` reaches here that way, and
/// `FitConfig::from_design` is what contains it). An error would be a `cv` bug (rule 14).
pub fn to_yaml(cv: &Cv, design: &Design) -> String {
    serde_yaml_ng::to_string(&DataFile::new(cv, design))
        .expect("the template-facing data file always serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;

    const SOURCE: &str = concat!(
        "schema: 1\n",
        "cv:\n",
        "  name: Jane Doe\n",
        "  summary: staff **engineer** at [Acme](https://acme.dev)\n",
        "  sections:\n",
        "    - title: Work\n",
        "      type: entries\n",
        "      items:\n",
        "        - heading: Staff Engineer\n",
        "          bullets:\n",
        "            - led the *rewrite*\n",
        "            - plain bullet\n",
        "    - title: Skills\n",
        "      type: key_values\n",
        "      items:\n",
        "        - key: Rust\n",
        "          value: strong\n",
        "    - title: Points\n",
        "      type: bullets\n",
        "      items:\n",
        "        - first `point`\n",
        "    - title: About\n",
        "      type: text\n",
        "      items:\n",
        "        - writes **docs**\n",
        "design:\n",
        "  template: compact\n",
    );

    #[test]
    fn markdown_fields_serialize_as_token_lists() {
        let doc = Document::from_yaml_str(SOURCE).expect("fixture parses");
        let yaml = to_yaml(&doc.cv, &doc.design);

        assert!(
            yaml.contains("kind: bold") && yaml.contains("text: engineer"),
            "summary bold token expected, got:\n{yaml}"
        );
        assert!(
            yaml.contains("kind: italic") && yaml.contains("text: rewrite"),
            "entry bullet italic token expected, got:\n{yaml}"
        );
        assert!(
            yaml.contains("kind: code") && yaml.contains("text: point"),
            "bullets-section code token expected, got:\n{yaml}"
        );
        assert!(
            yaml.contains("text: plain bullet"),
            "plain bullet must still be a text token, got:\n{yaml}"
        );
    }

    #[test]
    fn links_in_markdown_serialize_with_url() {
        let doc = Document::from_yaml_str(SOURCE).expect("fixture parses");
        let yaml = to_yaml(&doc.cv, &doc.design);
        assert!(
            yaml.contains("kind: link") && yaml.contains("url: https://acme.dev"),
            "link token with url expected, got:\n{yaml}"
        );
    }

    #[test]
    fn non_markdown_fields_stay_literal_strings() {
        let doc = Document::from_yaml_str(SOURCE).expect("fixture parses");
        let yaml = to_yaml(&doc.cv, &doc.design);
        assert!(
            yaml.contains("title: Work"),
            "titles stay plain, got:\n{yaml}"
        );
        assert!(
            yaml.contains("heading: Staff Engineer"),
            "headings stay plain"
        );
        assert!(yaml.contains("key: Rust"), "keys stay plain");
        assert!(yaml.contains("value: strong"), "values stay plain");
    }

    #[test]
    fn page_serializes_as_its_lowercase_name() {
        let doc = Document::parse_yaml("schema: 1\ncv:\n  name: X\ndesign:\n  page: letter\n")
            .expect("fixture parses");
        let yaml = to_yaml(&doc.cv, &doc.design);
        assert!(yaml.contains("page: letter"), "got:\n{yaml}");
    }

    /// The set of fields the tool parses into inline-Markdown tokens is a contract:
    /// `data.yaml` carries token lists for exactly the fields
    /// [`crate::model::Cv::markdown_texts`] names, so the Markdown URL check and the
    /// template-facing data can never cover different fields (ADR-0016, ADR-0033).
    #[test]
    fn the_tokenized_fields_are_exactly_the_ones_markdown_texts_lists() {
        let doc = Document::parse_yaml(SOURCE).expect("fixture parses");
        let paths: Vec<String> = doc
            .cv
            .markdown_texts()
            .iter()
            .map(|field| field.span_path.clone())
            .collect();
        assert_eq!(
            paths,
            [
                "cv.summary",
                "cv.sections[0].items[0].bullets[0]",
                "cv.sections[0].items[0].bullets[1]",
                "cv.sections[2].items[0]",
                "cv.sections[3].items[0]",
            ],
            "summary, entry bullets, and the items of the bullets and text sections"
        );
    }
}
