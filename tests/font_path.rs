//! `--font-path` end-to-end (plan 3.4): a user font directory is discoverable,
//! selectable and shadowing — proven with the `CvTest Sans` fixture (ADR-0022), a
//! renamed clone of Inter that exists nowhere else, so a passing resolution can only
//! come from the user directory and a failing one from its absence.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts")
}

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

/// The starter YAML with the body font switched to the fixture family.
fn init_with_test_font(dir: &Path) {
    assert!(run_cv(dir, &["init"]).0, "cv init must succeed");
    let yaml_path = dir.join("cv.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read starter");
    let with_font = yaml.replace("body: Inter", "body: CvTest Sans");
    assert_ne!(yaml, with_font, "starter must carry the body font line");
    fs::write(&yaml_path, with_font).expect("write yaml");
}

/// Without the directory the family is unknown, and `cv check` says so (spec §5).
#[test]
fn fixture_font_is_unknown_without_a_font_path() {
    let dir = unique_dir("fontpath-absent");
    init_with_test_font(&dir);

    let (ok, out) = run_cv(&dir, &["check"]);
    assert!(!ok, "a font that exists nowhere else must fail validation");
    assert!(
        out.contains("unknown font \"CvTest Sans\""),
        "the error must name the family, got: {out}"
    );
}

/// With `--font-path` the font validates, is listed as `font-path`, and the build
/// embeds it — the user directory font is what actually rendered (spec §8).
#[test]
fn font_path_flag_makes_a_user_font_selectable_and_listed() {
    let dir = unique_dir("fontpath-flag");
    init_with_test_font(&dir);
    let fixture = fixture_dir();
    let fixture = fixture.to_str().expect("utf-8 fixture path");

    let (ok, out) = run_cv(&dir, &["--font-path", fixture, "fonts"]);
    assert!(ok, "cv fonts with --font-path must succeed, got: {out}");
    assert!(
        out.contains("CvTest Sans")
            && out
                .lines()
                .any(|l| l.starts_with("CvTest Sans") && l.trim_end().ends_with("font-path")),
        "the fixture must be listed with its source, got: {out}"
    );

    let (ok, out) = run_cv(
        &dir,
        &["--font-path", fixture, "build", "--output", "out.pdf"],
    );
    assert!(ok, "build with the user font must succeed, got: {out}");

    let pdf = dir.join("out.pdf");
    let pages = pdf_extract::extract_text_by_pages(&pdf).expect("text extraction");
    assert_eq!(pages.len(), 1, "one page");
    assert!(pages.concat().contains("Jane Doe"), "selectable text");

    let bytes = fs::read(&pdf).expect("read pdf");
    assert!(
        bytes
            .windows(b"CvTestSans".len())
            .any(|w| w == b"CvTestSans"),
        "the PDF must embed the user-directory font subset"
    );
}

/// `design.font_paths` (relative to the YAML file, spec §4) selects the font without
/// any global flag.
#[test]
fn design_font_paths_select_a_relative_user_font_directory() {
    let dir = unique_dir("fontpath-design");
    init_with_test_font(&dir);

    fs::create_dir(dir.join("myfonts")).expect("font dir");
    fs::copy(
        fixture_dir().join("CvTestSans-Regular.ttf"),
        dir.join("myfonts/CvTestSans-Regular.ttf"),
    )
    .expect("copy fixture next to the yaml");

    let yaml_path = dir.join("cv.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read starter");
    let with_paths = yaml.replace("design:\n", "design:\n  font_paths:\n    - myfonts\n");
    assert_ne!(yaml, with_paths);
    fs::write(&yaml_path, with_paths).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "out.pdf"]);
    assert!(
        ok,
        "design.font_paths relative to the YAML must be used for resolution, got: {out}"
    );
    let bytes = fs::read(dir.join("out.pdf")).expect("read pdf");
    assert!(
        bytes
            .windows(b"CvTestSans".len())
            .any(|w| w == b"CvTestSans"),
        "the font embedded from the relative dir must render"
    );
}
