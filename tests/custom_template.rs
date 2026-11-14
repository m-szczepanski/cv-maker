//! Custom-template tests (spec §6, ADR-0027/0028).
//!
//! The acceptance scenario: a user copies a built-in template folder, edits it, and
//! uses it via `design.template: ./my-template`. Every user-facing error class a
//! custom template can produce (folder, manifest, entry point, and a PDF the exporter
//! refuses) plus the manifest consistency warnings get a test here (spec §13.9).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

/// Make a file unreadable for the current user (POSIX permission model; the same
/// assumption the init disk-fault tests make), and readable again after `f` so
/// the temp directory stays deletable.
fn unreadable<R>(path: &Path, f: impl FnOnce() -> R) -> R {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
    let result = f();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("chmod back");
    result
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
        format!("schema: 1\ncv:\n  name: Jane Doe\ndesign:\n  template: {template_ref}\n"),
    )
    .expect("write cv.yaml");
}

/// Create a template folder with exactly the given sources (key = virtual path).
fn make_folder(dir: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create sub dir");
        }
        fs::write(full, content).expect("write template file");
    }
}

fn manifest(supported: &[&str]) -> String {
    let mut yaml = String::from(
        "name: my-template\ndescription: A custom template.\nschema_version: 1\nsupported:\n",
    );
    for group in supported {
        yaml.push_str(&format!("  - {group}\n"));
    }
    yaml
}

const SIMPLE_TYP: &str = "#let d = yaml(\"data.yaml\")\n#d.cv.name\n";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create copy dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// Plan 5.7 acceptance: copy a built-in, tweak it, build through the path reference.
#[test]
fn copied_and_edited_builtin_renders_through_the_path_reference() {
    let dir = unique_dir("custom-e2e");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/compact"),
        &dir.join("my-template"),
    );
    fs::write(dir.join("my-template/template.typ"), {
        let mut source =
            fs::read_to_string(dir.join("my-template/template.typ")).expect("read copy");
        source.push_str("\n#block[custom-template-marker]\n");
        source
    })
    .expect("tweak copy");
    write_cv(&dir, "./my-template");

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "custom template build failed: {out}");

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("extract text");
    assert_eq!(pages.len(), 1, "starter-less CV must fit one page");
    let text = pages.concat();
    assert!(text.contains("Jane Doe"), "name renders, got: {text}");
    assert!(
        text.contains("custom-template-marker"),
        "the user's edit must render"
    );
}

/// The copied built-in's nested helper must load too: break the folder's own
/// `lib/inline.typ`; the build must reflect the broken copy, proving it is the
/// helper the entry point imports (not the binary's embedded one).
#[test]
fn custom_folder_nested_typ_sources_are_available_to_the_entry_point() {
    let dir = unique_dir("custom-nested");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/compact"),
        &dir.join("my-template"),
    );
    fs::write(
        dir.join("my-template/lib/inline.typ"),
        "#let inline(x) = none\n",
    )
    .expect("break the copied helper");
    fs::write(
        dir.join("cv.yaml"),
        "schema: 1\ncv:\n  name: Jane Doe\n  summary: staff **engineer**\ndesign:\n  template: ./my-template\n",
    )
    .unwrap();

    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "a helper returning none still renders: {out}");
    let text = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("extract")
        .concat();
    assert!(
        !text.contains("engineer"),
        "the broken custom helper must swallow the summary — proving the folder's \
         own lib/inline.typ was loaded: {text}"
    );
}

#[test]
fn missing_template_folder_is_a_clear_error_naming_the_reference() {
    let dir = unique_dir("custom-missing-folder");
    write_cv(&dir, "./nope");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "missing folder must fail");
    assert!(
        out.contains("nope") && out.contains("not found"),
        "error names the reference and the problem: {out}"
    );
}

#[test]
fn missing_manifest_yaml_is_a_clear_error() {
    let dir = unique_dir("custom-no-manifest");
    make_folder(&dir.join("my-template"), &[("template.typ", SIMPLE_TYP)]);
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("manifest.yaml") && out.contains("template.typ"),
        "error must name the missing manifest and both required files: {out}"
    );
}

