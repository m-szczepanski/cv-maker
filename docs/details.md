# SPEC: `cv`, a code-first YAML to PDF CV tool (one page by default)

Audience: coding agents implementing this project, and the human reviewer.
Status: v0.1 draft. This file is the source of truth. If code and spec disagree, fix one of them in the same change and say which.

---

## 1. Purpose

Build a small, fast command-line tool for technical people who want to maintain their CV as code.

The user writes a single YAML file containing both **content** and **design settings**. The tool validates it and renders a clean **PDF** next to the YAML file — one page by default, `design.pages` pages when the user raises the target.

### Target user flow

```
cv init              # writes cv.yaml (content + design + template name) in the current directory
$EDITOR cv.yaml      # user edits in any text editor
cv build             # reads cv.yaml, writes cv.pdf in the same directory
```

### Goals

- Clean and lean: few commands, few dependencies, no runtime services.
- Code first: everything is configured in YAML, nothing in a GUI.
- Single static binary with templates and fonts embedded. Nothing else to install.
- Per-CV design configuration (fonts, sizes, margins, colors) in a separate `design` section of the same file.
- Template support: built-in templates plus user-supplied ones.
- Guaranteed fit within the target page count (auto-fit; one page by default, configurable with `design.pages`) with clear warnings when impossible.
- Guaranteed fill: when content fits the target with room to spare, the loop grows the layout so the pages are used, and a section that fits one page is never split across two (ADR-0044, ADR-0046).
- Great error messages: line-numbered, actionable.
- Real selectable text in PDFs, correct reading order, ATS-friendly.
- Deterministic: the same YAML and the same tool version produce the same PDF on every machine.

### Non-goals (for v1)

- No web UI, playground, or server.
- No free-flowing document length. The page count is set once in `design.pages`
  (default 1) and the fit loop works toward exactly that target; content beyond
  it overflows with a warning rather than growing the document silently.
  (Revised after v1: the original non-goal said "no multi-page CVs" — `design.pages`
  makes the count a deliberate user choice instead of a hard one-page limit.)
- No GUI template editor.
- No cloud features, accounts, or telemetry.
- No Markdown/HTML/DOCX output. PDF only.
- No i18n or locale-aware date formatting. Dates are free-form strings.

### Prior art (read, do not copy blindly)

RenderCV (YAML to Typst to PDF) exists. This project differs by: single Rust binary, first-class auto-fit to a chosen page count (one page by default), simple single-folder templates, and a schema generated from Rust types.

---

## 2. Tech stack

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust (stable, 2021 or 2024 edition) | Pin with `rust-toolchain.toml` |
| Rendering engine | **Typst** as a library (`typst`, `typst-pdf`) | Pin exact versions. No shelling out to a `typst` binary |
| Typst "World" glue | `typst-as-lib` if it supports the pinned Typst version; otherwise implement the `World` trait in a small, isolated module | See section 9 |
| Fonts | `typst-kit` for system font discovery, plus bundled open fonts embedded in the binary | Bundled fonts take priority for reproducibility |
| CLI | `clap` (derive API) | |
| YAML | `serde` + `serde_yaml_ng` | `serde_yaml` is archived; do not use it |
| Schema | `schemars` | JSON Schema is **derived from the Rust structs**; never hand-written |
| Errors | `miette` + `thiserror` | Source-span errors pointing into the user's YAML |
| Embedded assets | `include_dir` or `rust-embed` | Templates and fonts |
| Watch mode | `notify` | Debounced |
| Tests | `cargo test`, `insta` for snapshots, a PDF text extraction crate for assertions (e.g. `pdf-extract` or `lopdf`) | |

### Rules for dependencies

- Before adding any crate, justify it in the PR description in one sentence.
- Prefer crates that are actively maintained and widely used.
- **Do not recall Typst APIs from memory.** Typst's Rust API changes between releases. Read the docs.rs page for the exact pinned version before writing integration code.

### Bundled fonts (open licenses only)

Inter, IBM Plex Sans, IBM Plex Mono, Source Sans 3, JetBrains Mono. Record each font's license in `fonts/LICENSES.md`. Users can add more through `--font-path` or the `design.font_paths` field.

---

## 3. CLI specification

