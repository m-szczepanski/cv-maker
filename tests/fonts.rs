//! `cv fonts` integration tests (plan 3.3): the real binary lists families with their
//! winning source, per ADR-0007 priority (font-path dirs → bundled → system).

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

/// Every spec §2 family must appear, labelled `bundled`.
#[test]
fn fonts_lists_bundled_families_with_source() {
    let dir = unique_dir("fonts-bundled");
    let (ok, out) = run_cv(&dir, &["fonts"]);
    assert!(ok, "cv fonts must succeed, got: {out}");
    for family in [
        "Inter",
        "JetBrains Mono",
        "IBM Plex Sans",
        "IBM Plex Mono",
        "Source Sans 3",
    ] {
        let line = out
            .lines()
            .find(|l| l.starts_with(family))
            .unwrap_or_else(|| panic!("`{family}` must be listed, got:\n{out}"));
        assert!(
            line.trim_end().ends_with("bundled"),
            "`{family}` must come from the bundle, got line: {line}"
        );
    }
}

/// A `--font-path` directory shadows a bundled family of the same name (ADR-0007),
/// and the listing marks the winner.
#[test]
fn font_path_dir_wins_and_is_marked_as_source() {
    let dir = unique_dir("fonts-shadow");
    let user_fonts = dir.join("user-fonts");
    fs::create_dir(&user_fonts).expect("font dir");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts/Inter-Regular.ttf"),
        user_fonts.join("inter.ttf"),
    )
    .expect("copy bundled Inter into the user dir");

    let dir_arg = user_fonts.to_str().expect("utf-8 path");
    let (ok, out) = run_cv(&dir, &["--font-path", dir_arg, "fonts"]);
    assert!(ok, "cv fonts with --font-path must succeed, got: {out}");
    let line = out
        .lines()
        .find(|l| l.starts_with("Inter"))
        .expect("Inter listed");
    assert!(
        line.trim_end().ends_with("font-path"),
        "the user copy must win over the bundle, got line: {line}"
    );
}

/// The listing is the command's essential output, not progress: `--quiet` keeps it.
#[test]
fn quiet_still_prints_the_listing() {
    let dir = unique_dir("fonts-quiet");
    let (ok, out) = run_cv(&dir, &["--quiet", "fonts"]);
    assert!(ok, "got: {out}");
    assert!(out.contains("Inter"), "listing must survive --quiet: {out}");
}

/// A missing `--font-path` directory fails cleanly and names it (spec §13.9).
#[test]
fn missing_font_path_dir_is_a_clean_error() {
    let dir = unique_dir("fonts-missing-dir");
    let (ok, out) = run_cv(&dir, &["--font-path", "no-such-fonts", "fonts"]);
    assert!(!ok, "a missing font dir must fail, got: {out}");
    assert!(
        out.contains("no-such-fonts"),
        "the error must name the directory, got: {out}"
    );
}
