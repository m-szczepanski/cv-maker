//! M1 `cv init` tests (plan 1.3.3) and `--font-path` error path (spec §8, rule 13.9).

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

/// 1.3.3: the starter file parses back through the model without errors.
#[test]
fn starter_file_round_trips_through_the_model() {
    let dir = unique_dir("init-roundtrip");
    let (ok, out) = run_cv(&dir, &["init"]);
    assert!(ok, "cv init failed: {out}");

    let yaml = fs::read_to_string(dir.join("cv.yaml")).expect("starter written");
    let doc = cv::model::Document::from_yaml_str(&yaml)
        .expect("starter YAML must parse back through the model");
    assert_eq!(doc.schema, 1);
    assert_eq!(doc.cv.name, "Jane Doe");
    assert!(!doc.cv.sections.is_empty(), "starter must demo sections");
}

/// 1.3.3: second `init` without `--force` fails; with `--force` it overwrites.
#[test]
fn init_refuses_overwrite_without_force() {
    let dir = unique_dir("init-force");
    assert!(run_cv(&dir, &["init"]).0);

    let (ok, out) = run_cv(&dir, &["init"]);
    assert!(!ok, "second init without --force must fail");
    assert!(
        out.contains("--force"),
        "error must mention --force, got: {out}"
    );

    let (ok, out) = run_cv(&dir, &["init", "--force"]);
    assert!(ok, "init --force must overwrite, got: {out}");
    assert!(out.contains("wrote"), "success line: {out}");
}

/// `--template` with an unknown name fails with the available set listed.
#[test]
fn init_unknown_template_lists_available() {
    let dir = unique_dir("init-template");
    let (ok, out) = run_cv(&dir, &["init", "--template", "comapct"]);
    assert!(!ok, "unknown template name must fail");
    assert!(
        out.contains("did you mean 'compact'") || out.contains("compact"),
        "error should hint at the close match, got: {out}"
    );
}

/// A missing `--font-path` directory is an error, never silent fallback (spec §8).
#[test]
fn missing_font_path_dir_fails_the_build() {
    let dir = unique_dir("font-path");
    assert!(run_cv(&dir, &["init"]).0);

    let (ok, out) = run_cv(&dir, &["--font-path", "no-such-fonts", "build"]);
    assert!(!ok, "missing --font-path dir must fail, got: {out}");
    assert!(
        out.contains("no-such-fonts"),
        "error must name the missing dir, got: {out}"
    );
}