#[test]
fn missing_entry_point_is_a_clear_error() {
    let dir = unique_dir("custom-no-entry");
    make_folder(
        &dir.join("my-template"),
        &[
            ("manifest.yaml", &manifest(&[])),
            ("template.txt", "not typst"),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("template.typ"),
        "error must name the missing entry point: {out}"
    );
}

#[test]
fn unparseable_manifest_is_reported_with_the_folder_context() {
    let dir = unique_dir("custom-bad-manifest");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", "name: [unclosed\n"),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("invalid manifest.yaml"),
        "the error must label the manifest: {out}"
    );
}

#[test]
fn unknown_manifest_keys_are_rejected() {
    let dir = unique_dir("custom-manifest-unknown-key");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            (
                "manifest.yaml",
                "name: t\ndescription: d\nschema_version: 1\nsuppoted: []\n",
            ),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("suppoted"),
        "the typo'd manifest key must be quoted back: {out}"
    );
}

#[test]
fn manifest_targeting_another_schema_version_is_rejected() {
    let dir = unique_dir("custom-manifest-version");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            (
                "manifest.yaml",
                "name: t\ndescription: d\nschema_version: 2\n",
            ),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("targets schema version") && out.contains("uses version 1"),
        "the error must state both versions: {out}"
    );
}

#[test]
fn manifest_with_empty_name_is_rejected() {
    let dir = unique_dir("custom-manifest-name");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            (
                "manifest.yaml",
                "name: \"\"\ndescription: d\nschema_version: 1\n",
            ),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("non-empty `name`"),
        "the error must name the missing field: {out}"
    );
}

#[test]
fn non_utf8_template_source_is_a_clear_error() {
    let dir = unique_dir("custom-non-utf8");
    make_folder(
        &dir.join("my-template"),
        &[("manifest.yaml", &manifest(&[]))],
    );
    fs::write(dir.join("my-template/template.typ"), [0xffu8, 0xfe, 0x00])
        .expect("write invalid bytes");
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok);
    assert!(
        out.contains("UTF-8") && out.contains("template.typ"),
        "the error must name the file and the encoding: {out}"
    );
}

/// The manifest is read through the same decoder: an undecodable `manifest.yaml`
/// names itself, not the entry point.
#[test]
fn non_utf8_manifest_is_a_clear_error() {
    let dir = unique_dir("custom-non-utf8-manifest");
    fs::create_dir(dir.join("my-template")).expect("create folder");
    fs::write(dir.join("my-template/template.typ"), "#let a = 1\n").expect("write source");
    fs::write(dir.join("my-template/manifest.yaml"), [0xffu8, 0x28, 0x61])
        .expect("write invalid manifest bytes");
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "an undecodable manifest must fail: {out}");
    assert!(
        out.contains("UTF-8") && out.contains("manifest.yaml"),
        "the error must name the file and the encoding: {out}"
    );
}

