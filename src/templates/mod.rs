//! Template loading (spec §6): built-in templates from the embedded assets and custom
//! template folders from disk (plan 5.3, ADR-0027).
//!
//! `design.template` is either a built-in name (`compact`, `classic`) or a path
//! reference — anything containing a `/` or `\` or starting with `.` (`./my-template`,
//! `../shared/tpl`, `/abs/tpl`). Path references resolve against the CV file's
//! directory, the same rule as `design.font_paths` (ADR-0027).
//!
//! Purity (spec §6: no network, no reads outside the folder) is enforcement by
//! construction: a template reaches the compiler only as the set of virtual sources
//! this module loads from its folder plus the tool's `data.yaml`; there is no file
//! system resolver and no package resolver in `src/render/`.

pub mod manifest;

use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry, include_dir};

use crate::error::{Error, Result};

/// All built-in template folders, embedded at compile time (spec §2, §10).
pub static BUILTIN_TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// A loaded template: its manifest plus the Typst sources that make it up.
#[derive(Debug, Clone)]
pub struct Template {
    /// Template name as declared in the manifest.
    pub name: String,
    /// One-line description from the manifest (used by `cv templates`).
    pub description: String,
    /// `design.*` fields this template honors (plan 3.5); drives the `cv check`
    /// warning for ignored fields (spec §6).
    pub supported_options: Vec<String>,
    /// Content of the entry-point `template.typ`.
    pub main_source: String,
    /// Additional Typst sources, as (virtual path, content) pairs (e.g. `lib/inline.typ`).
    pub extra_sources: Vec<(String, String)>,
}

impl Template {
    /// Whether any source text mentions `design.<group>` — the heuristic backing the
    /// manifest-vs-reads consistency check for custom templates (plan 5.4, ADR-0028).
    /// Aliasing (`let d = design`) defeats the scan, which is why mismatches only ever
    /// warn (ADR-0015) and built-ins are pinned by unit tests instead.
    pub fn references_group(&self, group: &str) -> bool {
        let needle = format!("design.{group}");
        self.main_source.contains(&needle)
            || self
                .extra_sources
                .iter()
                .any(|(_, source)| source.contains(&needle))
    }
}

/// Whether a `design.template` reference points at a custom folder rather than a
/// built-in name (ADR-0027): any path separator, or a leading `.` for `./x` and `../x`.
/// Bare names are built-ins, so `template: compactt` stays a "did you mean" mistake
/// rather than silently becoming a folder lookup.
pub fn is_path_like(reference: &str) -> bool {
    reference.contains('/') || reference.contains('\\') || reference.starts_with('.')
}

/// Load the template `design.template` refers to, resolving a path reference against
/// the CV file's directory (`base_dir`, plan 5.3).
pub fn load(reference: &str, base_dir: &Path) -> Result<Template> {
    if is_path_like(reference) {
        load_from_folder(&base_dir.join(reference), reference)
    } else if builtin_names().iter().any(|known| known == reference) {
        load_builtin(reference)
    } else {
        Err(unknown_template_error(reference))
    }
}

/// Names of the built-in templates, sorted for stable output.
///
/// A folder counts as a built-in only if it carries the `template.typ` entry point:
/// anything else embedded under `templates/` (a shared asset folder, a half-added
/// template) is not a template, and listing it would offer a name that `cv init
/// --template` and `design.template` cannot use.
pub fn builtin_names() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_TEMPLATES
        .dirs()
        .filter_map(|d| d.path().file_name())
        .filter_map(|n| n.to_str())
        .filter(|name| has_entry_point(&embedded_template_files(name)))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// The files embedded under `templates/<name>/` as (path relative to the folder,
/// contents) pairs, sorted for determinism. Empty for a name that is not embedded.
fn embedded_template_files(name: &str) -> Vec<(&'static str, &'static [u8])> {
    fn walk<'a>(dir: &Dir<'a>, out: &mut Vec<(&'a str, &'a [u8])>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::File(f) => out.extend(f.path().to_str().map(|p| (p, f.contents()))),
                DirEntry::Dir(d) => walk(d, out),
            }
        }
    }
    // Embedded paths are relative to `templates/` (e.g. `compact/template.typ`), so the
    // folder is selected by prefix; `Dir::get_file` compares full paths and would
    // never find the subfolder's file. Paths that are not valid UTF-8 cannot occur in
    // files that ship as source text, so they are skipped rather than errored on.
    let prefix = format!("{name}/");
    let mut files = Vec::new();
    walk(&BUILTIN_TEMPLATES, &mut files);
    let mut owned: Vec<(&'static str, &'static [u8])> = files
        .into_iter()
        .filter(|(p, _)| p.starts_with(prefix.as_str()))
        .map(|(p, c)| (&p[prefix.len()..], c))
        .collect();
    owned.sort_unstable_by_key(|(p, _)| *p);
    owned
}

