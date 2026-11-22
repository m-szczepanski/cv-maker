//! Layout-geometry regression tests for the color-panel built-ins. Text extraction
//! cannot see a misplaced background — and a `page(background)` rect that loses its
//! edge pinning renders centered on the page (the marginals are laid out with
//! `center + horizon` alignment, see docs/decisions.md ADR-0047) — so these tests
//! read the PDF content streams and assert the drawn rectangles' coordinates.

use flate2::read::ZlibDecoder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const A4_W: f32 = 595.276;
const A4_H: f32 = 841.890;
/// Default `design.margins.x` in points (1.5 cm) and the body measure it leaves.
const MARGIN_X: f32 = 42.52;
const BODY_W: f32 = A4_W - 2.0 * MARGIN_X;

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

/// Two pages of entries plus a rail-friendly section: the sidebar shapes must span
/// several pages so the per-page background repeat is exercised.
fn long_cv(template_ref: &str, pages: u32) -> String {
    let mut sections = String::new();
    for s in 0..4 {
        sections.push_str(&format!(
            "    - title: Experience {s}\n      type: entries\n      items:\n"
        ));
        for i in 0..6 {
            sections.push_str(&format!(
                "        - heading: Role {s}.{i}\n          org: Org {s}.{i}\n          bullets:\n            - Achievement number {s}.{i} with realistic wording that fills the line\n"
            ));
        }
    }
    format!(
        "schema: 1\ncv:\n  name: Jane Doe\n  contact:\n    email: jane@example.dev\n  sections:\n{sections}    - title: Skills\n      type: key_values\n      items:\n        - {{ key: Languages, value: \"Rust, Go\" }}\ndesign:\n  template: {template_ref}\n  pages: {pages}\n"
    )
}

/// Bounding boxes of every filled vector path in the PDF's page-content streams.
/// Typst draws rects as corner paths (`m`/`l` … `h` `f`), not `re` operators, and
/// marks the background with a tagged `/Artifact … /Type/Background` block, so the
/// vertex sets of page streams are the ground truth for what was painted where.
fn drawn_paths(pdf: &[u8]) -> Vec<(f32, f32, f32, f32)> {
    fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        if hay.len() < from + needle.len() {
            return None;
        }
        (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
    }

    let mut boxes = Vec::new();
    let mut from = 0;
    while let Some(stream_at) = find(pdf, b"stream", from) {
        let data_at = stream_at + 6;
        let Some(rel_end) = find(&pdf[data_at..], b"endstream", 0) else {
            break;
        };
        let mut data = &pdf[data_at..data_at + rel_end];
        // The spec puts an EOL after the `stream` keyword; trim it before inflating.
        data = match data.first() {
            Some(b'\r') => &data[2.min(data.len())..],
            Some(b'\n') => &data[1..],
            _ => data,
        };
        from = data_at + rel_end + 9; // past the `endstream` keyword itself
        let mut inflated = Vec::new();
        if ZlibDecoder::new(data).read_to_end(&mut inflated).is_err() {
            // Not a zlib stream: embedded font or image, no content operators.
            continue;
        }
        // Page-content streams carry tagged text with `/MCID`; font and image
        // streams do not. (A `BDC` mark alone is too broad: glyph definitions use it.)
        if !contains(&inflated, b"/MCID") {
            continue; // not a page-content stream
        }
        // PDF tokens can abut without whitespace (`cm/c0`, `EMC/Span`,
        // `<</MCID 0>>`), and binary string arguments `(...)` must be dropped, so
        // lex properly before walking.
        let mut points: Vec<(f32, f32)> = Vec::new();
        let mut nums: Vec<f32> = Vec::new();
        let mut ctm = Affine::IDENTITY;
        let mut stack: Vec<Affine> = Vec::new();
        for token in lex_content(&inflated) {
            if let Ok(value) = token.parse::<f32>() {
                nums.push(value);
                continue;
            }
            match (token.as_str(), nums.as_slice()) {
                ("m", [.., x, y]) => {
                    points.clear();
                    points.push(ctm.apply(*x, *y));
                }
                ("l", [.., x, y]) => points.push(ctm.apply(*x, *y)),
                ("cm", [a, b, c, d, e, f]) => ctm = Affine::new(*a, *b, *c, *d, *e, *f).then(ctm),
                ("q", _) => stack.push(ctm),
                ("Q", _) => ctm = stack.pop().unwrap_or(Affine::IDENTITY),
                ("f" | "f*", _) => {
                    if let Some(b) = bbox(&points) {
                        boxes.push(b);
                    }
                    points.clear();
                }
                _ => {}
            }
            nums.clear();
        }
    }
    boxes
}

/// Split a PDF content stream into operator/number/name tokens: string literal
/// bytes are discarded (nested parens and escapes honored) and the delimiters
/// `/ < > [ ] ( )` always end a token, so `cm/c0`, `BBox[0 0]` and `EMC/Span`
/// lex the way a PDF reader sees them.
fn lex_content(data: &[u8]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut depth: usize = 0;
    let mut escaped = false;
    for &byte in data {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    in_string = depth > 0;
                }
                b'\\' => escaped = true,
                _ => {}
            }
            continue;
        }
        match byte {
            b'(' => {
                in_string = true;
                depth = 1;
                flush(&mut current, &mut tokens);
            }
            byte if byte.is_ascii_whitespace()
                || matches!(byte, b'/' | b'<' | b'>' | b'[' | b']') =>
            {
                flush(&mut current, &mut tokens);
            }
            _ => current.push(byte as char),
        }
    }
    flush(&mut current, &mut tokens);
    tokens
}