Binary name: `cv`.

| Command | Behavior |
|---|---|
| `cv init [--template <name>] [--out <file>]` | Writes a starter YAML (default `cv.yaml`) into the current directory. Refuses to overwrite an existing file unless `--force`. |
| `cv build [<file>]` | Builds `<file>` (default `cv.yaml`). Writes `<file-stem>.pdf` **in the same directory as the input file**. Exit code 0 on success, non-zero on any error. |
| `cv watch [<file>]` | Rebuilds on change of the YAML or its custom template files. Prints the result of each build. |
| `cv check [<file>]` | Validates only. No PDF written. |
| `cv templates` | Lists built-in templates with one-line descriptions. |
| `cv schema` | Prints the JSON Schema to stdout. |
| `cv fonts` | Lists fonts available to the tool (bundled and discovered). |

Global flags: `--font-path <dir>` (repeatable), `--quiet`, `--verbose`.

`cv build` flags: `--output <path>` overrides the output location, `--no-fit` disables the fitting loop for this run.

### Output rules

- Default PDF path: same directory and file stem as the input (`cv.yaml` gives `cv.pdf`).
- Overwrite the PDF silently if it exists.
- Write atomically (write to a temp file in the same directory, then rename) so a failed build never leaves a corrupt PDF.
- Success output is one line, e.g. `cv.pdf  1 page  fit: scale 0.94  (118 ms)`.
- Exit codes: `0` on success — including a build that wrote an overflowing PDF and
  warned about it (§7 step 4, ADR-0026) — `1` on any error, `2` when the command line
  itself is malformed (clap's convention, documented in the README).

---

## 4. Input file format

One YAML file per CV with these top-level keys:

- `schema`: integer, schema version. Starts at `1`. Required.
- `cv`: the content.
- `design`: presentation settings. Optional; every field has a default.

`cv init` writes a first line `# yaml-language-server: $schema=<path-or-url>` so editors give autocomplete and inline errors. The schema must be a file the init command can also write next to the YAML (`cv.schema.json`) so it works offline.

### Example

```yaml
# yaml-language-server: $schema=./cv.schema.json
schema: 1

cv:
  name: Jane Doe
  headline: Staff Software Engineer
  location: Berlin, Germany
  contact:
    email: jane@example.dev
    phone: "+49 000 000000"
    links:
      - { label: GitHub, url: "https://github.com/janedoe" }
      - { label: Website, url: "https://janedoe.dev" }

  summary: >
    Backend engineer focused on high-throughput systems and developer tooling.

  gdpr: >-
    I agree that my personal data may be stored and processed for the purpose
    of this application.     # optional; the default is the standard consent clause, `null` removes the footer

  sections:
    - title: Experience
      type: entries
      items:
        - heading: Staff Engineer
          org: Acme Corp
          location: Remote
          dates: 2021 – present
          bullets:
            - Cut p99 latency by 40% by moving the hot path to Rust
            - Led a 6-person team through a zero-downtime migration
    - title: Skills
      type: key_values
      items:
        - { key: Languages, value: "Rust, Go, Python, TypeScript" }
        - { key: Infrastructure, value: "Kubernetes, Postgres, Kafka" }
    - title: Education
      type: entries
      items:
        - heading: BSc Computer Science
          org: University of Example
          dates: 2012 – 2016

design:
  template: compact
  page: a4                 # a4 | letter
  pages: 1                 # target page count; raise for a longer CV
  margins: { x: 1.5cm, y: 1.2cm }
  fonts:
    body: Inter
    heading: Inter
    mono: JetBrains Mono
  sizes:
    name: 20pt
    heading: 11pt
    body: 9.5pt
    small: 8.5pt
  spacing:
    section_gap: 8pt
    entry_gap: 4pt
    line_height: 1.25
  colors:
    accent: "#2563eb"
    text: "#111111"
    muted: "#555555"
  fit_one_page: true
  fit_floor:
    min_body_size: 8.5pt
    min_scale: 0.85
  font_paths: []           # extra directories with .ttf/.otf files
```

### Content model

- Section `type` values for v1: `entries` (heading/org/location/dates/bullets), `key_values` (skills style), `bullets` (flat list), `text` (paragraph).
- The set of section types is part of the schema contract and is closed in v1. Templates must support all of them.
- Bullet and text fields support a **small inline Markdown subset**: `**bold**`, `*italic*`, `` `code` ``, and `[label](url)`. Anything else is literal text. Conversion to Typst must escape all Typst-special characters in user text. User text must never be able to inject Typst code.
- Dates are free-form strings.
- `cv.gdpr` (ADR-0045) carries the consent statement, always literal text (no Markdown), the user's own wording. It defaults to the standard recruitment-consent clause (`DEFAULT_GDPR`); `gdpr: null` opts out. Built-in templates render it small and muted at the bottom of the last page; custom templates decide for themselves (`data.yaml` always carries the value, `null` when opted out).
- Unknown keys are an error (`deny_unknown_fields`), with a "did you mean" hint where practical.

### Design defaults

Every `design` field has a default defined in Rust. A missing `design` section is valid. Defaults must produce an attractive, fits-on-one-page-if-reasonable result with the default template. `pages` is a positive integer (`pages: 0` is a parse error at the value) and defaults to `1`.

---

## 5. Validation and errors

1. Parse YAML with spans retained.
2. Deserialize into typed Rust structs (`schemars`-derivable).
3. Run semantic checks the types cannot express: font family exists, template exists, size/margin values parse, colors are valid hex, URLs are well formed.
4. Report **all** errors found, not just the first, using `miette` with the YAML source and a labeled span. This includes a font directory that cannot be read: it is one more thing wrong with the input, not a reason to stop checking it.
5. Severity follows what the renderer does (ADR-0033): an inline Markdown link with an **empty** target is an error, because the PDF export refuses it; a target no reader can resolve (a bare host, a path relative to the PDF) warns, because it renders; any absolute URL (`https:`, `mailto:`) is accepted. `cv.contact.links` stay on the stricter ADR-0017 rule.

Example of the quality bar:

```
Error: unknown font "Inte"
   ╭─[cv.yaml:34:11]
34 │     body: Inte
   ·           ──┬─
   ·             ╰── did you mean "Inter"?
   ╰─ run `cv fonts` to list available fonts
```

Errors from the Typst compiler (template bugs) must be reported separately from user errors and clearly labeled as template errors.

---

## 6. Templates

### Structure

A template is a folder:

```
templates/<name>/
  template.typ       # entry point
  manifest.yaml      # name, description, supported design options, schema version
  assets/            # optional (not fonts)
```

### Contract

- The tool validates the user's YAML, applies defaults, and makes the fully resolved data available to Typst as a virtual file (for example `data.yaml`, or JSON). The template reads it with Typst's built-in `yaml()` or `json()`. **The tool does not generate Typst source from user data.** This avoids escaping bugs.
- Inline Markdown in text fields is converted by the template through a shared Typst helper (`lib/inline.typ`), or pre-parsed by the tool into a small structured form. Decide in milestone 2 and record the decision in `docs/decisions.md`.
- The template receives `cv` (content) and `design` (fully resolved, no missing values) and a `fit` object with the current scale factors used by the fit loop.
- A template must honor all `design` fields listed in its `manifest.yaml`. Fields it ignores must be declared as unsupported so `cv check` can warn.
- Templates are pure: no network access, no reading outside their folder.

### Built-in templates for v1

1. `compact`: dense, single column, ideal for experienced engineers.
2. `classic`: conventional single-column layout — name with the contact block on the
   right, title-case accent headings. No rules or dividers (ADR-0030): the built-ins are
   clean, minimal and slick, and hierarchy comes from type, color and spacing.
3. (Optional) `sidebar`: narrow left column for skills and contact.

Both built-ins follow two page-layout rules (ADR-0046, ADR-0045):

- **A section that fits one page stays whole**: its height is measured against the
  content area (Typst's `measure` inside `context`) and, when it fits, the section is
  wrapped in an unbreakable block so Typst moves the whole section to the next page
  instead of cutting a list in half. A section taller than a page must flow; its
  heading is `sticky` so it never strands at a page bottom.
- **`cv.gdpr` renders at the bottom of the last page**: stretchable space (`v(1fr)`)
  above the statement pins it to the bottom margin — the last page of a multi-page
  CV, the only page of a one-page CV.

### Custom templates

`design.template` accepts either a built-in name or a relative path to a template folder (`./my-template`). Custom templates use the same contract. `cv watch` also watches the custom template folder.

---

## 7. Fitting to the page count

When `design.fit_one_page` is `true` (default) and the first compile produces more
than `design.pages` pages (default 1):

1. Tighten spacing in steps: `spacing.*` and margins down to a minimum (defaults: spacing scale 0.6, margins not below 1.0 cm).
2. Then scale font sizes down in small steps (for example 0.02 per step) until it fits or the floor is reached (`fit_floor.min_body_size` or `min_scale`, whichever is stricter).
3. Pick the **least aggressive** setting that fits. Prefer a binary search over a linear walk to limit compile count.
4. If the content still does not fit at the floor: write the PDF anyway, exit code 0 or a distinct documented exit code (decide and document), and print a clear warning stating how many lines overflow — measured against the `design.pages` target — and suggesting what to change (cut content, raise `design.pages`, or relax `fit_floor` / margins).

When the first compile already fits and the target is more than one page, the loop
instead **fills** (ADR-0044): content spread thinly over the target's pages looks
broken, so the loop grows — in bounded searches, each entered only when the previous
one ended at its cap with the document still fitting — `spacing_scale`, then
`font_scale`, then `margin_scale`, each to the largest value whose compile still fits
within the target. The caps (spacing 3.0, font 1.25, margins 1.4) are sanity limits:
content far below the target cannot fill it gracefully at any setting. A one-page
target never fills: its layout stays the top-flow identity.

Hard requirements:

- The loop must have a maximum iteration count. No unbounded loops. Worst case:
  13 compiles on the tighten path, 19 on the fill path (the two are mutually exclusive).
- Never go below the floor.
- Never shrink if the first compile already fits within `design.pages`.
- The result is deterministic.
- Fit scale factors are passed to the template through the `fit` object; templates multiply their own sizes and spacings by them.
- The final output line reports the applied scales.
- The fit loop measures the whole document, `cv.gdpr` footer included: a full page
  tightens rather than pushing the footer to a new page.

---

## 8. Fonts

- Bundled fonts are embedded in the binary. System fonts are available as a fallback.
- Resolution order: `design.font_paths` and `--font-path` directories, then bundled, then system.
- Missing font: error with suggestions (see section 5), never a silent fallback to a different family.
- The PDF must embed fonts (subsetting is fine) so it renders identically elsewhere.

---

## 9. Typst integration notes

- This is the highest-risk module. Keep it isolated in `src/render/` behind a small interface:

```rust
pub struct RenderRequest { /* template, resolved data, fit params, font config */ }
pub struct RenderOutput { pub pdf: Vec<u8>, pub page_count: usize, pub warnings: Vec<Warning> }
pub fn render(req: &RenderRequest) -> Result<RenderOutput, RenderError>;
```

- The rest of the codebase must not import Typst types.
- Compile in memory; do not write temporary files for the document itself.
- Reuse the compiler state across fit-loop iterations where possible (fonts load once).
- PDF settings: deterministic output (fixed or absent creation timestamp unless explicitly set), tagged PDF if the pinned `typst-pdf` supports it, correct document title and author metadata from `cv.name`.

---

## 10. Repository layout

```
.
├── Cargo.toml
├── rust-toolchain.toml
├── SPEC.md
├── AGENTS.md                  # short working rules, points to this file
├── docs/decisions.md          # short ADR-style log
├── fonts/                     # bundled fonts + LICENSES.md
├── templates/
│   ├── compact/
│   └── classic/
├── examples/
│   ├── minimal.yaml
│   └── full.yaml
├── src/
│   ├── main.rs
│   ├── cli.rs                 # clap definitions
│   ├── model/                 # serde + schemars structs, defaults
│   ├── validate/              # semantic checks, miette diagnostics
│   ├── templates/             # loading built-in and custom templates
│   ├── render/                # Typst integration (isolated)
│   ├── fit.rs                 # fitting loop to the target page count
│   └── commands/              # init, build, watch, check, templates, schema, fonts
└── tests/
    ├── build_examples.rs
    ├── errors.rs
    └── snapshots/
```

---

## 11. Quality gates and testing

Every change must pass, and the agent must run them before declaring a task done:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Required tests:

- **Example builds:** every file in `examples/` builds with every built-in template, produces exactly one page, and contains the expected text (extract text from the PDF and assert on name, a bullet, a heading).
- **Schema:** `cv schema` output matches a committed snapshot; the example files validate against it.
- **Errors:** snapshot tests for common mistakes (unknown key, bad font, bad color, missing required field, wrong type) asserting the line and column.
- **Escaping:** content with `#`, `$`, `*`, `_`, `\`, `@`, backticks, and Typst-looking code must render as literal text and never execute.
- **Fit loop:** an overflowing CV fits after fitting; an impossible CV hits the floor and warns; a short CV is not shrunk.
- **Determinism:** two builds of the same input produce byte-identical PDFs.
- **Atomic write:** a failed build leaves any existing PDF untouched.

---

## 12. Milestones and acceptance criteria

Complete in order. Each milestone ends with a working, tested, reviewable state. Stop and wait for review after each one.

### M1: Skeleton and first PDF

- Cargo project, the Gates, `AGENTS.md`.
- Model structs, YAML parsing, `cv init` writes a valid starter file.
- One template (`compact`), `cv build` writes `cv.pdf` next to the YAML.
- **Accept:** the flow `cv init`, `cv build` produces a one-page PDF with selectable text.

### M2: Validation and errors

- `schemars` schema, `cv schema`, `cv check`, `miette` diagnostics for all error classes in section 5.
- Inline Markdown subset and escaping tests.
- **Accept:** the error snapshot tests pass; the editor-schema header works in VS Code.

### M3: Design block

- All `design` fields wired through to the template: fonts, sizes, spacing, margins, colors, page size.
- Bundled fonts embedded; `cv fonts`; `--font-path`.
- **Accept:** changing each design field in YAML visibly changes the PDF; snapshot tests cover this.

### M4: One-page fit

- Fit loop per section 7, `--no-fit`, warnings.
- **Accept:** all fit-loop tests pass; build time for a typical CV stays under 500 ms on a normal laptop.

### M5: Templates and watch

- `classic` template, custom template path, manifest checks, `cv templates`, `cv watch`.
- **Accept:** a user can copy a built-in template folder, edit it, and use it via `design.template: ./my-template`.

### M6: Polish and release

- README with a 60-second quick start, example outputs, documented exit codes.
- Release builds for Linux, macOS, Windows (produced locally; see decisions ADR-0013 — no CI).
- **Accept:** a fresh machine can run the quick start using only the downloaded binary.

---

## 13. Working agreements for agents

1. Read this spec and `AGENTS.md` before starting. Re-read the relevant section before each milestone.
2. Work on one milestone at a time. Do not start the next one without review.
3. Keep changes small and focused. Prefer several small commits to one large one.
4. When the spec is ambiguous, pick the simplest option that fits the goals, document it in `docs/decisions.md`, and flag it for the reviewer. Do not silently expand scope.
5. Do not add features outside this spec. Propose them in the PR description instead.
6. Never invent Typst or crate APIs. Check the docs for the pinned version, or compile a small experiment.
7. Keep Typst types inside `src/render/`.
8. No `unwrap()` or `expect()` outside tests and `main` startup, unless commented with the invariant.
9. Every user-facing error must have a test.
10. Report at the end of each task: what changed, what was tested, what is uncertain.

### Areas that need extra scrutiny from the human reviewer

- The Typst integration layer (`src/render/`).
- The fit loop (termination, floor, determinism).
- Template visual quality. Passing tests do not mean a template looks good; render and look at it.

---

## 14. Open questions (resolve and record in `docs/decisions.md`)

- Exit code when content cannot fit on one page at the floor: 0 with warning, or a dedicated non-zero code?
- Inline Markdown: convert in the tool (structured output) or in Typst helpers?
- Tagged PDF and accessibility: how much does the pinned `typst-pdf` support?
- Should `cv init` offer example content tuned for different roles (backend, frontend, data), or just one neutral example?
- Per-job variants of one base CV (for example `cv build --variant backend`): out of scope for v1, but keep the model from blocking it.