/// Load a built-in template by name from the embedded assets.
pub fn load_builtin(name: &str) -> Result<Template> {
    let files = embedded_template_files(name);
    if !has_entry_point(&files) {
        // Either no such folder or a folder that is not a template (a shared asset
        // directory): from the user's side both read as an unknown name, and the answer
        // is the built-in list, not an internal error about our own assets.
        return Err(unknown_template_error(name));
    }
    assemble_builtin(name, &files)
}

/// Whether a file list is a template folder: a `template.typ` at its root.
fn has_entry_point(files: &[(&str, &[u8])]) -> bool {
    // Folder-relative, so equality also excludes a nested `lib/template.typ` helper.
    files.iter().any(|(path, _)| *path == "template.typ")
}

/// Defect in the binary's own embedded assets: shipped templates are pinned by the
/// parity tests in this module, so reaching a user through these arms means a `cv` bug.
fn builtin_asset_bug(name: &str, problem: impl Into<String>) -> Error {
    Error::Internal(format!(
        "built-in template '{name}' {} (this is a bug in cv)",
        problem.into()
    ))
}

/// Build a built-in template from the files embedded under its folder. Pure over the
/// file list, so a test can feed it broken synthetic assets: `load_builtin` can only
/// ever hand it the shipped ones.
fn assemble_builtin(name: &str, files: &[(&str, &[u8])]) -> Result<Template> {
    let find = |path: &str| files.iter().find(|(p, _)| *p == path).map(|(_, c)| *c);
    let text = |bytes: &[u8], what: &str| -> Result<String> {
        String::from_utf8(bytes.to_vec())
            .map_err(|_| builtin_asset_bug(name, format!("{what} is not valid UTF-8")))
    };

    let main_bytes =
        find("template.typ").ok_or_else(|| builtin_asset_bug(name, "has no template.typ"))?;
    let main_source = text(main_bytes, "template.typ")?;

    let extra_sources = files
        .iter()
        .filter(|(path, _)| *path != "template.typ" && path.ends_with(".typ"))
        .map(|(path, contents)| Ok(((*path).to_owned(), text(contents, path)?)))
        .collect::<Result<Vec<_>>>()?;

    let manifest_bytes =
        find("manifest.yaml").ok_or_else(|| builtin_asset_bug(name, "has no manifest.yaml"))?;
    let manifest = manifest::Manifest::from_yaml_str(&text(manifest_bytes, "manifest.yaml")?)?;
    // Every shipped manifest is usable: `every_builtin_manifest_is_usable_and_named_after_its_folder`
    // runs this same check on the embedded assets.
    manifest
        .check_usable()
        .map_err(|problem| builtin_asset_bug(name, format!("is not usable: {problem}")))?;

    Ok(template_from(&manifest, main_source, extra_sources))
}

/// Load a custom template folder from disk (spec §6, plan 5.3). `reference` is the
/// user-written path, kept verbatim in errors so messages match what was typed.
fn load_from_folder(folder: &Path, reference: &str) -> Result<Template> {
    if !folder.is_dir() {
        return Err(template_load_error(
            reference,
            format!("folder not found: '{}'", folder.display()),
        ));
    }

    let manifest_yaml = read_folder_file(folder, "manifest.yaml", reference)?.ok_or_else(|| {
        template_load_error(
            reference,
            "has no manifest.yaml — a template folder needs template.typ and manifest.yaml (`cv templates` lists the built-ins to copy)",
        )
    })?;
    let manifest = manifest::Manifest::from_yaml_str(&manifest_yaml).map_err(|e| {
        template_load_error(reference, format!("has an invalid manifest.yaml: {e}"))
    })?;
    manifest.check_usable().map_err(|problem| {
        template_load_error(
            reference,
            format!("has an unusable manifest.yaml: {problem}"),
        )
    })?;

    let mut sources = Vec::new();
    walk_typ_files(folder, "", reference, &mut sources)?;
    let index = sources
        .iter()
        .position(|(path, _)| path == "template.typ")
        .ok_or_else(|| {
            template_load_error(
                reference,
                "has no template.typ — a template folder needs template.typ and manifest.yaml (`cv templates` lists the built-ins to copy)",
            )
        })?;
    let main_source = sources.remove(index).1;

    Ok(template_from(&manifest, main_source, sources))
}

