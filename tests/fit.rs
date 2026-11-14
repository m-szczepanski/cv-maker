//! M4 fit-loop end-to-end tests (plan 4.1/4.5): run the real binary; the PDF page
//! count is ground truth, the success line must agree with it (spec §7).

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

/// A CV with `entries` × `bullets_per_entry` lines of content: `14 × 2` overflows
/// one A4 page at full sizes, `4 × 1` fits comfortably.
fn cv_yaml(entries: usize, bullets_per_entry: usize) -> String {
    let mut items = String::new();
    for i in 0..entries {
        items.push_str(&format!(
            "    - heading: Role {i}\n      org: Org {i}\n      bullets:\n"
        ));
        for b in 0..bullets_per_entry {
            items.push_str(&format!(
                "        - Achievement number {i}.{b} with realistic wording\n"
            ));
        }
    }
    format!(
        "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n  - title: Experience\n    type: entries\n    items:\n{items}\n"
    )
}

/// A CV with one section per element of `entries_per_section`: multiple sections
/// give the template many flow points, so pages break between entries naturally —
/// the multi-page shape the `design.pages` tests need.
fn sections_yaml(entries_per_section: &[usize]) -> String {
    let mut body = String::new();
    for (s, per) in entries_per_section.iter().enumerate() {
        body.push_str(&format!(
            "  - title: Section {s}\n    type: entries\n    items:\n"
        ));
        for i in 0..*per {
            body.push_str(&format!(
                "      - heading: Role {s}.{i}\n        org: Org {s}.{i}\n        bullets:\n          - Achievement number {s}.{i} with realistic wording\n"
            ));
        }
    }
    format!("schema: 1\ncv:\n  name: Jane Doe\n  sections:\n{body}\n")
}

/// Plan 4.5: an overflowing CV fits after the loop shrinks, and the applied scale
/// is below 1.0 in the success line.
#[test]
fn overflowing_cv_fits_after_shrinking() {
    let dir = unique_dir("fit-shrink");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "the fitting build must succeed, got: {out}");

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text extraction");
    assert_eq!(
        pages.len(),
        1,
        "the fit loop must guarantee one page, log: {out}"
    );
    assert!(
        out.contains("1 page"),
        "the success line must agree with the PDF, got: {out}"
    );
    assert!(
        ["scale 0.", "spacing 0.", "margins 0."]
            .iter()
            .any(|prefix| out.contains(prefix)),
        "at least one applied scale must be below 1.00, got: {out}"
    );
}

/// Plan 4.5 hard requirement: a CV that already fits is never shrunk, at the
/// original (single-scale) line.
#[test]
fn short_cv_is_not_shrunk() {
    let dir = unique_dir("fit-untouched");
    fs::write(dir.join("cv.yaml"), cv_yaml(4, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("1 page") && out.contains("fit: scale 1.00"),
        "a first-fit CV must report scale 1.00 without tightening details, got: {out}"
    );
}

/// Plan 4.5: same input → same applied scales and byte-identical PDFs, with the
/// loop active (not just for first-fit documents).
#[test]
fn fitted_builds_are_deterministic() {
    let dir = unique_dir("fit-determinism");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, a) = run_cv(&dir, &["build", "--output", "a.pdf"]);
    let (ok2, b) = run_cv(&dir, &["build", "--output", "b.pdf"]);
    assert!(ok && ok2, "got: {a} / {b}");
    assert_eq!(
        fit_note_of(&a),
        fit_note_of(&b),
        "same input must apply the same scales"
    );
    assert_eq!(
        fs::read(dir.join("a.pdf")).expect("a.pdf"),
        fs::read(dir.join("b.pdf")).expect("b.pdf"),
        "a fitted build must stay byte-deterministic"
    );
}

