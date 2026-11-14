//! Strictly parsed value types (plan 1.1.3): lengths, colors, unitless ratios.
//!
//! These deserialize from user-facing strings (`1.5cm`, `#2563eb`) and serialize to plain
//! strings again for YAML round-trips. For the template-facing `data.yaml`, lengths and
//! colors are additionally exposed in a structured, template-friendly form via
//! [`Length::to_data`] / [`Color::to_hex`] (see `src/render/data.rs`).

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A length with an explicit unit, parsed from strings like `1.5cm`, `9.5pt`, `2mm`, `0.5in`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: LengthUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Pt,
    Cm,
    Mm,
    In,
}

impl Length {
    pub fn new(value: f64, unit: LengthUnit) -> Self {
        Self { value, unit }
    }

    /// Human-readable form, e.g. `1.5cm` (used in error messages and YAML round-trips).
    pub fn to_string_form(&self) -> String {
        format!("{}{}", fmt_f64(self.value), self.unit.suffix())
    }

    /// Structured form for the template-facing data file: `{"value": 1.5, "unit": "cm"}`.
    pub fn to_data(&self) -> LengthData {
        LengthData {
            value: self.value,
            unit: self.unit.suffix().to_owned(),
        }
    }

    /// The length in PostScript points (1 pt = 1/72 in, 1 in = 2.54 cm exactly).
    /// Used by the fit loop to compare floors across units (spec §7).
    pub fn to_pt(&self) -> f64 {
        let pt_per_unit = match self.unit {
            LengthUnit::Pt => 1.0,
            LengthUnit::Mm => 72.0 / 25.4,
            LengthUnit::Cm => 720.0 / 25.4,
            LengthUnit::In => 72.0,
        };
        self.value * pt_per_unit
    }
}

/// Structured length as seen by templates in `data.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LengthData {
    pub value: f64,
    pub unit: String,
}

impl LengthUnit {
    pub fn suffix(self) -> &'static str {
        match self {
            LengthUnit::Pt => "pt",
            LengthUnit::Cm => "cm",
            LengthUnit::Mm => "mm",
            LengthUnit::In => "in",
        }
    }

    fn from_suffix(s: &str) -> Option<Self> {
        match s {
            "pt" => Some(LengthUnit::Pt),
            "cm" => Some(LengthUnit::Cm),
            "mm" => Some(LengthUnit::Mm),
            "in" => Some(LengthUnit::In),
            _ => None,
        }
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_form())
    }
}

/// Format a float without a trailing `.0` (`1.5` stays `1.5`, `20.0` becomes `20`).
fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl<'de> Deserialize<'de> for Length {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct LengthVisitor;
        impl Visitor<'_> for LengthVisitor {
            type Value = Length;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    f,
                    "a length like \"1.5cm\", \"9.5pt\", \"2mm\", or \"0.5in\""
                )
            }

            fn visit_str<E: de::Error>(self, s: &str) -> std::result::Result<Length, E> {
                parse_length(s).map_err(de::Error::custom)
            }
        }
        deserializer.deserialize_str(LengthVisitor)
    }
}

impl Serialize for Length {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string_form())
    }
}

impl schemars::JsonSchema for Length {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Length".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schema_from_json(serde_json::json!({
            "type": "string",
            "pattern": "^[0-9]+(\\.[0-9]+)?(pt|cm|mm|in)$",
            "examples": ["1.5cm", "9.5pt"]
        }))
    }
}

/// Build a JSON Schema fragment from a fixed JSON literal. `try_from` validates the
/// value; the literals in this file are schema documents by construction, so a failure
/// is a bug in this file and never user input (rule 14).
fn schema_from_json(value: serde_json::Value) -> schemars::Schema {
    schemars::Schema::try_from(value).expect("a hand-written schema literal is valid")
}

