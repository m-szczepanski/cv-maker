//! `cv build` (plan 1.6): parse YAML, render via Typst, write the PDF atomically.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cli::{BuildArgs, Cli};
use crate::error::{Error, Result};
use crate::model::Document;
use crate::render::{self, RenderRequest};

pub fn run(args: &BuildArgs, cli: &Cli) -> Result<()> {
    let started = Instant::now();

    let yaml = std::fs::read_to_string(&args.file).map_err(|source| Error::Io {
        path: args.file.display().to_string(),
        source,
    })?;
    let document = Document::from_yaml_str(&yaml)?;

    let template = crate::templates::load_builtin(&document.design.template)?;

    let mut font_dirs: Vec<PathBuf> = cli.font_path.iter().map(|p| p.to_path_buf()).collect();
    for rel in &document.design.font_paths {
        font_dirs.push(resolve_relative(&args.file, rel));
    }

    let data_yaml = render::data::to_yaml(&document.cv, &document.design)?;
    let request = RenderRequest {
        template,
        data_yaml,
        user_font_dirs: font_dirs,
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

    if !cli.quiet {
        let ms = started.elapsed().as_millis();
        println!(
            "{}  {} page{}  fit: scale 1.00  ({} ms)",
            out_path.display(),
            output.page_count,
            if output.page_count == 1 { "" } else { "s" },
            ms
        );
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

/// `design.font_paths` entries are relative to the YAML file's directory (spec §4 example).
fn resolve_relative(input: &Path, rel: &str) -> PathBuf {
    let base = input.parent().unwrap_or(Path::new("."));
    base.join(rel)
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
    (temp.as_file().write_all(bytes)).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    temp.persist(path).map_err(|e| {
        // persist errors carry the original error and the destination
        Error::Unsupported(format!(
            "could not move PDF into place at {}: {e}",
            path.display()
        ))
    })?;
    Ok(())
}
