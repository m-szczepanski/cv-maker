//! `cv build` (plan 1.6): parse YAML, render via Typst, write the PDF atomically.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cli::{BuildArgs, Cli};
use crate::error::{Error, Result};
use crate::render::{self, RenderRequest};

pub fn run(args: &BuildArgs, cli: &Cli) -> Result<()> {
    let started = Instant::now();

    let loaded = crate::commands::load_cv(&args.file, &cli.font_path)?;
    let document = loaded.document;

    let template = crate::templates::load_builtin(&document.design.template)?;

    let data_yaml = render::data::to_yaml(&document.cv, &document.design)?;
    let request = RenderRequest {
        template,
        data_yaml,
        user_font_dirs: loaded.font_dirs,
    };

    // M1: single compile, no fitting (the loop arrives in M4; --no-fit is accepted and is
    // trivially honored because there is no loop to disable yet).
    let _ = args.no_fit;
    let output = render::render(&request).map_err(Error::Render)?;

    let out_path = match &args.output {
        Some(p) => p.clone(),
        None => default_output_path(&args.file),
    };
    write_atomic(&out_path, &output.pdf)?;

    super::report_warnings(&loaded.validation.warnings);

    if !cli.quiet {
        let ms = started.elapsed().as_millis();
        println!(
            "{}  {} page{}  fit: scale 1.00  ({} ms)",
            out_path.display(),
            output.page_count,
            if output.page_count == 1 { "" } else { "s" },
            ms
        );
        // Typst render warnings (e.g. font substitution) cannot occur for validated
        // input in M2: validation resolves families against the same font book the
        // render uses (ADR-0014), so every state Typst would warn about is rejected
        // before it compiles. Kept as the reporting point for M5 custom templates.
        for warning in &output.warnings {
            eprintln!("warning: {warning}");
        }
    }

    Ok(())
}

/// Default PDF location: same directory and file stem as the input (spec §3).
fn default_output_path(input: &Path) -> PathBuf {
    let stem = input.file_stem().unwrap_or(input.as_os_str());
    input.with_file_name(format!("{}.pdf", stem.to_string_lossy()))
}

/// Write to a temp file in the target directory, then rename (spec §3: a failed build must
/// never leave a corrupt PDF).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".cv-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|source| Error::Io {
            path: dir.display().to_string(),
            source,
        })?;
    // Writing a freshly created temp file in a directory that just accepted it can only
    // fail on a full or failing disk; surfaced as an I/O error rather than a panic.
    (temp.as_file().write_all(bytes)).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    temp.persist(path).map_err(|e| {
        // persist errors carry the original error and the destination; the path is
        // included in the Write variant's message
        Error::Write {
            path: path.display().to_string(),
            message: e.to_string(),
        }
    })?;
    Ok(())
}