/// Plan 5.1 (spec §7 fit contract, M5): every built-in honors it — the same
/// overflowing content shrinks `classic` to one page exactly like `compact`.
#[test]
fn the_fit_loop_shrinks_classic_like_compact() {
    let dir = unique_dir("fit-classic");
    let classic = cv_yaml(14, 2) + "\ndesign:\n  template: classic\n";
    fs::write(dir.join("cv.yaml"), classic).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "classic.pdf"]);
    assert!(ok, "classic fit build: {out}");
    assert!(
        out.contains("1 page"),
        "classic must fit after shrinking, got: {out}"
    );
    let note = fit_note_of(&out);
    assert!(
        ["scale 0.", "spacing 0.", "margins 0."]
            .iter()
            .any(|prefix| note.contains(prefix)),
        "at least one applied scale must be below 1.00, got: {note}"
    );
}

/// Plan 4.5 / spec §7: what the loop reports never crosses the floors defined
/// for the default design (font ≥ 8.5pt of 9.5pt, spacing ≥ 0.6, margins ≥ 1 cm).
#[test]
fn fitted_scales_never_cross_the_floors() {
    let dir = unique_dir("fit-floors");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    let note = fit_note_of(&out);
    let mut parsed = vec![];
    let mut parts = note.split_whitespace();
    while let Some(label) = parts.next() {
        let value = parts
            .next()
            .unwrap_or_else(|| panic!("value after {label}"));
        parsed.push((
            label.to_lowercase(),
            value.parse::<f64>().expect("scale number"),
        ));
    }
    for (label, value) in &parsed {
        let floor = match label.as_str() {
            "scale" => 8.5 / 9.5 - 0.005, // %.2 display rounding
            "spacing" => 0.6 - 0.005,
            "margins" => 1.0 / 1.2 - 0.005,
            other => panic!("unexpected scale label {other}"),
        };
        assert!(
            (0.0..=1.0001).contains(value) && *value >= floor,
            "{label} = {value} crossed the spec §7 floor"
        );
    }
}

/// Extract the stable `fit: ...` part of the success line (ADR-0025 format),
/// without the timing suffix.
fn fit_note_of(out: &str) -> String {
    let start = out
        .find("fit: ")
        .unwrap_or_else(|| panic!("no fit note in: {out}"));
    out[start + "fit: ".len()..]
        .split("  (")
        .next()
        .expect("fit segment")
        .to_owned()
}

/// `--verbose` exposes the loop telemetry (compile count) for diagnostics.
#[test]
fn verbose_reports_the_compile_count() {
    let dir = unique_dir("fit-verbose");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["--verbose", "build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("compiles"),
        "verbose must report the fit compile count, got: {out}"
    );
}

/// Plan 4.4: `--no-fit` disables the loop for the run — an overflowing CV is
/// written as-is, reports `scale 1.00`, and gets no floor warning.
#[test]
fn no_fit_flag_skips_the_loop() {
    let dir = unique_dir("fit-nofit-flag");
    fs::write(dir.join("cv.yaml"), cv_yaml(14, 2)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--no-fit", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("2 pages") && out.contains("fit: scale 1.00"),
        "disabled loop must write through the overflow, got: {out}"
    );
    assert!(
        !out.contains("content still overflows"),
        "no floor warning when the user opted out, got: {out}"
    );
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 2);
}

/// Plan 4.4: the design field does the same permanently (spec §4/§7: the loop
/// runs only when `design.fit_one_page` is true).
#[test]
fn design_fit_one_page_false_skips_the_loop() {
    let dir = unique_dir("fit-design-off");
    let yaml = cv_yaml(14, 2) + "\ndesign:\n  fit_one_page: false\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("2 pages") && out.contains("fit: scale 1.00"),
        "design-level opt-out must skip shrinking, got: {out}"
    );
    assert!(!out.contains("content still overflows"), "got: {out}");
}

