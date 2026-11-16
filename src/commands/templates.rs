//! `cv templates` (plan 5.5): list the built-in templates with their manifest
//! descriptions (spec §3). Custom folders are not listed: they are addressed by path
//! in the YAML, and there is no registry to enumerate (ADR-0027).
//!
//! `--extract <name>` writes a built-in's folder to disk so it can be edited and
//! used as a custom template — the DIY path of spec §6 (a user copies a built-in,
//! edits it, and points `design.template` at the folder).

use crate::cli::TemplatesArgs;
use crate::error::Result;
use crate::templates;

pub fn run(args: &TemplatesArgs) -> Result<()> {
    run_with(args, &templates::load_builtin)
}

/// `run` with the built-in loader injected. Listing loads every embedded folder:
/// a failure there is a defect in the shipped assets, reported as the typed
/// internal error `load_builtin` builds rather than assumed away (ADR-0042). The
/// loader is a parameter so both arms of that failure path — and the listing
/// itself — are tested behaviour, not commented-impossible code (the seam pattern
/// of ADR-0036).
fn run_with(
    args: &TemplatesArgs,
    load: &dyn Fn(&str) -> Result<templates::Template>,
) -> Result<()> {
    if let Some(name) = &args.extract {
        return extract(name, args);
    }
    let loaded: Vec<templates::Template> = templates::builtin_names()
        .iter()
        .map(|name| load(name))
        .collect::<Result<_>>()?;
    let width = loaded
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or_default();
    // Essential output, like `cv fonts`: kept even under `--quiet` (ADR-0012/0021).
    for template in &loaded {
        super::out_line(&format!(
            "{:<width$}  {}",
            template.name,
            template.description,
            width = width
        ));
    }
    Ok(())
}

/// Write the built-in folder to disk and print how to use it (spec §6 custom
/// templates; M5 acceptance: "a user can copy a built-in template folder, edit it,
/// and use it via `design.template: ./my-template`").
fn extract(name: &str, args: &TemplatesArgs) -> Result<()> {
    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from(name));
    let written = templates::extract_builtin(name, &out_dir, args.force)?;
    super::out_line(&format!(
        "extracted '{name}' to {} ({} files)",
        out_dir.display(),
        written.len()
    ));
    super::out_line(&format!(
        "edit it freely, then use it: set `design.template: {}` in cv.yaml \
         (path references resolve against the cv.yaml's directory)",
        template_ref(&out_dir)
    ));
    Ok(())
}

/// The string to write into `design.template` for an extracted folder: a relative
/// path becomes `./`-prefixed (a bare name reads as a built-in, spec §6), while a
/// path already written as `./`, `../` or absolute is passed through unchanged.
fn template_ref(out_dir: &std::path::Path) -> String {
    let display = out_dir.display().to_string();
    if out_dir.is_relative() && !display.starts_with('.') {
        format!("./{display}")
    } else {
        display
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_bare_relative_name_is_written_as_a_path_reference() {
        // `sidebar` alone would resolve as a built-in name, so the extracted folder
        // must be referenced as `./sidebar`.
        assert_eq!(template_ref(Path::new("sidebar")), "./sidebar");
    }

    #[test]
    fn explicit_relative_and_absolute_paths_pass_through() {
        assert_eq!(template_ref(Path::new("./tpl")), "./tpl");
        assert_eq!(template_ref(Path::new("../shared/tpl")), "../shared/tpl");
        assert_eq!(template_ref(Path::new("/abs/tpl")), "/abs/tpl");
    }

    #[test]
    fn nested_relative_dirs_get_the_dot_slash_prefix() {
        assert_eq!(template_ref(Path::new("a/b/tpl")), "./a/b/tpl");
    }

    fn list_args() -> TemplatesArgs {
        TemplatesArgs {
            extract: None,
            out: None,
            force: false,
        }
    }

    #[test]
    fn run_with_lists_every_shipped_builtin() {
        run_with(&list_args(), &templates::load_builtin).expect("the listing succeeds");
    }

    #[test]
    fn a_failing_loader_reports_the_typed_asset_defect() {
        // The listing's collect error arm: one broken bundle aborts it with the
        // internal error the loader built, not a panic and not a silent partial list.
        let err = run_with(&list_args(), &|name| {
            if name == "modern" {
                Err(crate::error::Error::Internal(
                    "built-in template 'modern' is not usable (this is a bug in cv)".into(),
                ))
            } else {
                templates::load_builtin(name)
            }
        })
        .expect_err("a broken shipped asset must fail the listing");
        assert!(
            err.to_string().contains("internal error") && err.to_string().contains("modern"),
            "the defect must be reported typed and named: {err}"
        );
    }
}
