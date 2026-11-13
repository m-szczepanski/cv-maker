//! Per-field design change tests (plan 3.6): every `design` field a template renders
//! must visibly change the PDF. Each test builds the base fixture and one variant that
//! differs in exactly one field, then asserts the PDF bytes differ and both are one
//! page (spec §12 M3 acceptance: "changing each design field in YAML visibly changes
//! the PDF").

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The fixture exercises every rendered design field: markdown code spans (mono font,
/// via the plan 3.1 show-set rule), dates and contact (small size, muted color), links
/// and headings (accent color, heading font/weights), all four section types (the
/// spacing fields), and paragraphs for the line height.
///
/// Every design value below is unique in this string, so each test replaces exactly
/// one line.
const BASE_YAML: &str = r##"schema: 1
cv:
  name: Jane Doe
  headline: Staff Software Engineer
  location: Berlin, Germany
  contact:
    email: jane@example.dev
    phone: "+49 000 000000"
    links:
      - { label: GitHub, url: "https://github.com/janedoe" }
  summary: >
    Backend engineer with **Rust** and *Go* focus, shipping `kafka` pipelines daily.
  sections:
    - title: Experience
      type: entries
      items:
        - heading: Staff Engineer
          org: Acme Corp
          location: Remote
          dates: 2021 – present
          bullets:
            - Led the *rewrite* to Rust
        - heading: Engineer
          org: Example Inc
          dates: 2016 – 2021
    - title: Skills
      type: key_values
      items:
        - { key: Languages, value: "Rust, Go" }
    - title: Highlights
      type: bullets
      items:
        - Shipped the tool
        - Mentored two juniors
    - title: About
      type: text
      items:
        - A short paragraph that exercises the line height and body settings.
design:
  template: compact
  page: a4
  margins: { x: 1.5cm, y: 1.2cm }
  fonts:
    body: Inter
    heading: Inter
    mono: JetBrains Mono
  sizes:
    name: 20pt
    heading: 11pt
    body: 9.5pt
    small: 8.5pt
  spacing:
    section_gap: 8pt
    entry_gap: 4pt
    line_height: 1.25
  colors:
    accent: "#2563eb"
    text: "#111111"
    muted: "#555555"
"##;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

/// Build `yaml` inside `dir`, returning the PDF bytes; asserts one page.
fn build(dir: &Path, out_name: &str, yaml: &str) -> Vec<u8> {
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(["build", "--output", out_name])
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pdf = dir.join(out_name);
    let pages = pdf_extract::extract_text_by_pages(&pdf).expect("text extraction");
    assert_eq!(pages.len(), 1, "{out_name} must be exactly one page");
    fs::read(&pdf).expect("read pdf")
}

/// Rebuild the base with `old` replaced by `new` and assert the PDF changed.
///
/// Also asserts the extracted text still contains the fixture's words: a design
/// change must re-render, not blank out the document.
fn assert_field_changes_the_pdf(test_name: &str, old: &str, new: &str) {
    let dir = unique_dir(test_name);
    let base = build(&dir, "base.pdf", BASE_YAML);
    let variant_yaml = BASE_YAML.replace(old, new);
    assert_ne!(
        variant_yaml, BASE_YAML,
        "fixture must contain the line under test ('{old}')"
    );
    let variant = build(&dir, "variant.pdf", &variant_yaml);
    assert_ne!(
        base, variant,
        "changing '{old}' to '{new}' must produce different PDF bytes"
    );

    let pages = pdf_extract::extract_text_by_pages(dir.join("variant.pdf")).expect("text");
    let text = pages.concat();
    assert!(text.contains("Jane Doe"), "variant must keep the name");
    assert!(text.contains("kafka"), "variant must keep the code span");
}

#[test]
fn the_same_fixture_builds_byte_identical_twice() {
    let dir = unique_dir("design-determinism");
    let first = build(&dir, "a.pdf", BASE_YAML);
    let second = build(&dir, "b.pdf", BASE_YAML);
    assert_eq!(first, second, "determinism floor for the diff tests");
    let pages = pdf_extract::extract_text_by_pages(dir.join("a.pdf")).expect("text");
    let text = pages.concat();
    assert!(
        text.contains("Jane Doe") && text.contains("EXPERIENCE"),
        "got: {text}"
    );
}

#[test]
fn changing_page_size_changes_the_pdf() {
    assert_field_changes_the_pdf("design-page", "page: a4", "page: letter");
}

#[test]
fn changing_the_x_margin_changes_the_pdf() {
    assert_field_changes_the_pdf("design-margin-x", "x: 1.5cm", "x: 2.5cm");
}

#[test]
fn changing_the_y_margin_changes_the_pdf() {
    assert_field_changes_the_pdf("design-margin-y", "y: 1.2cm", "y: 2cm");
}

#[test]
fn changing_the_body_font_changes_the_pdf() {
    assert_field_changes_the_pdf("design-font-body", "body: Inter", "body: Source Sans 3");
}

#[test]
fn changing_the_heading_font_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-font-heading",
        "heading: Inter",
        "heading: IBM Plex Sans",
    );
}

/// Regression for plan 3.1: `raw` must honor `design.fonts.mono`, not the built-in
/// DejaVu Sans Mono, and the change must be observable through the code span.
#[test]
fn changing_the_mono_font_changes_the_pdf() {
    let dir = unique_dir("design-font-mono");
    let base = build(&dir, "base.pdf", BASE_YAML);
    let variant = build(
        &dir,
        "variant.pdf",
        &BASE_YAML.replace("mono: JetBrains Mono", "mono: IBM Plex Mono"),
    );
    assert_ne!(
        base, variant,
        "the mono font must reach the rendered code span"
    );
}

#[test]
fn changing_the_name_size_changes_the_pdf() {
    assert_field_changes_the_pdf("design-size-name", "name: 20pt", "name: 24pt");
}

#[test]
fn changing_the_heading_size_changes_the_pdf() {
    assert_field_changes_the_pdf("design-size-heading", "heading: 11pt", "heading: 13pt");
}

#[test]
fn changing_the_body_size_changes_the_pdf() {
    assert_field_changes_the_pdf("design-size-body", "body: 9.5pt", "body: 10.5pt");
}

#[test]
fn changing_the_small_size_changes_the_pdf() {
    assert_field_changes_the_pdf("design-size-small", "small: 8.5pt", "small: 9.5pt");
}

#[test]
fn changing_the_section_gap_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-spacing-section",
        "section_gap: 8pt",
        "section_gap: 14pt",
    );
}

#[test]
fn changing_the_entry_gap_changes_the_pdf() {
    assert_field_changes_the_pdf("design-spacing-entry", "entry_gap: 4pt", "entry_gap: 8pt");
}

#[test]
fn changing_the_line_height_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-spacing-leading",
        "line_height: 1.25",
        "line_height: 1.6",
    );
}

#[test]
fn changing_the_accent_color_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-color-accent",
        "accent: \"#2563eb\"",
        "accent: \"#dc2626\"",
    );
}

#[test]
fn changing_the_text_color_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-color-text",
        "text: \"#111111\"",
        "text: \"#444444\"",
    );
}

#[test]
fn changing_the_muted_color_changes_the_pdf() {
    assert_field_changes_the_pdf(
        "design-color-muted",
        "muted: \"#555555\"",
        "muted: \"#999999\"",
    );
}