/// Plan 4.3 / ADR-0026: impossible content still writes the PDF and exits 0,
/// with a warning stating the overflow and the suggestion; the success line
/// shows the exact floors. The verbose telemetry proves the real-engine fit
/// ran exactly the documented worst-case probe budget.
#[test]
fn impossible_content_still_builds_with_a_warning_and_exit_zero() {
    let dir = unique_dir("fit-overflow");
    fs::write(dir.join("cv.yaml"), cv_yaml(60, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["--verbose", "build", "--output", "cv.pdf"]);
    assert!(
        ok,
        "overflow must not fail the build (ADR-0026), got: {out}"
    );
    assert!(
        out.contains("cv: fit: 13 compiles"),
        "A(5+1) + B(5+1) + identity is the hard bound (ADR-0024), got: {out}"
    );
    assert!(
        out.contains("content still overflows at the fit floor"),
        "the warning must be printed, got: {out}"
    );
    assert!(
        out.contains("spill onto") && out.contains("relax design.fit_floor"),
        "the warning must quantify the spill and suggest a fix, got: {out}"
    );
    assert!(
        out.contains("fit: scale 0.89 spacing 0.60 margins 0.83"),
        "the line must report the exact floors for the default design, got: {out}"
    );

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert!(pages.len() > 1, "the overflowed PDF is still written");
}

/// `design.pages: 2` raises the target: content that naturally flows onto a
/// second page is a first fit — never shrunk (every scale stays ≥ 1.00; the
/// fill phase may grow them), no warning, and the PDF keeps both pages.
#[test]
fn two_page_target_does_not_shrink_content_that_fits_two_pages() {
    let dir = unique_dir("fit-two-pages");
    let yaml = sections_yaml(&[10, 10]) + "\ndesign:\n  pages: 2\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(out.contains("2 pages"), "got: {out}");
    assert!(
        !out.contains("scale 0.") && !out.contains("spacing 0.") && !out.contains("margins 0."),
        "a CV within its 2-page target must not be shrunk, got: {out}"
    );
    assert!(!out.contains("content still overflows"), "got: {out}");
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 2, "got: {out}");
}

/// The overflow warning measures the spill against the target: with `pages: 2`,
/// the "extra pages" figure is the PDF's page count minus two, and the warning
/// suggests raising `design.pages` (the new escape hatch).
#[test]
fn overflow_warning_counts_pages_past_the_target() {
    let dir = unique_dir("fit-overflow-two-pages");
    let yaml = sections_yaml(&[10, 10, 10, 10]) + "\ndesign:\n  pages: 2\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["--verbose", "build", "--output", "cv.pdf"]);
    assert!(
        ok,
        "overflow must not fail the build (ADR-0026), got: {out}"
    );
    assert!(
        out.contains("content still overflows at the fit floor"),
        "the warning must be printed, got: {out}"
    );
    assert!(
        out.contains("raise design.pages"),
        "the warning must suggest raising design.pages, got: {out}"
    );

    let reported = reported_extra_pages(&out);
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(
        pages.len(),
        2 + reported,
        "the warning's figure must be the PDF's pages minus the target, got: {out}"
    );
}

/// ADR-0044: a sparse CV with a two-page target is *filled*, not just permitted:
/// the fill stages grow the scales (spacing to its cap here) so both pages
/// carry content instead of stranding a few lines on the last page.
#[test]
fn a_sparse_two_page_cv_is_filled_by_grown_scales() {
    let dir = unique_dir("fit-fill");
    let yaml = sections_yaml(&[13]) + "\ndesign:\n  pages: 2\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("spacing 3.00"),
        "sparse content reaches the spacing cap, got: {out}"
    );
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 2, "got: {out}");
    assert!(
        pages[1].chars().count() > 200,
        "page 2 must be substantially filled, got: {:?}",
        pages[1]
    );
    assert!(!out.contains("content still overflows"), "got: {out}");
}

