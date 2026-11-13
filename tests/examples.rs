//! M2 example and schema tests (plan 2.8): every `examples/` file validates, and the
//! derived JSON Schema is pinned by a snapshot so accidental changes are reviewable.
//!
//! Examples "validate against the schema" through the typed model itself: the schema is
//! derived (`schemars`) from the same structs the parser uses, so a successful parse
//! `Document::from_yaml_str` plus semantic checks (`cv check`) is equivalent to running a
//! JSON-Schema validator on the converted document — without adding a validator crate
//! (docs/decisions.md ADR-0020).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_cv")
}

fn run(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(cv_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), stdout, stderr)
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Every example passes `cv check` (typed parse + semantic checks) without warnings.
#[test]
fn every_example_passes_check() {
    let dir = examples_dir();
    for file in ["minimal.yaml", "full.yaml"] {
        let (ok, stdout, stderr) = run(&dir, &["check", file]);
        assert!(ok, "`cv check {file}` failed:\n{stderr}");
        assert!(
            stdout.contains(&format!("{file} is valid")),
            "success line expected for {file}, got: {stdout}"
        );
    }
}

/// Every example builds into a one-page PDF with selectable text.
#[test]
fn every_example_builds_a_one_page_pdf() {
    let dir = examples_dir();
    for (file, name) in [("minimal.yaml", "Jane Doe"), ("full.yaml", "Jane Doe")] {
        let out = format!("{file}.test.pdf");
        let (ok, _, stderr) = run(&dir, &["build", file, "--output", &out]);
        assert!(ok, "`cv build {file}` failed:\n{stderr}");

        let pdf_path = dir.join(&out);
        let pages = pdf_extract::extract_text_by_pages(&pdf_path).expect("text extraction works");
        assert_eq!(pages.len(), 1, "{file} must fit on one page");
        let text = pages.concat();
        assert!(
            text.contains(name),
            "{file} must contain the name, got: {text}"
        );
        fs::remove_file(&pdf_path).expect("clean up test pdf");
    }
}

/// The `full` example demonstrates the Markdown subset in its rendered text: markers in
/// Markdown-enabled fields are gone from the extractable text (bold/italic are styling,
/// code/link labels stay). Headline markers stay literal by design (ADR-0016).
#[test]
fn markdown_markers_do_not_leak_into_the_pdf_text() {
    let dir = examples_dir();
    let (ok, _, stderr) = run(
        &dir,
        &["build", "full.yaml", "--output", "full.md.test.pdf"],
    );
    assert!(ok, "build failed:\n{stderr}");
    let pdf_path = dir.join("full.md.test.pdf");
    let text = pdf_extract::extract_text_by_pages(&pdf_path)
        .expect("extract")
        .concat();
    // Link syntax never appears in rendered text (only the label does).
    assert!(
        !text.contains("](https"),
        "link syntax must not appear, got: {text}"
    );
    assert!(
        text.contains("zero-downtime"),
        "marked-up words must still render, got: {text}"
    );
    assert!(
        text.contains("engineering practice"),
        "link labels must render, got: {text}"
    );
    fs::remove_file(&pdf_path).expect("clean up test pdf");
}

/// The derived JSON Schema is pinned byte-for-byte (plan 2.8, ADR-0019).
#[test]
fn schema_output_matches_snapshot() {
    let schema = cv::commands::schema::schema_json().expect("schema serializes");
    insta::assert_snapshot!(schema);
}