/// Parse `"<number><unit>"` with optional whitespace between number and unit.
fn parse_length(s: &str) -> Result<Length> {
    let trimmed = s.trim();
    let split = trimmed
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| {
            Error::invalid_value(trimmed, "a length with a unit, e.g. \"1.5cm\" or \"9.5pt\"")
        })?;
    let (num, unit) = trimmed.split_at(split);
    let unit = LengthUnit::from_suffix(unit)
        .ok_or_else(|| Error::invalid_value(unit, "one of the units pt, cm, mm, in"))?;
    let value: f64 = num.trim().parse().map_err(|_| {
        Error::invalid_value(trimmed, "a number followed by a unit, e.g. \"1.5cm\"")
    })?;
    if !value.is_finite() {
        return Err(Error::invalid_value(trimmed, "a finite number"));
    }
    Ok(Length { value, unit })
}

/// An RGB color parsed from `#rgb` or `#rrggbb` hex strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rrggbb` lowercase hex form (used in error messages and YAML round-trips).
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ColorVisitor;
        impl Visitor<'_> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a hex color like \"#2563eb\" or \"#25b\"")
            }

            fn visit_str<E: de::Error>(self, s: &str) -> std::result::Result<Color, E> {
                parse_color(s).map_err(de::Error::custom)
            }
        }
        deserializer.deserialize_str(ColorVisitor)
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl schemars::JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schema_from_json(serde_json::json!({
            "type": "string",
            "pattern": "^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$",
            "examples": ["#2563eb"]
        }))
    }
}

/// Parse one ASCII hex digit, case-insensitive.
fn hex_digit(d: u8) -> Option<u8> {
    match d {
        b'0'..=b'9' => Some(d - b'0'),
        b'a'..=b'f' => Some(d - b'a' + 10),
        b'A'..=b'F' => Some(d - b'A' + 10),
        _ => None,
    }
}

fn parse_color(s: &str) -> Result<Color> {
    let hex = s.strip_prefix('#').ok_or_else(|| {
        Error::invalid_value(s, "a hex color starting with '#', e.g. \"#2563eb\"")
    })?;
    if !hex.is_ascii() {
        return Err(Error::invalid_value(s, "hex digits (0-9, a-f)"));
    }
    let digits = hex.as_bytes();

    let pair = |bytes: &[u8]| -> Result<u8> {
        let hi =
            hex_digit(bytes[0]).ok_or_else(|| Error::invalid_value(s, "hex digits (0-9, a-f)"))?;
        let lo =
            hex_digit(bytes[1]).ok_or_else(|| Error::invalid_value(s, "hex digits (0-9, a-f)"))?;
        Ok((hi << 4) | lo)
    };

    match digits.len() {
        3 => {
            let mut rgb = [0u8; 3];
            for (i, d) in digits.iter().enumerate() {
                let v = hex_digit(*d)
                    .ok_or_else(|| Error::invalid_value(s, "hex digits (0-9, a-f)"))?;
                rgb[i] = v * 17; // scale 0xf to 0xff
            }
            Ok(Color::new(rgb[0], rgb[1], rgb[2]))
        }
        6 => Ok(Color::new(
            pair(&digits[0..2])?,
            pair(&digits[2..4])?,
            pair(&digits[4..6])?,
        )),
        _ => Err(Error::invalid_value(
            s,
            "a hex color like \"#2563eb\" (6 digits) or \"#25b\" (3 digits)",
        )),
    }
}

/// A unitless multiplier parsed from a plain YAML number (`1.25`).
///
/// Distinct from [`Length`] because it carries no unit; used for `line_height` and,
/// in the fit loop, for scale factors. Must be positive (the JSON Schema says
/// `exclusiveMinimum: 0`, so the parser enforces it too).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineHeight(pub f64);

impl<'de> Deserialize<'de> for LineHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = f64::deserialize(deserializer)?;
        if !v.is_finite() || v <= 0.0 {
            return Err(de::Error::custom("must be a positive number"));
        }
        Ok(LineHeight(v))
    }
}

impl Serialize for LineHeight {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl schemars::JsonSchema for LineHeight {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "UnitlessNumber".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schema_from_json(serde_json::json!({
            "type": "number",
            "exclusiveMinimum": 0.0
        }))
    }
}
