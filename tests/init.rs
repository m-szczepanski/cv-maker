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

/// Plan 2.3 / ADR-0019: `init` also writes `cv.schema.json` next to the YAML, and it is
/// the same JSON `cv schema` prints.
#[test]
fn init_writes_schema_file_next_to_the_yaml() {
    let dir = unique_dir("init-schema");
    let (ok, out) = run_cv(&dir, &["init"]);
    assert!(ok, "cv init failed: {out}");

    let schema_path = dir.join("cv.schema.json");
    let schema = fs::read_to_string(&schema_path).expect("schema file written");
    assert!(
        schema.contains("\"$schema\""),
        "written schema must be JSON Schema, got:\n{}",
        &schema[..schema.len().min(300)]
    );

    let (ok, out) = run_cv(&dir, &["schema"]);
    assert!(ok, "cv schema failed: {out}");
    assert_eq!(
        out.trim(),
        schema.trim(),
        "`cv schema` and the written file agree"
    );
}

/// The `$schema` header in the starter points at the written schema file (plan 2.3).
#[test]
fn starter_schema_header_matches_written_file_name() {
    let dir = unique_dir("init-header");
    assert!(run_cv(&dir, &["init"]).0);
    let yaml = fs::read_to_string(dir.join("cv.yaml")).expect("starter written");
    let header = yaml.lines().next().expect("starter has a first line");
    assert_eq!(header, "# yaml-language-server: $schema=./cv.schema.json");
    assert!(
        dir.join("cv.schema.json").exists(),
        "header target must exist"
    );
}

/// An unwritable destination is a typed I/O error, not a panic (spec §13.9).
#[test]
fn init_fails_cleanly_when_the_yaml_destination_is_unwritable() {
    let dir = unique_dir("init-readonly");
    fs::create_dir(dir.join("sub")).expect("create sub dir");
    // Read-only directory: creating cv.yaml inside must fail.
    let mut perms = fs::metadata(dir.join("sub")).expect("meta").permissions();
    perms.set_readonly(true);
    fs::set_permissions(dir.join("sub"), perms).expect("chmod");

    let (ok, out) = run_cv(&dir, &["init", "--out", "sub/cv.yaml"]);
    assert!(!ok, "unwritable destination must fail, got: {out}");
    assert!(
        out.contains("could not write") || out.contains("sub/cv.yaml"),
        "error must name the destination, got: {out}"
    );
}

/// A schema write failure after the YAML write still surfaces as a typed I/O error
/// naming the schema file (spec §13.9).
#[test]
fn init_fails_cleanly_when_the_schema_destination_is_unwritable() {
    let dir = unique_dir("init-schema-blocked");
    // cv.schema.json already exists as a directory: the YAML write succeeds, the schema
    // write cannot.
    fs::create_dir(dir.join("cv.schema.json")).expect("create dir");

    let (ok, out) = run_cv(&dir, &["init"]);
    assert!(!ok, "blocked schema file must fail, got: {out}");
    assert!(
        out.contains("cv.schema.json"),
        "error must name the schema file, got: {out}"
    );
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
