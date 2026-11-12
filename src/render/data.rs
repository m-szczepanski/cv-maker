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

use crate::model::{Cv, Design, Page};

/// The document handed to templates: `cv`, resolved `design`, and the `fit` scales.
#[derive(Debug, Serialize)]
pub struct DataFile<'a> {
    pub cv: &'a Cv,
    pub design: DesignData<'a>,
    pub fit: FitData,
}

impl<'a> DataFile<'a> {
    pub fn new(cv: &'a Cv, design: &'a Design) -> Self {
        Self {
            cv,
            design: DesignData::from(design),
            fit: FitData::default(),
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

/// Current fit-loop scale factors (spec §7). Templates multiply their own sizes/spacings
/// by these; all 1.0 until the fit loop (M4) changes them.
#[derive(Debug, Serialize)]
pub struct FitData {
    pub font_scale: f64,
    pub spacing_scale: f64,
    pub margin_scale: f64,
}

impl Default for FitData {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            spacing_scale: 1.0,
            margin_scale: 1.0,
        }
    }
}

/// Lowercase page name used in `data.yaml` and in error messages.
pub fn page_name(page: Page) -> String {
    match page {
        Page::A4 => "a4".to_owned(),
        Page::Letter => "letter".to_owned(),
    }
}

/// Serialize the data file to a YAML string (deterministic: struct field order).
pub fn to_yaml(cv: &Cv, design: &Design) -> crate::error::Result<String> {
    serde_yaml_ng::to_string(&DataFile::new(cv, design)).map_err(|e| {
        crate::error::Error::Unsupported(format!("could not serialize data file: {e}"))
    })
}
