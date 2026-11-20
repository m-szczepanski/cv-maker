//! `cv templates --extract` tests (spec §6 custom templates, M5 acceptance): the
//! DIY path — materialize a built-in folder, edit it, build through the path
//! reference. Every user-facing failure of the command gets a test (spec §13.9).

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

fn write_cv(dir: &Path, template_ref: &str) {
    fs::write(
        dir.join("cv.yaml"),
        format!(
            "schema: 1\ncv:\n  name: Jane Doe\n  summary: staff **engineer**\ndesign:\n  template: {template_ref}\n"
        ),
    )
    .expect("write cv.yaml");
}

#[test]
fn extract_writes_a_complete_editable_folder_and_prints_the_usage_hint() {
    let dir = unique_dir("extract-compact");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "compact"]);
    assert!(ok, "extract must succeed: {out}");
    assert!(
        dir.join("compact/template.typ").is_file()
            && dir.join("compact/manifest.yaml").is_file()
            && dir.join("compact/lib/inline.typ").is_file(),
        "the whole embedded folder must land on disk: {:?}",
        dir.join("compact")
    );
    assert!(
        out.contains("design.template: ./compact"),
        "the hint must name the path reference: {out}"
    );
    assert!(
        !out.contains("design.template: ./.compact") && !out.contains("design.template: ../"),
        "the hint path must be exactly `./compact`: {out}"
    );
}

#[test]
fn the_extracted_folder_is_a_working_custom_template_after_an_edit() {
    let dir = unique_dir("extract-then-build");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "compact"]);
    assert!(ok, "extract must succeed: {out}");
    fs::write(dir.join("compact/template.typ"), {
        let mut source =
            fs::read_to_string(dir.join("compact/template.typ")).expect("read extracted");
        source.push_str("\n#block[extracted-marker]\n");
        source
    })
    .expect("tweak extracted folder");
    write_cv(&dir, "./compact");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "build with the extracted folder failed: {out}");
    let text = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("extract")
        .concat();
    assert!(
        text.contains("extracted-marker"),
        "user edit must render: {text}"
    );
}

#[test]
fn extract_out_writes_to_the_requested_directory() {
    let dir = unique_dir("extract-out");
    let (ok, out) = run_cv(
        &dir,
        &["templates", "--extract", "modern", "--out", "my-modern"],
    );
    assert!(ok, "extract --out must succeed: {out}");
    assert!(dir.join("my-modern/template.typ").is_file(), "{out}");
    assert!(
        !dir.join("modern").exists(),
        "--out must replace the default directory"
    );
    assert!(
        out.contains("design.template: ./my-modern"),
        "the hint must use the given directory: {out}"
    );
}

#[test]
fn extract_refuses_a_non_empty_directory_until_force_is_passed() {
    let dir = unique_dir("extract-refuses");
    fs::create_dir_all(dir.join("sidebar")).expect("create target");
    fs::write(dir.join("sidebar/keep.txt"), "mine").expect("fill target");

    let (ok, out) = run_cv(&dir, &["templates", "--extract", "sidebar"]);
    assert!(!ok, "a non-empty target must be refused");
    assert!(
        out.contains("already exists and is not empty") && out.contains("--force"),
        "the error must name the state and the escape: {out}"
    );
    assert!(
        dir.join("sidebar/keep.txt").exists(),
        "a refused extract writes nothing"
    );

    let (ok, out) = run_cv(&dir, &["templates", "--extract", "sidebar", "--force"]);
    assert!(ok, "--force must extract: {out}");
    assert!(dir.join("sidebar/template.typ").is_file(), "{out}");
    assert!(
        dir.join("sidebar/keep.txt").exists(),
        "--force overwrites the template's files, not the user's other files"
    );
}

#[test]
fn extracting_an_unknown_name_suggests_the_closest_builtin() {
    let dir = unique_dir("extract-unknown");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "comapct"]);
    assert!(!ok, "unknown built-in must fail");
    assert!(
        out.contains("no such built-in template") && out.contains("did you mean 'compact'"),
        "the error must reject the name and suggest the fix: {out}"
    );
}

