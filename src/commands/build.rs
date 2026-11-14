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
    write_atomic(&out_path, &output.pdf)?;

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

/// Write to a temp file in the target directory, then rename it over the destination
/// (spec §3: a failed build never leaves a corrupt PDF). Both disk steps report the
/// same typed error naming the destination.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".cv-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|source| write_failure(path, source))?;
    write_to(&mut temp.as_file(), path, bytes)?;
    temp.persist(path)
        .map_err(|e| write_failure(path, e.error))?;
    Ok(())
}

/// The PDF bytes into a sink, then flush it: the rename that follows must not publish
/// a file whose tail is still buffered (spec §3). The sink is a parameter so this
/// user-facing failure has a test: a real disk only breaks when it is full or failing,
/// which no test can arrange, while the message the user sees must be right.
///
/// Coverage note (rule 21): the fault is injected at this seam, so `write_atomic`'s
/// `?` below never sees an error in a test — the disk in a temp directory always
/// accepts the PDF.
fn write_to(sink: &mut dyn std::io::Write, path: &Path, bytes: &[u8]) -> Result<()> {
    sink.write_all(bytes)
        .and_then(|()| sink.flush())
        .map_err(|source| write_failure(path, source))
}

/// One write failure: the destination that was asked for, the OS reason.
fn write_failure(path: &Path, source: std::io::Error) -> Error {
    Error::Write {
        path: path.display().to_string(),
        message: source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// A sink that can be told to break on the write, on the flush, or on neither:
    /// the three states of the disk the PDF is written to.
    struct Sink {
        break_write: bool,
        break_flush: bool,
        written: Vec<u8>,
    }

    impl Sink {
        /// A disk that takes the bytes.
        fn working() -> Self {
            Self {
                break_write: false,
                break_flush: false,
                written: Vec::new(),
            }
        }

        /// A disk with no room left: `write` itself refuses.
        fn full() -> Self {
            Self {
                break_write: true,
                break_flush: false,
                written: Vec::new(),
            }
        }

        /// A device that accepts the bytes and loses them on the way out: the failure
        /// only shows when the buffer is flushed.
        fn failing() -> Self {
            Self {
                break_write: false,
                break_flush: true,
                written: Vec::new(),
            }
        }
    }

    impl io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.break_write {
                return Err(io::Error::other("No space left on device"));
            }
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.break_flush {
                return Err(io::Error::other("disk I/O error"));
            }
            Ok(())
        }
    }

    fn write_error(sink: &mut Sink) -> String {
        write_to(sink, Path::new("cv.pdf"), b"%PDF-1.7")
            .expect_err("a broken disk must fail the write")
            .to_string()
    }

    #[test]
    fn a_full_disk_names_the_destination_and_the_os_reason() {
        let message = write_error(&mut Sink::full());
        assert!(
            message.contains("cv.pdf") && message.contains("No space left on device"),
            "the user must see which file and why: {message}"
        );
    }

    /// The bytes can leave the buffer only to be lost: a flush failure is the same
    /// typed error, or the rename would publish a truncated PDF.
    #[test]
    fn a_failing_device_is_reported_at_the_flush() {
        let message = write_error(&mut Sink::failing());
        assert!(
            message.contains("cv.pdf") && message.contains("disk I/O error"),
            "{message}"
        );
    }

    #[test]
    fn a_healthy_sink_receives_every_byte() {
        let mut sink = Sink::working();
        write_to(&mut sink, Path::new("cv.pdf"), b"%PDF-1.7 bytes").expect("the write works");
        assert_eq!(sink.written, b"%PDF-1.7 bytes", "nothing lost or reordered");
    }

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
