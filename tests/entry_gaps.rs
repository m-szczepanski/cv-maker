//! Spacing-rhythm regression tests for the `compact` template's entries. Typst
//! collapses the spacing of adjacent blocks to its maximum, so a bare `entry_gap`
//! between entry blocks separated Experience entries by barely more than a line pitch
//! — entries blended into their own bullet lists, and the global paragraph spacing
//! floated each entry's heading, midline and bullets apart from one another. The
//! template doubles the block spacing (`between-entries`) and tightens spacing inside
//! an entry. These tests read span baselines from the PDF content stream and pin the
//! visible rhythm: the distance between entries must exceed BOTH the bullet pitch and
//! the heading→midline distance by a full design `entry_gap`, and must grow with the
//! design value.

use flate2::read::ZlibDecoder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default `design.spacing.entry_gap` in pt — the unit the assertions are stated in.
const ENTRY_GAP: f32 = 4.0;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

fn build_cv(dir: &Path, yaml: &str) -> Vec<u8> {
    fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        // `--no-fit` keeps the measurements at the designed rhythm: the fit
        // loop may grow a page this short (ADR-0053), and absolute pt
        // distances must not depend on where its search lands.
        .args(["build", "--no-fit"])
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "build failed: {text}");
    let pdf = fs::read(dir.join("cv.pdf")).expect("read pdf");
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf")).expect("parse pdf");
    assert_eq!(pages.len(), 1, "the tiny CV must fit one page");
    pdf
}

/// Two Experience entries with orgs and bullets and nothing else (no GDPR footer),
/// so the text-row sequence is fully predictable: name, section heading, then per
/// entry the heading row with dates, the midline row, and its bullet rows.
fn entries_yaml(extra_design: &str) -> String {
    format!(
        "schema: 1\ncv:\n  name: Jane Doe\n  gdpr: null\n  sections:\n    - title: Experience\n      type: entries\n      items:\n        - heading: Staff Engineer\n          org: Acme\n          dates: \"2021\"\n          bullets:\n            - One short achievement\n            - Another short achievement\n        - heading: Senior Engineer\n          org: Globex\n          dates: \"2016\"\n          bullets:\n            - Third short achievement\ndesign:\n  template: compact\n{extra_design}"
    )
}

/// Origins of every text span in the page-content streams, sorted top-to-bottom.
/// Typst emits each span as `q 1 0 0 -1 x y cm`, a y-flip whose translation puts
/// `y` at the span's baseline measured from the page top.
fn span_origins(pdf: &[u8]) -> Vec<(f32, f32)> {
    let mut origins = Vec::new();
    let mut from = 0;
    while let Some(rel) = find(pdf, b"stream\n", from) {
        let data_at = rel + 7;
        let Some(rel_end) = find(&pdf[data_at..], b"endstream", 0) else {
            break;
        };
        let mut inflated = Vec::new();
        from = data_at + rel_end + 9;
        if ZlibDecoder::new(&pdf[data_at..data_at + rel_end])
            .read_to_end(&mut inflated)
            .is_err()
        {
            continue; // not zlib: font or image stream
        }
        // Tagged text marks (`/MCID`) occur only in page-content streams.
        if !contains(&inflated, b"/MCID") {
            continue;
        }
        let text = String::from_utf8_lossy(&inflated);
        let mut pos = 0;
        while let Some(r) = text[pos..].find("q 1 0 0 -1 ") {
            let at = pos + r + "q 1 0 0 -1 ".len();
            let mut coords = text[at..]
                .split_whitespace()
                .take(2)
                .map(|v| v.parse::<f32>());
            if let (Some(Ok(x)), Some(Ok(y))) = (coords.next(), coords.next()) {
                origins.push((x, y));
            }
            pos = at;
        }
    }
    origins.sort_by(|a, b| b.1.total_cmp(&a.1));
    origins
}

/// Text rows: consecutive spans whose baselines lie within 2 pt form one row; the
/// row keeps its top baseline and the largest span origin — the dates column sits far
/// right (past 400 pt on A4), which identifies entry-heading rows.
fn rows(origins: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut rows: Vec<(f32, f32)> = Vec::new();
    for &(x, y) in origins {
        match rows.last_mut() {
            Some((top, max_x)) if (*top - y).abs() < 2.0 => *max_x = max_x.max(x),
            _ => rows.push((y, x)),
        }
    }
    rows
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle, 0).is_some()
}

/// Baseline distances `(between_entries, bullet_pitch, heading_to_midline)` from the
/// standard fixture. The layout is asserted row-by-row first, so the indices below
/// are stable.
fn entry_rhythm(yaml: &str, dir_name: &str) -> (f32, f32, f32) {
    let pdf = build_cv(&unique_dir(dir_name), yaml);
    let r = rows(&span_origins(&pdf));
    assert_eq!(r.len(), 9, "unexpected row layout: {r:?}");
    let heading_rows: Vec<usize> = (0..r.len()).filter(|&i| r[i].1 > 400.0).collect();
    assert_eq!(heading_rows, [2, 6], "rows 2 and 6 carry the dates column");
    (r[5].0 - r[6].0, r[4].0 - r[5].0, r[2].0 - r[3].0)
}

#[test]
fn compact_entries_sit_apart_by_more_than_their_own_bullets() {
    let (between_entries, bullet_pitch, heading_to_midline) =
        entry_rhythm(&entries_yaml(""), "gaps-compact");
    assert!(
        between_entries - bullet_pitch >= ENTRY_GAP,
        "the between-entry baseline distance ({between_entries:.1}pt) must exceed the \
         bullet pitch ({bullet_pitch:.1}pt) by a full design entry_gap ({ENTRY_GAP:.0}pt); \
         if it does not, entries blend into their bullet lists"
    );
    assert!(
        between_entries - heading_to_midline >= ENTRY_GAP,
        "the between-entry distance ({between_entries:.1}pt) must exceed the gap from an \
         entry's heading to its midline ({heading_to_midline:.1}pt) by a full entry_gap; \
         otherwise the midline reads as belonging to the next entry, not its own heading"
    );
}

#[test]
fn a_larger_entry_gap_widens_the_separation_between_entries() {
    let (base_between, _, _) = entry_rhythm(&entries_yaml(""), "gaps-compact-ref");
    let (wide_between, _, _) = entry_rhythm(
        &entries_yaml("  spacing:\n    entry_gap: 8pt\n"),
        "gaps-compact-8",
    );
    assert!(
        wide_between > base_between,
        "doubling the design entry_gap must widen the entry separation \
         (got {wide_between:.1}pt vs {base_between:.1}pt)"
    );
}