/// ADR-0045: `cv.gdpr` renders on the last page — the only page for a one-page
/// CV, the target page for a multi-page CV. (The bottom pinning itself is
/// asserted against real layout in `src/render`.)
#[test]
fn the_gdpr_statement_renders_on_the_last_page() {
    let gdpr = "GDPR-SENTINEL: I agree that my personal data may be processed.";
    let with_gdpr = |entries: &[usize]| {
        let mut yaml = sections_yaml(entries);
        yaml.insert_str(
            yaml.find("  name: Jane Doe\n")
                .expect("sections_yaml has a name")
                + "  name: Jane Doe\n".len(),
            &format!("  gdpr: \"{gdpr}\"\n"),
        );
        yaml
    };

    // One-page CV: the statement sits on page 1 and nowhere else.
    let dir = unique_dir("fit-gdpr-one-page");
    fs::write(dir.join("cv.yaml"), with_gdpr(&[4])).expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 1, "got: {out}");
    assert!(
        pages[0].contains(gdpr),
        "the statement renders on the single page, got: {:?}",
        pages[0]
    );

    // Two-page CV: the statement moves to the last page with the content.
    let dir = unique_dir("fit-gdpr-two-pages");
    fs::write(
        dir.join("cv.yaml"),
        with_gdpr(&[13]) + "\ndesign:\n  pages: 2\n",
    )
    .expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    assert_eq!(pages.len(), 2, "got: {out}");
    assert!(
        pages[pages.len() - 1].contains(gdpr),
        "the statement renders on the last page, got: {:?}",
        pages
    );
    assert!(
        !pages[..pages.len() - 1]
            .iter()
            .any(|page| page.contains(gdpr)),
        "the statement must not repeat on earlier pages, got: {:?}",
        pages
    );
}

/// The number in "… spill onto N extra page(s)" of the fit-overflow warning.
fn reported_extra_pages(out: &str) -> usize {
    let start = out
        .find("spill onto ")
        .unwrap_or_else(|| panic!("no spill figure in: {out}"));
    let digits: String = out[start + "spill onto ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("the spill figure is a number")
}

/// ADR-0046: a section that fits one page is never cut in half — every section
/// of the fixture fits a page at the default sizes, so each section's heading
/// and its last entry share a page.
#[test]
fn a_section_that_fits_one_page_stays_whole() {
    let dir = unique_dir("fit-whole-sections");
    let yaml = sections_yaml(&[9, 9]) + "\ndesign:\n  pages: 2\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["build", "--no-fit", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");

    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("text");
    for section in 0..2 {
        let head = format!("Role {section}.0");
        let tail = format!("Role {section}.8");
        let head_page = pages.iter().position(|p| p.contains(&head));
        let tail_page = pages.iter().position(|p| p.contains(&tail));
        assert_eq!(
            head_page, tail_page,
            "Section {section} must live on one page: {:?} vs {:?}, pages: {pages:?}",
            head_page, tail_page
        );
        assert!(
            head_page.is_some(),
            "Section {section} must render, got: {out}"
        );
    }
}

/// `pages: 0` is not a document: a parse error that names the field.
#[test]
fn pages_zero_is_rejected_with_the_field_named() {
    let dir = unique_dir("fit-pages-zero");
    let yaml = cv_yaml(4, 1) + "\ndesign:\n  pages: 0\n";
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["check"]);
    assert!(!ok, "zero pages must fail the check, got: {out}");
    assert!(
        out.contains("design.pages"),
        "the error must name design.pages, got: {out}"
    );
}

/// The overflow warning is an essential (ADR-0026/0012): `--quiet` drops the
/// success line but keeps the warning.
#[test]
fn quiet_still_prints_the_overflow_warning() {
    let dir = unique_dir("fit-overflow-quiet");
    fs::write(dir.join("cv.yaml"), cv_yaml(60, 1)).expect("write yaml");

    let (ok, out) = run_cv(&dir, &["--quiet", "build", "--output", "cv.pdf"]);
    assert!(ok, "got: {out}");
    assert!(
        out.contains("content still overflows at the fit floor"),
        "warning survives --quiet, got: {out}"
    );
    assert!(
        !out.contains("cv.pdf  "),
        "the success line must be suppressed under --quiet, got: {out}"
    );
}