fn template_from(
    manifest: &manifest::Manifest,
    main_source: String,
    extra_sources: Vec<(String, String)>,
) -> Template {
    Template {
        name: manifest.name.clone(),
        description: manifest.description.clone(),
        supported_options: manifest.supported.clone(),
        main_source,
        extra_sources,
    }
}

fn template_load_error(reference: &str, message: impl Into<String>) -> Error {
    Error::TemplateLoad {
        name: reference.to_owned(),
        message: message.into(),
    }
}

/// Read one file from a template folder; `None` when it does not exist.
fn read_folder_file(folder: &Path, name: &str, reference: &str) -> Result<Option<String>> {
    let path = folder.join(name);
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| template_load_error(reference, format!("'{name}' is not valid UTF-8"))),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// Collect every `.typ` file under `dir` as (virtual path, content) pairs, virtual
/// paths relative to the folder root with `/` separators, sorted for determinism.
/// Non-Typst files (assets, README) are ignored: the compiler only ever sees sources.
fn walk_typ_files(
    dir: &Path,
    prefix: &str,
    reference: &str,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    // The `read_dir` and `read` failure arms are disk-fault paths (permissions,
    // I/O errors, files vanishing mid-walk); surfaced as typed `Error::Io` with
    // the path named. They cannot be reached from a well-formed template folder
    // on a healthy disk — same tolerated class as the other disk-failure arms.
    let entries = std::fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect();
    names.sort();
    for (name, path) in names {
        let virtual_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk_typ_files(&path, &virtual_path, reference, out)?;
        } else if name.ends_with(".typ") {
            let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                path: path.display().to_string(),
                source,
            })?;
            let content = String::from_utf8(bytes).map_err(|_| {
                template_load_error(reference, format!("'{virtual_path}' is not valid UTF-8"))
            })?;
            out.push((virtual_path, content));
        }
    }
    Ok(())
}

