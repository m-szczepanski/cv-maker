//! The spec §11 escaping class: Typst-special characters in user text must render as
//! literal text and never execute (spec §4, plan 2.6).
//!
//! The guarantee holds by construction (ADR-0016): user text reaches Typst only as
//! string values inside `data.yaml`, never as source. These tests pin the observable
//! consequence, because a construction can rot: if a template ever interpolated user
//! text into markup or `eval`ed it, the injected code below would run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-test-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

/// Every character Typst gives a meaning in source, plus constructs that would change
/// the document if parsed: a `#panic` (a failed compile), a `#set`/`#show` (restyled
/// output), and a raw `#calc` expression.
const HOSTILE_YAML: &str = r##"schema: 1
cv:
  name: "Jane # Doe $ _ \\ @ `tick`"
  headline: "#panic(\"injected\") #show heading: it => emoji"
  location: "Berlin $ #{ 1 + 1 }"
  contact:
    email: "jane@example.dev"
    links:
      - { label: "GitHub #1", url: "https://github.com/janedoe" }
  summary: >
    Bold **works**, and the rest stays literal: #hash $dollar _under \backslash
    @at 100% C# 2 * 3 = 6.
  sections:
    - title: "Skills # \\"
      type: entries
      items:
        - heading: "Engineer #2 *risky*"
          org: "Acme $ Corp"
          dates: "2021 – `present`"
          bullets:
            - "Wrote #let x = 1 and #panic(\"no\") in the runbook"
            - "Used $money, _slashes\\ and @handles"
    - title: "Escapes #1"
      type: bullets
      items:
        - "A line with #, $, *, _, \\, @ and `"
    - title: "Escapes #2"
      type: text
      items:
        - "Another with #emph{x} and `code` and 5 < 6 & 7 > 2."
design:
  template: compact
"##;

fn build(dir: &Path, out: &str) -> String {
    let yaml = dir.join("cv.yaml");
    fs::write(&yaml, HOSTILE_YAML).expect("write yaml");
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(["build", "--output", out])
        .current_dir(dir)
        .output()
        .expect("cv binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "hostile content must build, not fail: {stderr}"
    );
    let pdf = dir.join(out);
    let pages = pdf_extract::extract_text_by_pages(&pdf).expect("text extraction");
    assert_eq!(pages.len(), 1, "the document must still fit one page");
    pages.concat()
}

#[test]
fn typst_special_characters_render_as_literal_text() {
    let dir = unique_dir("escaping-literal");
    let text = build(&dir, "cv.pdf");
    for literal in [
        "Jane", "#panic", "$", "_", "\\", "@", "`tick`", "100%", "C#", "#hash",
    ] {
        assert!(
            text.contains(literal),
            "`{literal}` must appear as text, got:\n{text}"
        );
    }
    // Nothing was computed: an evaluated `#{ 1 + 1 }` would print `2`, never the source.
    assert!(
        text.contains("#{ 1 + 1 }"),
        "the code expression must stay literal, got:\n{text}"
    );
}

#[test]
fn injected_typst_code_never_runs() {
    let dir = unique_dir("escaping-injection");
    let text = build(&dir, "cv.pdf");
    // `#panic` ran: the build would have failed; `#show heading:` ran: the section
    // titles would be emoji. Both are asserted by the build succeeding plus the
    // literal source surviving in the extracted text.
    assert!(
        text.contains("#panic(\"injected\")"),
        "the panic call must be text, got:\n{text}"
    );
    assert!(
        text.contains("#show heading: it => emoji"),
        "the show rule must be text, got:\n{text}"
    );
    assert!(
        text.to_uppercase().contains("SKILLS") && text.to_uppercase().contains("ESCAPES"),
        "the section headings must keep their own titles, got:\n{text}"
    );
}

/// Markdown-enabled fields keep their markers working and everything else literal.
#[test]
fn markdown_still_works_around_the_special_characters() {
    let dir = unique_dir("escaping-markdown");
    let text = build(&dir, "cv.pdf");
    assert!(
        text.contains("works"),
        "the bold span must render, got:\n{text}"
    );
    assert!(
        !text.contains("**works**"),
        "the markers of a Markdown-enabled field must not leak, got:\n{text}"
    );
}
