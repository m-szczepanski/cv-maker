//! Multi-page pagination tests for the newer built-ins (spec §6 house rules,
//! ADR-0044/45/46/47): content that overflows one page must flow, the rail tint /
//! column layout must keep working per page, and the GDPR clause must land on the
//! last page. The two-column templates are the risk: everything lives in one grid
//! row, so a broken row split would strand most content on one page.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

fn run_cv(dir: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

/// Four sections of six entries each, plus a rail-friendly skills section: far more
/// than one page, spread over enough sections that whole-section keeps (ADR-0046)
/// break between sections rather than mid-list.
fn long_cv(template_ref: &str) -> String {
    let mut sections = String::new();
    for s in 0..4 {
        sections.push_str(&format!(
            "    - title: Experience {s}\n      type: entries\n      items:\n"
        ));
        for i in 0..6 {
            sections.push_str(&format!(
                "        - heading: Role {s}.{i}\n          org: Org {s}.{i}\n          dates: 20{s}{i:0>1} – 2025\n          bullets:\n            - Achievement number {s}.{i} with realistic wording that fills the line\n"
            ));
        }
    }
    format!(
        "schema: 1\ncv:\n  name: Jane Doe\n  headline: Engineer\n  contact:\n    email: jane@example.dev\n  sections:\n{sections}    - title: Skills\n      type: key_values\n      items:\n        - {{ key: Languages, value: \"Rust, Go\" }}\ndesign:\n  template: {template_ref}\n  pages: 2\n"
    )
}

/// What every multi-page render of a house template must look like, page by page.
fn assert_flows_well(pdf: &Path, label: &str) {
    let pages = pdf_extract::extract_text_by_pages(pdf).expect("text extraction");
    assert!(
        pages.len() >= 2,
        "{label}: expected multiple pages, got {}",
        pages.len()
    );
    assert!(
        pages[0].contains("Role 0.0"),
        "{label}: the first role must be on page 1: {:?}",
        pages[0]
    );
    for (i, page) in pages.iter().enumerate() {
        let lines = page.lines().filter(|l| !l.trim().is_empty()).count();
        assert!(
            lines >= 5,
            "{label}: page {} holds only {lines} lines",
            i + 1
        );
    }
    let last = pages.last().expect("a last page");
    assert!(
        last.contains("GDPR") && last.find("GDPR").expect("index") > last.len() / 2,
        "{label}: the consent clause must sit at the bottom of the last page"
    );
    assert!(
        !pages[..pages.len() - 1].iter().any(|p| p.contains("GDPR")),
        "{label}: the consent clause appears once, on the last page only"
    );
}

#[test]
fn sidebar_flows_multiline_content_and_pins_gdpr_to_the_last_page() {
    let dir = unique_dir("pag-sidebar");
    fs::write(dir.join("cv.yaml"), long_cv("sidebar")).expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "sidebar multi-page build failed: {out}");
    assert_flows_well(&dir.join("cv.pdf"), "sidebar");
}

#[test]
fn modern_flows_multiline_content_and_pins_gdpr_to_the_last_page() {
    let dir = unique_dir("pag-modern");
    fs::write(dir.join("cv.yaml"), long_cv("modern")).expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "modern multi-page build failed: {out}");
    assert_flows_well(&dir.join("cv.pdf"), "modern");
}

#[test]
fn diy_in_sidebar_layout_flows_multiline_content() {
    let dir = unique_dir("pag-diy");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
    assert!(ok, "extract failed: {out}");
    let cfg = dir.join("diy/config.typ");
    let original = fs::read_to_string(&cfg).expect("read config");
    fs::write(
        &cfg,
        original.replace(r#"layout: "single""#, r#"layout: "sidebar-left""#),
    )
    .expect("write config");
    fs::write(dir.join("cv.yaml"), long_cv("./diy")).expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "diy sidebar multi-page build failed: {out}");
    assert_flows_well(&dir.join("cv.pdf"), "diy sidebar");
}