/// A template that compiles but whose PDF the exporter refuses is reported as an
/// export error, not a user error, and writes nothing (spec §3, §5).
#[test]
fn a_refused_pdf_export_is_reported_as_an_export_error() {
    let dir = unique_dir("custom-export-fail");
    make_folder(
        &dir.join("my-template"),
        &[
            (
                "template.typ",
                "#pdf.attach(\"data.yaml\", mime-type: \"not a mime type\")\n#let d = yaml(\"data.yaml\")\n#d.cv.name\n",
            ),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build", "--output", "out.pdf"]);
    assert!(!ok, "a refused export must fail the build: {out}");
    assert!(
        out.contains("PDF export failed") && out.contains("mime type"),
        "the exporter's reason reaches the user: {out}"
    );
    assert!(
        !dir.join("out.pdf").exists(),
        "a refused export leaves no PDF behind"
    );
}

/// Unreadable folder files surface as typed I/O errors naming the file (same
/// class as the `--font-path` and init disk-fault tests; POSIX permission model).
#[test]
fn an_unreadable_manifest_reports_a_typed_io_error() {
    let dir = unique_dir("custom-unreadable-manifest");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = unreadable(&dir.join("my-template/manifest.yaml"), || {
        run_cv(&dir, &["build"])
    });
    assert!(!ok, "an unreadable manifest must fail: {out}");
    assert!(
        out.contains("could not read") && out.contains("manifest.yaml"),
        "a typed I/O error naming the file: {out}"
    );
}

#[test]
fn an_unreadable_template_source_reports_a_typed_io_error() {
    let dir = unique_dir("custom-unreadable-source");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = unreadable(&dir.join("my-template/template.typ"), || {
        run_cv(&dir, &["build"])
    });
    assert!(!ok, "an unreadable source must fail: {out}");
    assert!(
        out.contains("could not read") && out.contains("template.typ"),
        "a typed I/O error naming the file: {out}"
    );
}

/// An unlistable subdirectory is the same fault class one level down: the folder
/// walk reports `Error::Io` naming it instead of skipping the sources silently.
#[test]
fn an_unreadable_template_subdirectory_reports_a_typed_io_error() {
    let dir = unique_dir("custom-unreadable-subdir");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", &manifest(&[])),
            ("lib/inline.typ", "#let inline(x) = x\n"),
        ],
    );
    write_cv(&dir, "./my-template");

    use std::os::unix::fs::PermissionsExt;
    let lib = dir.join("my-template/lib");
    fs::set_permissions(&lib, fs::Permissions::from_mode(0o000)).expect("chmod 000 dir");
    let result = run_cv(&dir, &["build"]);
    fs::set_permissions(&lib, fs::Permissions::from_mode(0o755)).expect("chmod back");

    let (ok, out) = result;
    assert!(!ok, "an unlistable directory must fail: {out}");
    assert!(
        out.contains("could not read") && out.contains("lib"),
        "a typed I/O error naming the directory: {out}"
    );
}

