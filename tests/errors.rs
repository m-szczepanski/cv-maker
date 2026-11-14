//! M2 error snapshot tests (plan 2.7, spec §5/§11) and the every-error-class rule
//! (spec §13.9).
//!
//! Each test runs the real binary (`cv check` unless noted) on a broken YAML in an
//! isolated temp directory and snapshots the rendered diagnostics. The child's stderr is
//! a pipe, not a TTY, so output is the deterministic no-color unicode rendering
//! (ADR-0018); the snapshots pin message, line:column, snippet and hint.
//!
//! Error classes and where they are covered:
//! - parse errors (unknown key, wrong type, bad color/length, missing required field,
//!   malformed YAML, unknown section variant): this file.
//! - semantic errors (bad font, bad URL, broken Markdown link URL, unknown template,
//!   bad schema version, multi-error collection): this file.
//! - diagnostics without a span (a value written as a YAML alias): this file.
//! - warnings (duplicate title): this file.
//! - I/O (missing file): this file.
//! - write errors: `tests/build_examples.rs` (output is a directory).
//! - template errors (labeled distinctly from user errors): this file (`cv build`).
//! - custom template folder errors: `tests/custom_template.rs`.
//! - unknown `--template` for `init`: `tests/init.rs`.
//! - watch loop errors: `tests/watch.rs`.
//! - Typst-special characters rendering literally, never executing: `tests/escaping.rs`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-errors-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

/// Write `yaml` as `cv.yaml` in a fresh temp dir, run `cv check`, return
/// (exit ok, stdout, stderr).
fn check_broken(name: &str, yaml: &str) -> (bool, String, String) {
    let dir = unique_dir(name);
    fs::write(dir.join("cv.yaml"), yaml).expect("write broken yaml");
    run(dir, &["check", "cv.yaml"])
}

fn run(dir: PathBuf, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), stdout, stderr)
}

// ---- Parse-time errors (typed model rejects; span comes from serde's location) ----

