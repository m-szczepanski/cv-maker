//! Design model: everything under the `design` key (spec §4).
//!
//! Every field has a Rust-defined default; a missing `design` section deserializes to
//! [`Design::default`].

use serde::{Deserialize, Serialize};

use crate::model::value::{Color, Length, LineHeight};

/// Presentation settings (spec §4 design block).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Design {
    /// Built-in template name or relative path to a custom template folder.
    pub template: String,

    /// Page size.
    pub page: Page,

    pub margins: Margins,
    pub fonts: Fonts,
    pub sizes: Sizes,
    pub spacing: Spacing,
    pub colors: Colors,

    /// Enable the one-page fitting loop (spec §7).
    pub fit_one_page: bool,

    pub fit_floor: FitFloor,

    /// Extra directories with .ttf/.otf files, relative to the YAML file.
    pub font_paths: Vec<String>,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            template: "compact".to_owned(),
            page: Page::A4,
            margins: Margins::default(),
            fonts: Fonts::default(),
            sizes: Sizes::default(),
            spacing: Spacing::default(),
            colors: Colors::default(),
            fit_one_page: true,
            fit_floor: FitFloor::default(),
            font_paths: Vec::new(),
        }
    }
}

/// Page size (spec §4: `a4 | letter`).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Page {
    #[default]
    A4,
    Letter,
}

/// Horizontal and vertical page margins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Margins {
    pub x: Length,
    pub y: Length,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            x: Length::new(1.5, crate::model::value::LengthUnit::Cm),
            y: Length::new(1.2, crate::model::value::LengthUnit::Cm),
        }
    }
}

/// Font families used by templates (spec §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Fonts {
    pub body: String,
    pub heading: String,
    pub mono: String,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            body: "Inter".to_owned(),
            heading: "Inter".to_owned(),
            mono: "JetBrains Mono".to_owned(),
        }
    }
}

/// Font sizes (spec §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Sizes {
    pub name: Length,
    pub heading: Length,
    pub body: Length,
    pub small: Length,
}

impl Default for Sizes {
    fn default() -> Self {
        Self {
            name: Length::new(20.0, crate::model::value::LengthUnit::Pt),
            heading: Length::new(11.0, crate::model::value::LengthUnit::Pt),
            body: Length::new(9.5, crate::model::value::LengthUnit::Pt),
            small: Length::new(8.5, crate::model::value::LengthUnit::Pt),
        }
    }
}

/// Vertical rhythm settings (spec §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Spacing {
    pub section_gap: Length,
    pub entry_gap: Length,
    /// Unitless line-height multiplier (e.g. `1.25`).
    pub line_height: LineHeight,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            section_gap: Length::new(8.0, crate::model::value::LengthUnit::Pt),
            entry_gap: Length::new(4.0, crate::model::value::LengthUnit::Pt),
            line_height: LineHeight(1.25),
        }
    }
}

/// Colors used by templates (spec §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Colors {
    pub accent: Color,
    pub text: Color,
    pub muted: Color,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            accent: Color::new(0x25, 0x63, 0xeb),
            text: Color::new(0x11, 0x11, 0x11),
            muted: Color::new(0x55, 0x55, 0x55),
        }
    }
}

/// Lower bounds for the one-page fit loop (spec §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct FitFloor {
    pub min_body_size: Length,
    pub min_scale: f64,
}

impl Default for FitFloor {
    fn default() -> Self {
        Self {
            min_body_size: Length::new(8.5, crate::model::value::LengthUnit::Pt),
            min_scale: 0.85,
        }
    }
}
