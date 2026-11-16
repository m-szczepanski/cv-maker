//! `cv build` (plan 1.6): parse YAML, render via Typst, write the PDF atomically.
//! The one-page fit loop (spec §7, plan 4.1) drives repeated compiles against one
//! engine, so fonts and template sources load exactly once per build.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cli::{BuildArgs, Cli};
use crate::error::{Error, Result};
use crate::fit::{self, FitConfig};
use crate::render::{self, Engine, FitParams, RenderRequest};

pub fn run(args: &BuildArgs, cli: &Cli) -> Result<()> {
    let started = Instant::now();

    let loaded = crate::commands::load_cv(&args.file, &cli.font_path)?;
    let document = loaded.document;
    // The reference was resolved and judged by validation, which handed the template
    // over: a built-in name or the custom folder path, relative to the YAML file.
    let template = loaded.template;

    let data_yaml = render::data::to_yaml(&document.cv, &document.design);
    let request = RenderRequest {
        template,
        data_yaml,
        user_font_items: loaded.font_items,
    };

    let engine = Engine::build(&request);
    let config = FitConfig {
        // `--no-fit` overrides the design setting for this run (plan 4.4).
        enabled: document.design.fit_one_page && !args.no_fit,
        ..FitConfig::from_design(&document.design)
    };
    let outcome = fit::fit(&engine, &config).map_err(Error::Render)?;
    let output = outcome.output;

    if cli.verbose {
        super::err_line(&format!("cv: fit: {} compiles", outcome.compiles));
    }

    let out_path = match &args.output {
        Some(p) => p.clone(),
        None => default_output_path(&args.file),
    };
    crate::publish::publish(&out_path, &output.pdf)?;

    super::report_warnings(&loaded.warnings);
    // Typst render warnings (e.g. an unknown font in a custom template) are essentials,
    // not progress: they print under `--quiet` too (ADR-0012).
    for warning in &output.warnings {
        super::err_line(&format!("warning: {warning}"));
    }

    if outcome.overflowed {
        // Spec §7 step 4 + ADR-0026: the PDF was written; this warning is an
        // essential, so it also prints under `--quiet` (ADR-0012), and the exit
        // code stays 0 — overflow is a result, not an error.
        super::report_overflow(&output, config.target_pages);
    }

    if !cli.quiet {
        let ms = started.elapsed().as_millis();
        super::out_line(&format!(
            "{}  {} page{}  {}  ({} ms)",
            out_path.display(),
            output.page_count,
            if output.page_count == 1 { "" } else { "s" },
            fit_note(&outcome.applied),
            ms
        ));
    }

    Ok(())
}

/// The fit part of the success line (spec §7 "reports the applied scale",
/// ADR-0025 amending ADR-0012): a single `scale` (the font scale) when spacing
/// and margins did not move — byte-compatible with the pre-M4 line for
/// first-fit CVs — plus the spacing/margins scales when they did, tightened
/// below 1.0 or grown by the fill phase above it (ADR-0044).
fn fit_note(applied: &FitParams) -> String {
    if applied.spacing_scale == 1.0 && applied.margin_scale == 1.0 {
        format!("fit: scale {:.2}", applied.font_scale)
    } else {
        format!(
            "fit: scale {:.2} spacing {:.2} margins {:.2}",
            applied.font_scale, applied.spacing_scale, applied.margin_scale
        )
    }
}

/// Default PDF location: same directory and file stem as the input (spec §3).
fn default_output_path(input: &Path) -> PathBuf {
    let stem = input.file_stem().unwrap_or(input.as_os_str());
    input.with_file_name(format!("{}.pdf", stem.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_fit_line_reports_a_single_scale() {
        assert_eq!(fit_note(&FitParams::default()), "fit: scale 1.00");
    }

    #[test]
    fn tightened_fit_line_adds_spacing_and_margins() {
        let applied = FitParams {
            font_scale: 0.94,
            spacing_scale: 0.8,
            margin_scale: 1.0,
        };
        assert_eq!(
            fit_note(&applied),
            "fit: scale 0.94 spacing 0.80 margins 1.00"
        );
    }
}
