//! M1 end-to-end tests (plan 1.7.1–1.7.3): run the real binary via `CARGO_BIN_EXE_cv`.
//!
//! Each test works in an isolated temp directory with a unique name so parallel runs
//! don't collide.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

fn cv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_cv")
}

/// Run `cv` with arguments inside `dir`; returns (exit status success, combined stdout+stderr).
fn run_cv(dir: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(cv_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

/// 1.7.1: init → build → 1-page PDF with the expected selectable text.
#[test]
fn init_then_build_produces_one_page_pdf_with_expected_text() {
    let dir = unique_dir("build-examples");
    let (ok, out) = run_cv(&dir, &["init"]);
    assert!(ok, "cv init failed: {out}");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "cv build failed: {out}");

    let pdf_path = dir.join("cv.pdf");
    assert!(pdf_path.exists(), "cv.pdf must exist next to cv.yaml");

    let bytes = fs::read(&pdf_path).expect("read PDF");
    assert_eq!(&bytes[0..5], b"%PDF-", "output must be a PDF");

    // Exactly one page; pages are counted via text extraction (object streams are
    // compressed, so grepping for /Type /Page would find nothing).
    let pages = pdf_extract::extract_text_by_pages(&pdf_path).expect("text extraction");
    assert_eq!(pages.len(), 1, "starter CV must fit on exactly one page");

    // Selectable text: name, a section heading, and a bullet must be extractable.
    let text = pages.concat();
    assert!(
        text.contains("Jane Doe"),
        "name must be extractable, got: {text}"
    );
    assert!(text.contains("EXPERIENCE"), "heading must be extractable");
    assert!(
        text.contains("Cut p99 latency by 40%"),
        "bullet must be extractable, got: {text}"
    );
}

/// 1.7.2: two builds of the same input produce byte-identical PDFs.
#[test]
fn repeated_builds_are_byte_identical() {
    let dir = unique_dir("determinism");
    run_cv(&dir, &["init"]);
    assert!(run_cv(&dir, &["build", "--output", "a.pdf"]).0);
    assert!(run_cv(&dir, &["build", "--output", "b.pdf"]).0);

    let a = fs::read(dir.join("a.pdf")).expect("a.pdf");
    let b = fs::read(dir.join("b.pdf")).expect("b.pdf");
    assert_eq!(a, b, "same input must produce byte-identical PDFs");
}

/// 1.7.3: a failing build must not touch an existing PDF (atomic write, spec §3).
#[test]
fn failing_build_leaves_existing_pdf_untouched() {
    let dir = unique_dir("atomic-write");
    run_cv(&dir, &["init"]);
    assert!(run_cv(&dir, &["build"]).0);
    let original = fs::read(dir.join("cv.pdf")).expect("first build ok");

    // Break the YAML so rendering fails.
    let yaml_path = dir.join("cv.yaml");
    let broken = fs::read_to_string(&yaml_path).expect("read starter") + "\nsurprise_key: true\n";
    fs::write(&yaml_path, broken).expect("write broken yaml");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "broken YAML must fail the build, got: {out}");

    let after = fs::read(dir.join("cv.pdf")).expect("pdf still present");
    assert_eq!(
        original, after,
        "existing PDF must be untouched after failure"
    );

    // No temp files left behind.
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".cv-") || n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temp files must remain: {leftovers:?}"
    );
}

/// 1.6.4: template errors are reported as template errors, distinct from user errors.
#[test]
fn build_reports_template_errors_distinctly() {
    let dir = unique_dir("template-error");
    run_cv(&dir, &["init"]);
    // Break the design so the template gets invalid data it cannot render.
    let yaml_path = dir.join("cv.yaml");
    let broken = fs::read_to_string(&yaml_path)
        .expect("read starter")
        .replace("template: compact", "template: no-such-template");
    fs::write(&yaml_path, broken).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "unknown template must fail");
    assert!(
        out.contains("unknown template"),
        "error should say unknown template, got: {out}"
    );
}

/// `design.font_paths` entries resolve relative to the YAML file's directory (spec §4).
#[test]
fn design_font_paths_resolve_relative_to_the_yaml() {
    let dir = unique_dir("font-paths-rel");
    assert!(run_cv(&dir, &["init"]).0);

    // A font directory next to cv.yaml; the file content does not matter for discovery
    // (an unloadable file is simply not a resolvable family, never a crash).
    fs::create_dir(dir.join("fonts")).expect("create font dir");
    fs::write(dir.join("fonts/fake.ttf"), b"not a real font").expect("write fake font");

    let yaml_path = dir.join("cv.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read starter");
    let with_paths = yaml.replace("design:", "design:\n  font_paths:\n    - fonts");
    fs::write(&yaml_path, with_paths).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["check", "cv.yaml"]);
    assert!(
        ok,
        "an existing relative font dir must be accepted, got: {out}"
    );

    // A missing relative dir must fail and name the directory (spec §8).
    let yaml = fs::read_to_string(&yaml_path).expect("read starter");
    let broken = yaml.replace("- fonts", "- no-such-fonts");
    fs::write(&yaml_path, broken).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["check", "cv.yaml"]);
    assert!(!ok, "a missing design.font_paths dir must fail, got: {out}");
    assert!(
        out.contains("no-such-fonts"),
        "error must name the directory, got: {out}"
    );
}

/// Writing the PDF fails cleanly when the destination is unusable (e.g. `--output`
/// pointing at a directory): typed error, no panic, and no temp files left behind.
#[test]
fn build_fails_cleanly_when_output_is_a_directory() {
    let dir = unique_dir("output-is-dir");
    run_cv(&dir, &["init"]);
    fs::create_dir(dir.join("out-dir")).expect("create dir");

    let (ok, out) = run_cv(&dir, &["build", "--output", "out-dir"]);
    assert!(!ok, "output must not be a directory, got: {out}");
    assert!(
        out.contains("could not write"),
        "error should be a write error, got: {out}"
    );

    let leftovers: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".cv-") || n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "failed persist must clean up its temp file: {leftovers:?}"
    );
}

/// `--output` into a directory that does not exist: the temp file cannot be created, so
/// the build fails with a typed I/O error naming the directory (spec §3, §13.9).
#[test]
fn build_fails_cleanly_when_the_output_directory_is_missing() {
    let dir = unique_dir("output-missing-dir");
    run_cv(&dir, &["init"]);

    let (ok, out) = run_cv(&dir, &["build", "--output", "no-such-dir/cv.pdf"]);
    assert!(!ok, "missing output dir must fail, got: {out}");
    assert!(
        out.contains("no-such-dir"),
        "error should name the missing directory, got: {out}"
    );
}
