//! Unit tests for the data model and strict value parsing (plan 1.1.5).

use crate::commands::init::STARTER_YAML;
use crate::error::Result;
use crate::model::value::{Color, Length, LengthUnit, LineHeight};
use crate::model::{Design, Document};

/// The `cv init` starter, derived from the spec §4 example. It demonstrates every section
/// type and every design knob, so it doubles as the model's end-to-end parse fixture and
/// cannot drift from what users actually get.
#[test]
fn starter_yaml_parses_end_to_end() -> Result<()> {
    let doc = Document::from_yaml_str(STARTER_YAML)?;

    assert_eq!(doc.schema, 1);
    assert_eq!(doc.cv.name, "Jane Doe");
    assert_eq!(doc.cv.headline.as_deref(), Some("Staff Software Engineer"));

    let contact = doc.cv.contact.as_ref().expect("contact present");
    assert_eq!(contact.email.as_deref(), Some("jane@example.dev"));
    let links = contact.links.as_ref().expect("links present");
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].label, "GitHub");

    assert_eq!(doc.cv.sections.len(), 3);

    match &doc.cv.sections[0].body {
        crate::model::SectionBody::Entries { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].heading, "Staff Engineer");
            assert_eq!(items[0].org.as_deref(), Some("Acme Corp"));
            assert_eq!(items[0].bullets.as_ref().map(|b| b.len()), Some(2));
        }
        other => panic!("expected entries, got {other:?}"),
    }
    match &doc.cv.sections[1].body {
        crate::model::SectionBody::KeyValues { items } => {
            assert_eq!(items[0].key, "Languages");
            assert_eq!(items[0].value, "Rust, Go, Python, TypeScript");
        }
        other => panic!("expected key_values, got {other:?}"),
    }

    // Design values round-trip through the strict value types.
    let design = &doc.design;
    assert_eq!(design.template, "compact");
    assert_eq!(design.page, crate::model::Page::A4);
    assert_eq!(
        design.margins.x,
        Length::new(1.5, LengthUnit::Cm),
        "margins.x should parse 1.5cm"
    );
    assert_eq!(
        design.sizes.body,
        Length::new(9.5, LengthUnit::Pt),
        "sizes.body should parse 9.5pt"
    );
    assert_eq!(design.spacing.line_height.0, 1.25);
    assert_eq!(design.colors.accent, Color::new(0x25, 0x63, 0xeb));

    Ok(())
}

#[test]
fn missing_design_deserializes_to_defaults() -> Result<()> {
    let yaml = "schema: 1\ncv:\n  name: Jane Doe\n";
    let doc = Document::from_yaml_str(yaml)?;
    assert_eq!(doc.design, Design::default());
    assert_eq!(doc.design.template, "compact");
    assert_eq!(doc.design.page, crate::model::Page::A4);
    assert!(doc.design.fit_one_page);
    Ok(())
}

#[test]
fn partial_design_fills_missing_fields_with_defaults() -> Result<()> {
    let yaml = "schema: 1\ncv:\n  name: Jane Doe\ndesign:\n  page: letter\n";
    let doc = Document::from_yaml_str(yaml)?;
    assert_eq!(doc.design.page, crate::model::Page::Letter);
    assert_eq!(doc.design.template, "compact", "unset fields keep defaults");
    Ok(())
}

#[test]
fn unknown_top_level_key_is_rejected() {
    let yaml = "schema: 1\ncv:\n  name: Jane Doe\nsurprise: true\n";
    let err = Document::from_yaml_str(yaml).expect_err("unknown key must fail");
    assert!(
        err.to_string().contains("unknown field") || err.to_string().contains("surprise"),
        "error should mention the unknown field, got: {err}"
    );
}

#[test]
fn unknown_key_inside_section_item_is_rejected() {
    let yaml = concat!(
        "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n",
        "    - title: Experience\n      type: entries\n",
        "      items:\n        - heading: Staff Engineer\n          orgs: Acme\n"
    );
    let err = Document::from_yaml_str(yaml).expect_err("unknown item key must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("orgs") || msg.contains("unknown field"),
        "error should mention the unknown item key, got: {msg}"
    );
}

#[test]
fn wrong_section_type_is_rejected() {
    let yaml = concat!(
        "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n",
        "    - title: Projects\n      type: gallery\n      items: []\n"
    );
    let err = Document::from_yaml_str(yaml).expect_err("closed section type set");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("gallery") || msg.contains("type") || msg.contains("unknown variant"),
        "error should identify the bad type, got: {err}"
    );
}

#[test]
fn length_parsing_covers_units_and_rejects_bad_input() {
    assert_eq!(
        serde_yaml_ng::from_str::<Length>("\"1.5cm\"").unwrap(),
        Length::new(1.5, LengthUnit::Cm)
    );
    assert_eq!(
        serde_yaml_ng::from_str::<Length>("\"9.5pt\"").unwrap(),
        Length::new(9.5, LengthUnit::Pt)
    );
    assert_eq!(
        serde_yaml_ng::from_str::<Length>("\"2mm\"").unwrap(),
        Length::new(2.0, LengthUnit::Mm)
    );
    assert_eq!(
        serde_yaml_ng::from_str::<Length>("\"0.5in\"").unwrap(),
        Length::new(0.5, LengthUnit::In)
    );

    // Missing unit / unknown unit / garbage must fail.
    assert!(serde_yaml_ng::from_str::<Length>("\"9.5\"").is_err());
    assert!(serde_yaml_ng::from_str::<Length>("\"9.5em\"").is_err());
    assert!(serde_yaml_ng::from_str::<Length>("\"abc\"").is_err());
}