#[test]
fn the_extracted_diy_folder_honours_its_config_knobs() {
    // The knob file is the DIY surface: editing it must change the rendered bytes.
    let dir = unique_dir("extract-diy-knobs");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
    assert!(ok, "extract diy must succeed: {out}");
    fs::write(
        dir.join("cv.yaml"),
        "schema: 1\ncv:\n  name: Jane Doe\n  summary: staff **engineer**\n  contact:\n    email: jane@example.dev\n  sections:\n    - title: Skills\n      type: key_values\n      items:\n        - { key: Languages, value: Rust }\ndesign:\n  template: ./diy\n",
    )
    .expect("write cv.yaml");
    let build = |name: &str| {
        let (ok, out) = run_cv(&dir, &["build", "--output", name]);
        assert!(ok, "diy build failed: {out}");
        fs::read(dir.join(name)).expect("read pdf")
    };

    let base = build("base.pdf");
    let cfg = dir.join("diy/config.typ");
    let original = fs::read_to_string(&cfg).expect("read config");
    for (old, new, label) in [
        (
            r#"layout: "single""#,
            r#"layout: "sidebar-left""#,
            "sidebar.pdf",
        ),
        (
            r#"heading-case: "upper""#,
            r#"heading-case: "title""#,
            "title.pdf",
        ),
        (
            r#"header-style: "none""#,
            r#"header-style: "accent""#,
            "band.pdf",
        ),
    ] {
        fs::write(&cfg, original.replace(old, new)).expect("write config");
        let variant = build(label);
        assert_ne!(base, variant, "{old} -> {new} must change the PDF bytes");
    }
    fs::write(&cfg, original).expect("restore config");
    assert_eq!(base, build("base.pdf"), "same input, same bytes (rule 29)")
}

#[test]
fn an_invalid_diy_knob_value_fails_the_build_naming_the_knob() {
    // The knobs are asserted in the template, not silently defaulted: a typo must
    // surface as a template error quoting the offending key (spec §5: template
    // errors are labeled as such).
    let dir = unique_dir("extract-diy-typo");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
    assert!(ok, "extract must succeed: {out}");
    write_cv(&dir, "./diy");
    let cfg = dir.join("diy/config.typ");
    let original = fs::read_to_string(&cfg).expect("read config");
    fs::write(
        &cfg,
        original.replace(r#"heading-case: "upper""#, r#"heading-case: "capital""#),
    )
    .expect("write config");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "a typo'd knob value must fail the build");
    assert!(
        out.contains("template error") && out.contains("cfg.heading-case"),
        "the failure must be labeled and name the knob: {out}"
    );
}

#[test]
fn a_non_boolean_justify_knob_fails_the_build_naming_the_knob() {
    // Every knob is guarded, booleans included: `justify: "yes"` must name the
    // knob instead of surfacing Typst's generic parameter-type error (ADR-0048).
    let dir = unique_dir("extract-diy-justify-typo");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
    assert!(ok, "extract must succeed: {out}");
    write_cv(&dir, "./diy");
    let cfg = dir.join("diy/config.typ");
    let original = fs::read_to_string(&cfg).expect("read config");
    fs::write(&cfg, original.replace("justify: false", "justify: \"yes\"")).expect("write config");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "a non-boolean justify must fail the build");
    assert!(
        out.contains("template error") && out.contains("cfg.justify"),
        "the failure must be labeled and name the knob: {out}"
    );
}

#[test]
fn a_typo_in_the_layout_knob_fails_the_build_instead_of_defaulting_to_a_rail() {
    // `layout` gates the biggest visual choice; a near-miss like "sidebar" must
    // not silently render the left rail (ADR-0048: unknown knob values are loud).
    let dir = unique_dir("extract-diy-layout-typo");
    let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
    assert!(ok, "extract must succeed: {out}");
    write_cv(&dir, "./diy");
    let cfg = dir.join("diy/config.typ");
    let original = fs::read_to_string(&cfg).expect("read config");
    fs::write(
        &cfg,
        original.replace(r#"layout: "single""#, r#"layout: "sidebar""#),
    )
    .expect("write config");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "a typo'd layout value must fail the build");
    assert!(
        out.contains("template error") && out.contains("cfg.layout"),
        "the failure must be labeled and name the knob: {out}"
    );
}
