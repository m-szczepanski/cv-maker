//! M4 fit-loop end-to-end tests (plan 4.1/4.5): run the real binary; the PDF page
//! count is ground truth, the success line must agree with it (spec §7).

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

/// A CV with `entries` × `bullets_per_entry` lines of content: `14 × 2` overflows
/// one A4 page at full sizes, `4 × 1` fits comfortably.
fn cv_yaml(entries: usize, bullets_per_entry: usize) -> String {
    let mut items = String::new();
    for i in 0..entries {
        items.push_str(&format!(
            "    - heading: Role {i}\n      org: Org {i}\n      bullets:\n"
        ));
        for b in 0..bullets_per_entry {
            items.push_str(&format!(
                "        - Achievement number {i}.{b} with realistic wording\n"
            ));
        }
    }
    format!(
        "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n  - title: Experience\n    type: entries\n    items:\n{items}\n"
    )
}

/// Plan 4.5: an overflowing CV fits after the loop shrinks, and the applied scale
/// is below 1.0 in the success line.
#[test]
fn overflowing_cv_fits_after_shrinking() {
    let dir = unique_dir("fit-shrink");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "the fitting build must succeed, got: {out}");

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text extraction");
    assert_eq!(
        pages.len(),
        1,
        "the fit loop must guarantee one page, log: {out}"
    );
    assert!(
        out.contains("1 page"),
        "the success line must agree with the PDF, got: {out}"
    );
    assert!(
        ["scale 0.", "spacing 0.", "margins 0."]
            .iter()
            .any(|prefix| out.contains(prefix)),
        "at least one applied scale must be below 1.00, got: {out}"
    );
}

/// Plan 4.5 hard requirement: a CV that already fits is never shrunk, at the
/// original (single-scale) line.
#[test]
fn short_cv_is_not_shrunk() {
    let dir = unique_dir("fit-untouched");
    fs::write(dir.join("cv.yaml"), cv_yaml(4, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("1 page") && out.contains("fit: scale 1.00"),
        "a first-fit CV must report scale 1.00 without tightening details, got: {out}"
    );
}

/// Plan 4.4: `--no-fit` disables the loop for the run — an overflowing CV is
/// written as-is, reports `scale 1.00`, and gets no floor warning.
#[test]
fn no_fit_flag_skips_the_loop() {
    let dir = unique_dir("fit-nofit-flag");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--no-fit", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("2 pages") && out.contains("fit: scale 1.00"),
        "disabled loop must write through the overflow, got: {out}"
    );
    assert!(
        !out.contains("content still overflows"),
        "no floor warning when the user opted out, got: {out}"
    );
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 2);
}

/// Plan 4.4: the design field does the same permanently (spec §4/§7: the loop
/// runs only when `design.fit_one_page` is true).
#[test]
fn design_fit_one_page_false_skips_the_loop() {
    let dir = unique_dir("fit-design-off");
    let yaml = cv_yaml(14, 2) + "\ndesign:\n  fit_one_page: false\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("2 pages") && out.contains("fit: scale 1.00"),
        "design-level opt-out must skip shrinking, got: {out}"
    );
    assert!(!out.contains("content still overflows"), "got: {out}");
}

/// Plan 4.3 / ADR-0026: impossible content still writes the PDF and exits 0,
/// with a warning stating the overflow and the suggestion; the success line
/// shows the exact floors.
#[test]
fn impossible_content_still_builds_with_a_warning_and_exit_zero() {
    let dir = unique_dir("fit-overflow");
    fs::write(dir.join("cv.yaml"), cv_yaml(60, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(
        ok,
        "overflow must not fail the build (ADR-0026), got: {out}"
    );
    assert!(
        out.contains("content still overflows at the fit floor"),
        "the warning must be printed, got: {out}"
    );
    assert!(
        out.contains("spill onto") && out.contains("relax design.fit_floor"),
        "the warning must quantify the spill and suggest a fix, got: {out}"
    );
    assert!(
        out.contains("fit: scale 0.89 spacing 0.60 margins 0.83"),
        "the line must report the exact floors for the default design, got: {out}"
    );

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert!(pages.len() > 1, "the overflowed PDF is still written");
}

/// The overflow warning is an essential (ADR-0026/0012): `--quiet` drops the
/// success line but keeps the warning.
#[test]
fn quiet_still_prints_the_overflow_warning() {
    let dir = unique_dir("fit-overflow-quiet");
    fs::write(dir.join("cv.yaml"), cv_yaml(60, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["--quiet", "build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("content still overflows at the fit floor"),
        "warning survives --quiet, got: {out}"
    );
    assert!(
        !out.contains("cv.pdf  "),
        "the success line must be suppressed under --quiet, got: {out}"
    );
}