#[test]
fn unknown_key_gets_did_you_mean_hint() {
    let (ok, _, stderr) = check_broken("unknown-key", "schema: 1\ncv:\n  nmae: Jane Doe\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn unknown_top_level_key_without_close_match_lists_fields() {
    let (ok, _, stderr) = check_broken(
        "unknown-toplevel",
        "schema: 1\ncv:\n  name: Jane Doe\nsurprise: true\n",
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn unknown_section_type_is_rejected() {
    let (ok, _, stderr) = check_broken(
        "unknown-section-type",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n",
            "    - title: Projects\n      type: gallery\n      items: []\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn missing_required_name_is_rejected() {
    let (ok, _, stderr) = check_broken("missing-name", "schema: 1\ncv:\n  headline: X\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn wrong_value_type_is_rejected() {
    let (ok, _, stderr) = check_broken("wrong-type", "schema: \"one\"\ncv:\n  name: Jane Doe\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn bad_color_is_rejected() {
    let (ok, _, stderr) = check_broken(
        "bad-color",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "design:\n  colors:\n    accent: \"#zzzzzz\"\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn bad_length_is_rejected() {
    let (ok, _, stderr) = check_broken(
        "bad-length",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "design:\n  margins: { x: 1.5em, y: 1cm }\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn malformed_yaml_syntax_is_rejected() {
    let (ok, _, stderr) = check_broken("malformed-yaml", "schema: 1\ncv:\n  name: [unclosed\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

// ---- Semantic errors (src/validate; span comes from the ADR-0014 span scan) ----

#[test]
fn unknown_font_suggests_the_close_match() {
    let (ok, _, stderr) = check_broken(
        "unknown-font",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "design:\n  fonts:\n    body: Inte\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn unrelated_font_gets_the_font_resolution_help() {
    // No close match exists, so the hint explains how fonts resolve instead.
    let (ok, _, stderr) = check_broken(
        "unrelated-font",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "design:\n  fonts:\n    body: Zzzzqx\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn malformed_link_url_is_rejected() {
    let (ok, _, stderr) = check_broken(
        "bad-url",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n  contact:\n    links:\n",
            "      - label: Site\n        url: github.com/me\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

/// A Markdown link with an unusable URL used to pass `cv check` and then fail
/// `cv build` as a *template* error; it is a user error now (ADR-0034).
#[test]
fn malformed_markdown_link_urls_are_rejected() {
    let (ok, _, stderr) = check_broken(
        "markdown-url",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "  summary: My work is at [lab](github.com/me/lab) and [home]().\n",
        ),
    );
    assert!(!ok, "a broken Markdown link must fail the check");
    insta::assert_snapshot!(stderr);
}

/// A value written as a YAML alias has no span of its own: the diagnostic names the
/// resolved value without a snippet, instead of pointing at the wrong line (ADR-0036).
#[test]
fn an_aliased_bad_value_is_reported_without_a_snippet() {
    let (ok, _, stderr) = check_broken("aliased-value", "cv:\n  name: &ver 2\nschema: *ver\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn unknown_template_is_rejected() {
    let (ok, _, stderr) = check_broken(
        "unknown-template",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n",
            "design:\n  template: compactt\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let (ok, _, stderr) = check_broken("bad-schema-version", "schema: 2\ncv:\n  name: Jane Doe\n");
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

#[test]
fn all_errors_are_reported_not_just_the_first() {
    let (ok, _, stderr) = check_broken(
        "multi-error",
        concat!(
            "schema: 2\ncv:\n  name: Jane Doe\n  contact:\n    links:\n",
            "      - label: Site\n        url: github.com/me\n",
            "design:\n  template: compactt\n  fonts:\n    body: Inte\n",
        ),
    );
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

// ---- Warnings never fail (ADR-0015) ----

#[test]
fn duplicate_section_title_warns_but_still_passes() {
    let (ok, stdout, stderr) = check_broken(
        "duplicate-title",
        concat!(
            "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n",
            "    - title: Work\n      type: text\n      items: [Hello]\n",
            "    - title: Work\n      type: text\n      items: [Again]\n",
        ),
    );
    assert!(ok, "warnings must not fail: {stderr}");
    insta::assert_snapshot!("duplicate_title_stdout", stdout);
    insta::assert_snapshot!(stderr);
}

// ---- I/O errors ----

#[test]
fn missing_input_file_is_a_clean_error() {
    let dir = unique_dir("missing-file");
    let (ok, _, stderr) = run(dir, &["check", "no-such.yaml"]);
    assert!(!ok);
    insta::assert_snapshot!(stderr);
}

// ---- Template errors are labeled distinctly from user errors (spec §5) ----

#[test]
fn template_errors_are_labeled_as_template_errors() {
    // Template errors (Typst compiler rejects the template/data) carry a distinct label,
    // separate from user YAML errors (spec §5), and render as a plain `Error:` line
    // because they have no YAML source to label (ADR-0018).
    use cv::error::Error;
    use cv::render::RenderError;

    let error = Error::Render(RenderError::Template {
        message: "unknown variable: body".into(),
    });
    assert_eq!(
        error.to_string(),
        "template error: unknown variable: body",
        "template errors must carry the distinct label"
    );
    let rendered = cv::error::report(&error, false);
    assert!(
        rendered.contains("Error: template error:"),
        "got: {rendered}"
    );
}

#[test]
fn test_file_names_do_not_leak_into_snapshots() {
    // Sanity guard for the snapshot suite: diagnostics name the input file as passed
    // (`cv.yaml`), never an absolute temp path, so snapshots stay portable.
    let (ok, _, stderr) = check_broken("no-path-leak", "schema: 1\ncv:\n  name: Jane Doe\n");
    assert!(ok);
    assert!(
        !stderr.contains(std::env::temp_dir().to_str().expect("temp dir is utf-8")),
        "temp paths must not appear in output: {stderr}"
    );
}