fn flush(current: &mut String, tokens: &mut Vec<String>) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

/// The PDF transform `[a b 0; c d 0; e f 1]`: points are row vectors (`p' = p·M`)
/// and `cm` pre-concatenates (`M' = C·M`), so a translated y-flip maps `y=0` to
/// the bottom of the page rather than to `height + translation`.
#[derive(Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// `self` applied before `next` (row-vector composition).
    fn then(self, next: Self) -> Self {
        Self {
            a: self.a * next.a + self.b * next.d,
            b: self.a * next.b + self.b * next.c,
            c: self.c * next.a + self.d * next.c,
            d: self.c * next.b + self.d * next.d,
            e: self.e * next.a + self.f * next.c + next.e,
            f: self.e * next.b + self.f * next.d + next.f,
        }
    }

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (
            x * self.a + y * self.c + self.e,
            x * self.b + y * self.d + self.f,
        )
    }
}

fn bbox(points: &[(f32, f32)]) -> Option<(f32, f32, f32, f32)> {
    if points.len() < 2 {
        return None;
    }
    let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The rail candidate: a rect nearly as tall as the page, narrow, full color.
fn rails(boxes: &[(f32, f32, f32, f32)]) -> Vec<(f32, f32, f32, f32)> {
    boxes
        .iter()
        .copied()
        .filter(|(_x, _y, w, h)| *h > 0.95 * A4_H && *w > 100.0 && *w < 320.0)
        .collect()
}

#[test]
fn the_sidebar_rail_is_flush_with_the_left_page_edge_on_every_page() {
    let dir = unique_dir("geom-sidebar");
    fs::write(dir.join("cv.yaml"), long_cv("sidebar", 2)).expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build"]);
    assert!(ok, "build failed: {out}");
    let paths = drawn_paths(&fs::read(dir.join("cv.pdf")).expect("read pdf"));
    let pages = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("text")
        .len();
    let found = rails(&paths);
    assert!(
        found.len() >= pages,
        "one full-height rail rect per page ({} pages), got {}",
        pages,
        found.len()
    );
    for (x, _y, w, _h) in &found {
        assert!(
            *x < 5.0,
            "rail drawn at x={x:.1} (width {w:.1}) — it must be flush with the page's \
             left edge; a large x means the background lost its edge pinning and Typst \
             centered it in the page"
        );
    }
}

#[test]
fn the_diy_left_rail_is_flush_left_and_the_right_rail_flush_right() {
    for (layout, left) in [("sidebar-left", true), ("sidebar-right", false)] {
        let dir = unique_dir(&format!("geom-diy-{layout}"));
        let (ok, out) = run_cv(&dir, &["templates", "--extract", "diy"]);
        assert!(ok, "extract failed: {out}");
        let cfg = dir.join("diy/config.typ");
        let original = fs::read_to_string(&cfg).expect("read config");
        fs::write(
            &cfg,
            original.replace(r#"layout: "single""#, &format!(r#"layout: "{layout}""#)),
        )
        .expect("set layout knob");
        fs::write(dir.join("cv.yaml"), long_cv("./diy", 1)).expect("write yaml");
        let (ok, out) = run_cv(&dir, &["build"]);
        assert!(ok, "{layout} build failed: {out}");
        let paths = drawn_paths(&fs::read(dir.join("cv.pdf")).expect("read pdf"));
        let found = rails(&paths);
        assert!(!found.is_empty(), "{layout}: no rail rect drawn");
        for (x, _y, w, _h) in &found {
            if left {
                assert!(*x < 5.0, "{layout}: rail at x={x:.1}, expected flush left");
            } else {
                let right_edge = x + w;
                assert!(
                    right_edge > A4_W - 5.0,
                    "{layout}: rail spans {x:.1}..{right_edge:.1}, expected flush with \
                     the right edge ({A4_W:.1})"
                );
            }
        }
    }
}

#[test]
fn the_modern_band_spans_the_body_measure_from_the_left_margin() {
    // Pins the current, deliberate geometry: a panel over the text measure, not a
    // page-width and not a centered strip. `--no-fit` keeps the margins at the
    // defaults the coordinates assert: since ADR-0053 the fill loop grows
    // margins on a fitting one-page document too, which would move the band
    // with them. If the geometry ever changes, this test must change with it
    // and the ADR record why.
    let dir = unique_dir("geom-modern");
    fs::write(
        dir.join("cv.yaml"),
        "schema: 1\ncv:\n  name: Jane Doe\n  headline: Engineer\n  contact:\n    email: jane@example.dev\n  sections:\n    - title: Work\n      type: entries\n      items:\n        - heading: Staff\n          org: Acme\n          dates: \"2020\"\ndesign:\n  template: modern\n",
    )
    .expect("write yaml");
    let (ok, out) = run_cv(&dir, &["build", "--no-fit"]);
    assert!(ok, "build failed: {out}");
    let paths = drawn_paths(&fs::read(dir.join("cv.pdf")).expect("read pdf"));
    let bands: Vec<_> = paths
        .iter()
        .filter(|(x, _y, w, h)| {
            (40.0..50.0).contains(x)
                && (BODY_W - 2.0..BODY_W + 2.0).contains(w)
                && (40.0..300.0).contains(h)
        })
        .copied()
        .collect();
    assert!(
        !bands.is_empty(),
        "a band starting at the left margin ({MARGIN_X:.1}pt) and {BODY_W:.1}pt wide \
         must be drawn; paths seen: {paths:?}"
    );
}