/// Purity (spec §6, ADR-0027): the compiler only sees the folder's virtual sources,
/// so an import reaching outside the folder fails as a template error, and no
/// package/network resolver exists to reach the registry either.
#[test]
fn custom_templates_cannot_read_outside_their_folder_or_fetch_packages() {
    let dir = unique_dir("custom-purity");
    fs::write(dir.join("evil.typ"), "#evil-should-not-load\n").unwrap();
    make_folder(
        &dir.join("my-template"),
        &[
            ("manifest.yaml", &manifest(&[])),
            (
                "template.typ",
                "#import \"../evil.typ\": *\n#let d = yaml(\"data.yaml\")\n#d.cv.name\n",
            ),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "outside import must fail");
    assert!(
        out.contains("template error"),
        "labeled as a template error, got: {out}"
    );

    // And a package import has no resolver to download it.
    make_folder(
        &dir.join("pkg-template"),
        &[
            ("manifest.yaml", &manifest(&[])),
            (
                "template.typ",
                "#import \"@preview/cv-purity-probe:0.0.1\": *\n#let d = yaml(\"data.yaml\")\n#d.cv.name\n",
            ),
        ],
    );
    fs::write(
        dir.join("cv.yaml"),
        "schema: 1\ncv:\n  name: Jane Doe\ndesign:\n  template: ./pkg-template\n",
    )
    .unwrap();
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(!ok, "package import must fail without a resolver");
    assert!(
        out.contains("template error"),
        "also a template error, got: {out}"
    );
}

/// ADR-0028: declaring a group the sources never read warns (exit stays 0, ADR-0015).
#[test]
fn declared_but_unread_groups_warn_in_check() {
    let dir = unique_dir("custom-overpromise");
    make_folder(
        &dir.join("my-template"),
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", &manifest(&["page"])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["check"]);
    assert!(ok, "warnings never fail: {out}");
    assert!(
        out.contains("declares support for 'page'") && out.contains("never read design.page"),
        "the over-promise must be explained: {out}"
    );
    assert!(
        out.contains("is valid"),
        "check still reports the file valid: {out}"
    );
}

/// ADR-0028 the other way: reading a group the manifest does not declare warns.
#[test]
fn read_but_undeclared_groups_warn_in_check() {
    let dir = unique_dir("custom-underpromise");
    make_folder(
        &dir.join("my-template"),
        &[
            (
                "template.typ",
                "#let d = yaml(\"data.yaml\")\n#d.cv.name #d.design.page\n",
            ),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["check"]);
    assert!(ok, "warnings never fail: {out}");
    assert!(
        out.contains("reads design.page") && out.contains("does not declare 'page'"),
        "the under-declaration must be explained: {out}"
    );
}

/// A matching manifest produces no consistency noise at all.
#[test]
fn consistent_custom_template_checks_clean() {
    let dir = unique_dir("custom-consistent");
    make_folder(
        &dir.join("my-template"),
        &[
            (
                "template.typ",
                "#let d = yaml(\"data.yaml\")\n#d.cv.name #d.design.page\n",
            ),
            ("manifest.yaml", &manifest(&["page"])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["check"]);
    assert!(ok, "clean check: {out}");
    assert!(
        out.contains("(0 warnings)"),
        "the consistency check must stay silent on a matching manifest: {out}"
    );
}

/// An absolute path reference works like a relative one (ADR-0027).
#[test]
fn absolute_template_paths_are_accepted() {
    let dir = unique_dir("custom-absolute");
    let folder = dir.join("my-template");
    make_folder(
        &folder,
        &[
            ("template.typ", SIMPLE_TYP),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, &folder.display().to_string());
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "absolute reference must build: {out}");
    assert!(dir.join("cv.pdf").exists());
}

/// The fit loop reaches custom templates through the same `sys.inputs.fit` contract:
/// the overflowing CV shape from `tests/fit.rs` still shrinks to one page through a
/// copied built-in (spec §7 "templates multiply their own sizes and spacings").
#[test]
fn the_fit_loop_drives_custom_templates_too() {
    let dir = unique_dir("custom-fit");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/compact"),
        &dir.join("my-template"),
    );
    let mut items = String::new();
    for i in 0..14 {
        items.push_str(&format!(
            "    - heading: Role {i}\n      org: Org {i}\n      bullets:\n        - Achievement number {i}.1 with realistic wording\n        - Achievement number {i}.2 with realistic wording\n"
        ));
    }
    fs::write(
        dir.join("cv.yaml"),
        format!(
            "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n  - title: Experience\n    type: entries\n    items:\n{items}\ndesign:\n  template: ./my-template\n"
        ),
    )
    .expect("write long cv");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "long CV build: {out}");
    assert!(
        out.contains("1 page"),
        "the copy must fit after shrinking: {out}"
    );
    assert!(
        ["scale 0.", "spacing 0.", "margins 0."]
            .iter()
            .any(|prefix| out.contains(prefix)),
        "at least one applied scale must be below 1.00, got: {out}"
    );
}

/// Typst compiler warnings from a custom template surface on stderr (the reporting
/// point kept open since M2, now reachable exactly as planned). A `show page` rule
/// is the smallest stable warning in pinned Typst 0.15.1 ("`show page` is not
/// supported and has no effect", `typst-eval/src/rules.rs` — verified against the
/// pinned sources, not recalled).
#[test]
fn typst_warnings_from_custom_templates_reach_the_user() {
    let dir = unique_dir("custom-warn");
    make_folder(
        &dir.join("my-template"),
        &[
            (
                "template.typ",
                "#show page: it => it\n#let d = yaml(\"data.yaml\")\n#d.cv.name\n",
            ),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "warnings never fail: {out}");
    assert!(
        out.contains("warning:") && out.contains("`show page` is not supported"),
        "the Typst warning must be reported: {out}"
    );
}

/// ADR-0032: the warning is an essential (ADR-0012), so `--quiet` drops the result
/// line but keeps it.
#[test]
fn quiet_still_reports_typst_warnings() {
    let dir = unique_dir("custom-warn-quiet");
    make_folder(
        &dir.join("my-template"),
        &[
            (
                "template.typ",
                "#show page: it => it\n#let d = yaml(\"data.yaml\")\n#d.cv.name\n",
            ),
            ("manifest.yaml", &manifest(&[])),
        ],
    );
    write_cv(&dir, "./my-template");
    let (ok, out) = run_cv(&dir, &["--quiet", "build"]);
    assert!(ok, "warnings never fail: {out}");
    assert!(
        out.contains("warning:") && out.contains("`show page` is not supported"),
        "the Typst warning must survive --quiet: {out}"
    );
    assert!(
        !out.contains(".pdf"),
        "the success line is progress and must be gone: {out}"
    );
}