fn unknown_template_error(name: &str) -> Error {
    let known = builtin_names();
    let hint = match crate::suggest::closest_match(name, &known) {
        Some(best) => format!("did you mean '{best}'?"),
        None => format!("available templates: {}", known.join(", ")),
    };
    Error::Unsupported(format!(
        "unknown template '{name}' ({hint}). Custom template folders are referenced by path, e.g. './my-template'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal, usable manifest for the synthetic-asset tests below.
    const GOOD_MANIFEST: &str =
        "name: x\ndescription: test template\nschema_version: 1\nsupported: [page]\n";

    #[test]
    fn builtin_names_lists_both_builtins_sorted() {
        assert_eq!(builtin_names(), ["classic", "compact"]);
    }

    #[test]
    fn references_group_looks_through_the_helper_sources_too() {
        // The consistency scan covers the whole folder, not just the entry point:
        // a group read only in `lib/` must count as read (ADR-0028).
        let template = Template {
            name: "custom".into(),
            description: "test".into(),
            supported_options: vec![],
            main_source: "#let a = 1".into(),
            extra_sources: vec![("lib/type.typ".into(), "design.margins.x".into())],
        };
        assert!(
            template.references_group("margins"),
            "a group read in a helper source is read"
        );
        assert!(
            !template.references_group("colors"),
            "a group no source mentions is not read"
        );
    }

    #[test]
    fn embedded_files_are_folder_relative_and_sorted() {
        let paths: Vec<&str> = embedded_template_files("compact")
            .iter()
            .map(|(p, _)| *p)
            .collect();
        assert!(
            paths.contains(&"template.typ")
                && paths.contains(&"manifest.yaml")
                && paths.contains(&"lib/inline.typ"),
            "got: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("compact/")),
            "paths are relative to the template folder: {paths:?}"
        );
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "deterministic order");
        assert!(
            embedded_template_files("no-such-template").is_empty(),
            "an unembedded name has no files"
        );
    }

    #[test]
    fn assemble_builtin_reports_a_folder_without_an_entry_point() {
        let err = assemble_builtin("x", &[("manifest.yaml", GOOD_MANIFEST.as_bytes())])
            .expect_err("no template.typ is a broken bundle");
        assert!(
            err.to_string().contains("has no template.typ"),
            "got: {err}"
        );
    }

    #[test]
    fn assemble_builtin_reports_a_folder_without_a_manifest() {
        let err = assemble_builtin("x", &[("template.typ", b"#page \"\"".as_slice())])
            .expect_err("no manifest.yaml is a broken bundle");
        assert!(
            err.to_string().contains("has no manifest.yaml"),
            "got: {err}"
        );
    }

    #[test]
    fn assemble_builtin_reports_undecodable_sources() {
        let err = assemble_builtin(
            "x",
            &[
                ("template.typ", [0xff_u8, 0xfe, 0x00][..].as_ref()),
                ("manifest.yaml", GOOD_MANIFEST.as_bytes()),
            ],
        )
        .expect_err("a non-UTF-8 entry point is a broken bundle");
        let message = err.to_string();
        assert!(
            message.contains("template.typ") && message.contains("not valid UTF-8"),
            "got: {message}"
        );

        let err = assemble_builtin(
            "x",
            &[
                ("template.typ", b"#let a = 1".as_slice()),
                ("lib/broken.typ", [0xff_u8][..].as_ref()),
                ("manifest.yaml", GOOD_MANIFEST.as_bytes()),
            ],
        )
        .expect_err("a non-UTF-8 helper is a broken bundle");
        assert!(
            err.to_string().contains("lib/broken.typ"),
            "the failing source is named: {err}"
        );
    }

    #[test]
    fn assemble_builtin_reports_an_unusable_or_unparseable_manifest() {
        let files = [
            ("template.typ", "#let a = 1".as_bytes()),
            (
                "manifest.yaml",
                b"name: x\ndescription: d\nschema_version: 9\n".as_slice(),
            ),
        ];
        let err = assemble_builtin("x", &files).expect_err("a foreign manifest version");
        assert!(err.to_string().contains("is not usable"), "got: {err}");

        let files = [
            ("template.typ", "#let a = 1".as_bytes()),
            ("manifest.yaml", b"name: x\nnope: 1\n".as_slice()),
        ];
        let err = assemble_builtin("x", &files).expect_err("a malformed manifest");
        assert!(err.to_string().contains("nope"), "the key is named: {err}");
    }

    #[test]
    fn assemble_builtin_loads_helpers_and_ignores_non_sources() {
        let files = [
            ("manifest.yaml", GOOD_MANIFEST.as_bytes()),
            (
                "template.typ",
                b"#import \"lib/inline.typ\": inline".as_slice(),
            ),
            ("lib/inline.typ", b"#let inline(x) = x".as_slice()),
            // Binary assets are not compiler sources: undecodable bytes are fine here.
            ("assets/logo.png", [0xff_u8, 0x50, 0x4e, 0x47][..].as_ref()),
        ];
        let template = assemble_builtin("x", &files).expect("assembles");
        assert_eq!(template.name, "x");
        assert_eq!(
            template.extra_sources,
            vec![("lib/inline.typ".to_owned(), "#let inline(x) = x".to_owned())],
            "only .typ helpers, in path order"
        );
    }

    #[test]
    fn assemble_builtin_reports_an_undecodable_manifest() {
        let files = [
            ("template.typ", "#let a = 1".as_bytes()),
            ("manifest.yaml", [0xc3_u8, 0x28, 0x61][..].as_ref()),
        ];
        let err = assemble_builtin("x", &files).expect_err("undecodable manifest");
        assert!(
            err.to_string().contains("manifest.yaml") && err.to_string().contains("UTF-8"),
            "the failing asset is named: {err}"
        );
    }

    #[test]
    fn references_group_sees_the_helper_sources_too() {
        // The consistency check reads the whole folder: a group mentioned only in a
        // helper must count as read, and an unrelated helper must not invent one.
        let template = Template {
            name: "custom".into(),
            description: "d".into(),
            supported_options: vec![],
            main_source: "#let d = 1".into(),
            extra_sources: vec![
                (
                    "lib/inline.typ".to_owned(),
                    "design.colors.accent".to_owned(),
                ),
                ("lib/page.typ".to_owned(), "design.page".to_owned()),
            ],
        };
        assert!(template.references_group("colors"), "helper source counts");
        assert!(template.references_group("page"));
        assert!(
            !template.references_group("margins"),
            "a group no source mentions is not read"
        );
    }

    #[test]
    fn an_unreadable_template_directory_is_a_typed_io_error() {
        // `walk_typ_files` reads the folder before reading its sources: a path that
        // is not a directory surfaces as `Error::Io` naming it, never a panic.
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, "x").expect("write file");
        let mut sources = Vec::new();
        let err = walk_typ_files(&file, "", "./x", &mut sources).expect_err("not a directory");
        assert!(
            err.to_string().contains("not-a-dir"),
            "the error names the path: {err}"
        );
    }

    #[test]
    fn load_builtin_compact_has_sources_and_manifest() {
        let template = load_builtin("compact").expect("compact must load");
        assert_eq!(template.name, "compact");
        assert!(
            !template.description.is_empty(),
            "manifest needs a description"
        );
        assert!(
            template.main_source.contains("data.yaml"),
            "template reads the data file"
        );
        assert!(
            template.main_source.contains("document("),
            "template sets document metadata"
        );
    }

    #[test]
    fn classic_loads_with_all_section_types_and_the_inline_helper() {
        let template = load_builtin("classic").expect("classic must load");
        assert_eq!(template.name, "classic");
        for section_type in ["entries", "key_values", "bullets", "text"] {
            assert!(
                template
                    .main_source
                    .contains(&format!("section.type == \"{section_type}\"")),
                "classic must support the {section_type} section type"
            );
        }
        assert!(
            template
                .extra_sources
                .iter()
                .any(|(path, _)| path == "lib/inline.typ"),
            "classic ships the inline Markdown helper"
        );
    }

    #[test]
    fn every_builtin_reads_exactly_the_groups_its_manifest_declares() {
        // The ADR-0023 warning and the ADR-0028 consistency check must never fire for
        // shipped templates: each declares every group its sources read, and reads
        // every group it declares.
        for name in builtin_names() {
            let template = load_builtin(&name).expect("built-in loads");
            for group in manifest::DESIGN_GROUPS {
                let declared = template
                    .supported_options
                    .iter()
                    .any(|option| option == group);
                assert_eq!(
                    declared,
                    template.references_group(group),
                    "template '{name}' and design group '{group}' must agree on \
                     declared-vs-read (declared={declared})"
                );
            }
        }
    }

    #[test]
    fn every_builtin_manifest_is_usable_and_named_after_its_folder() {
        // `load_builtin` runs `Manifest::check_usable` on every manifest, so passing
        // here pins usability for shipped templates (ADR-0028).
        for name in builtin_names() {
            let template = load_builtin(&name).expect("built-in loads and its manifest is usable");
            assert_eq!(
                template.name, name,
                "manifest name must match the built-in folder name"
            );
        }
    }

    #[test]
    fn path_like_references_are_custom_folders_bare_names_are_builtins() {
        assert!(is_path_like("./my-template"));
        assert!(is_path_like("../shared/tpl"));
        assert!(is_path_like("/abs/tpl"));
        assert!(is_path_like("tpl\\sub"));
        assert!(is_path_like("."));
        assert!(!is_path_like("compact"));
        assert!(!is_path_like("classic"));
    }

    #[test]
    fn load_resolves_a_path_reference_against_the_base_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("my-template");
        std::fs::create_dir(&folder).expect("create folder");
        let err = load("./my-template", dir.path()).expect_err("empty folder is not a template");
        let message = err.to_string();
        assert!(
            message.contains("my-template") && message.contains("template.typ"),
            "the error must name the reference and the missing entry point: {message}"
        );
    }

    #[test]
    fn load_of_a_bare_unknown_name_keeps_the_did_you_mean_hint() {
        let err = load("comapct", Path::new(".")).expect_err("typo'd name must fail");
        let msg = err.to_string();
        assert!(msg.contains("did you mean 'compact'"), "got: {msg}");
    }

    #[test]
    fn load_builtin_itself_rejects_names_that_are_not_embedded() {
        // The loader's own contract (callers pre-check via `load`, but the shape
        // stands alone): unknown embedded names get the did-you-mean error.
        let err = load_builtin("comapct").expect_err("typo'd name must fail");
        assert!(err.to_string().contains("did you mean 'compact'"));
    }

    #[test]
    fn unknown_template_lists_available_names() {
        let err = load("nonexistent-template", Path::new(".")).expect_err("unknown must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("compact") && msg.contains("classic"),
            "got: {msg}"
        );
    }
}