#[test]
fn length_to_pt_covers_every_unit() {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(Length::new(10.0, LengthUnit::Pt).to_pt(), 10.0));
    assert!(near(Length::new(1.0, LengthUnit::In).to_pt(), 72.0));
    assert!(near(Length::new(2.54, LengthUnit::Cm).to_pt(), 72.0));
    assert!(near(Length::new(25.4, LengthUnit::Mm).to_pt(), 72.0));
}

#[test]
fn color_parsing_accepts_3_and_6_digit_hex() {
    assert_eq!(
        serde_yaml_ng::from_str::<Color>("\"#2563eb\"").unwrap(),
        Color::new(0x25, 0x63, 0xeb)
    );
    assert_eq!(
        serde_yaml_ng::from_str::<Color>("\"#25b\"").unwrap(),
        Color::new(0x22, 0x55, 0xbb)
    );
    assert!(serde_yaml_ng::from_str::<Color>("\"2563eb\"").is_err());
    assert!(serde_yaml_ng::from_str::<Color>("\"#2563\"").is_err());
    assert!(serde_yaml_ng::from_str::<Color>("\"#zzzzzz\"").is_err());

    // Multi-byte characters must be a clean error, not a panic (byte length differs from
    // character count; regression test for a mid-character byte-slice panic).
    assert!(serde_yaml_ng::from_str::<Color>("\"#€abc\"").is_err());
    assert!(serde_yaml_ng::from_str::<Color>("\"#ab€c\"").is_err());
}

#[test]
fn line_height_must_be_a_positive_number() {
    assert_eq!(
        serde_yaml_ng::from_str::<LineHeight>("1.25").unwrap(),
        LineHeight(1.25)
    );
    // Matches the schema's exclusiveMinimum: 0.
    assert!(serde_yaml_ng::from_str::<LineHeight>("0").is_err());
    assert!(serde_yaml_ng::from_str::<LineHeight>("-1.25").is_err());
    assert!(serde_yaml_ng::from_str::<LineHeight>("\"abc\"").is_err());
}

#[test]
fn lengths_display_in_their_unit_without_trailing_zeros() {
    assert_eq!(Length::new(1.5, LengthUnit::Cm).to_string(), "1.5cm");
    assert_eq!(Length::new(20.0, LengthUnit::Pt).to_string(), "20pt");
    assert_eq!(Length::new(2.0, LengthUnit::Mm).to_string(), "2mm");
    assert_eq!(Length::new(0.5, LengthUnit::In).to_string(), "0.5in");
}

#[test]
fn length_type_errors_name_the_expected_shape() {
    // A non-scalar value fails with the visitor's `expecting` text.
    let err = serde_yaml_ng::from_str::<Length>("[1]").expect_err("sequence is not a length");
    let msg = err.to_string();
    assert!(
        msg.contains("a length like \"1.5cm\""),
        "error should explain lengths, got: {msg}"
    );
}

#[test]
fn length_value_errors_name_the_problem() {
    // A unit with no number.
    let err = serde_yaml_ng::from_str::<Length>("\"pt\"").expect_err("unit without a number");
    assert!(
        err.to_string().contains("a number followed by a unit"),
        "got: {err}"
    );

    // A number too large for f64 (400 digits overflow to infinity).
    let huge = format!("{}cm", "9".repeat(400));
    let err = serde_yaml_ng::from_str::<Length>(&huge).expect_err("must be finite");
    assert!(err.to_string().contains("a finite number"), "got: {err}");
}

#[test]
fn colors_display_as_lowercase_hex() {
    assert_eq!(Color::new(0x25, 0x63, 0xeb).to_string(), "#2563eb");
    assert_eq!(Color::new(0x00, 0x0a, 0xff).to_string(), "#000aff");
}

#[test]
fn color_type_errors_name_the_expected_shape() {
    let err = serde_yaml_ng::from_str::<Color>("[1]").expect_err("sequence is not a color");
    let msg = err.to_string();
    assert!(
        msg.contains("a hex color like \"#2563eb\""),
        "error should explain colors, got: {msg}"
    );
}

#[test]
fn empty_sections_list_is_valid() -> Result<()> {
    let yaml = "schema: 1\ncv:\n  name: Jane Doe\n  sections: []\n";
    let doc = Document::from_yaml_str(yaml)?;
    assert_eq!(doc.cv.sections.len(), 0);
    Ok(())
}

#[test]
fn missing_required_name_is_rejected() {
    let yaml = "schema: 1\ncv:\n  headline: X\n";
    assert!(Document::from_yaml_str(yaml).is_err());
}

#[test]
fn entry_item_with_no_heading_is_rejected() {
    let yaml = concat!(
        "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n",
        "    - title: Experience\n      type: entries\n      items:\n",
        "        - org: Acme\n"
    );
    assert!(Document::from_yaml_str(yaml).is_err());
}
