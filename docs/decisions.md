# Decisions

ADR-style log. Each entry: context, decision, consequences. The spec (`docs/details.md`) is
the source of truth; if a decision here contradicts it, fix one of them and say which
(spec header note).

## ADR-0001 — Toolchain: stable channel, edition 2024

- **Context:** Spec §2 says "Rust (stable, 2021 or 2024 edition), pin with `rust-toolchain.toml`".
- **Decision:** `rust-toolchain.toml` pins `channel = "stable"` (not an exact version); the
  crate uses edition 2024.
- **Rationale:** Pinning an exact rustc version would force manual toolchain bumps for routine
  fixes and is not needed for reproducibility (the PDF determinism requirement is about inputs,
  not rustc versions). Edition 2024 is fully stable and avoids a later migration.
- **Flag for reviewer:** If you want a hermetic exact-version pin (e.g. `1.99.1`), say so and
  we change one line.

## ADR-0002 — Embedded assets: `include_dir` (not `rust-embed`)

- **Context:** Spec §2 offers `include_dir` or `rust-embed` for embedding templates and fonts.
- **Decision:** `include_dir`.
- **Rationale:** Simpler dependency (no proc-macro beyond its own), and identical behavior in
  debug and release builds. `rust-embed` re-reads files from disk in debug builds, which would
  make template/font behavior differ between dev and release builds — a determinism hazard for
  a tool whose selling point is byte-identical output.

## ADR-0003 — Spec location: `docs/details.md` (no root `SPEC.md` copy)

- **Context:** Spec §10 layout shows a root `SPEC.md`.
- **Decision:** The spec lives at `docs/details.md`; we do not create a duplicate root copy.
- **Rationale:** Two copies of a "source of truth" drift. If a root `SPEC.md` is wanted later,
  it should be a symlink or generated copy, not a second editable file.

## ADR-0004 — CLI shape testable from M0 via `env!("CARGO_BIN_EXE_cv")`

- **Context:** Plan 0.4: stubs must be real enough that the CLI shape is testable from day one.
- **Decision:** All commands from spec §3 exist as clap subcommands in M0. Unimplemented ones
  print `Error: cv <cmd> is not implemented yet` and exit 1. An integration test invokes the
  real binary via `env!("CARGO_BIN_EXE_cv")` and asserts on the help surface and stub exit codes.
- **Consequences:** The CLI shape test exercises the actual binary surface from M0 on.

## ADR-0005 — Typst integration via `typst-as-lib` 0.17; Typst stack pinned 0.15.1 (plan 1.4.1)

- **Context:** Plan 1.4.1 requires choosing a Typst integration approach and pinning versions
  before writing render code. Options: hand-roll a `typst::World` implementation, or use
  `typst-as-lib` as the glue layer.
- **Decision:** Use `typst-as-lib 0.17` with `typst =0.15.1`, `typst-pdf =0.15.1`,
  `typst-kit =0.15.1` (exact pins). The `typst-kit-fonts` feature of typst-as-lib provides
  system font discovery (spec §8).
- **Rationale:** Verified on crates.io: typst-as-lib 0.17.0 declares `typst ^0.15`, while its
  0.15.x line targets typst ^0.14 (incompatible with the pinned typst 0.15.1). Hand-rolling the
  `World`/`FileResolver` plumbing is ~200 lines of version-fragile code for no benefit at this
  stage; if it ever fights us we can swap it for a hand-rolled `World` behind
  `src/render/` (Typst types are already confined there, spec §13.7).
- **Consequences:** `TypstEngine` builds an in-memory world from (virtual path, source) pairs,
  a static `data.yaml` file, and a font list. Fonts order: `.fonts(...)` items first, then
  typst-kit-discovered fonts — see ADR-0007.

## ADR-0006 — Data contract: templates read virtual `data.yaml`; fit scales via `sys.inputs`

- **Context:** Spec §6/§9: the tool must not generate Typst source from user data; templates
  receive data through a file. Ambiguity: how do design values (lengths, colors) and the fit
  loop's scale factors reach the template?
- **Decision:** The tool serializes `cv` + fully resolved `design` + `fit` scales into a
  virtual `data.yaml` read via Typst's `yaml()`. Lengths are structured `{value, unit}`
  objects (Typst has no string→length conversion), colors are `#rrggbb` strings, page is
  `"a4"`/`"letter"`. Fit scales arrive via `sys.inputs.fit` (`{font_scale, spacing_scale,
  margin_scale}`) so the M4 fit loop can recompile with new inputs without touching the data
  file.
- **Rationale:** YAML keeps the template contract symmetric with user YAML; structured lengths
  avoid string parsing in templates; `sys.inputs` is the sanctioned Typst input mechanism and
  keeps fit iterations cheap (input change only).
- **Consequences:** `src/render/data.rs` defines the exact template-facing schema. Template
  authors see only resolved values (no defaults logic, no user-facing string forms).

## ADR-0007 — Font priority: bundled → user dirs → system (plan 1.4.3, spec §2 vs §8)

- **Context:** Spec §2 wants reproducible output ("open-license fonts bundled"), while spec §8
  also wants user font dirs and system font discovery. A same-named font in two sources makes
  the winner matter.
- **Decision:** Font book order: user `--font-path`/`design.font_paths` fonts first (explicit
  user intent wins), then bundled fonts, then system fonts (via typst-kit).
- **Rationale:** Explicit user overrides are the only case where a user *means* to override;
  bundled fonts beat same-named system fonts so output stays reproducible across machines
  (a machine with a different system Inter must not change the PDF). System fonts fill in
  families the user references that we don't bundle (e.g. custom corporate fonts).
- **Consequences:** With default settings (no user dirs), output is deterministic across
  machines as long as the referenced families are bundled.

## ADR-0008 — Inline Markdown (§14 open question): plain text in M1; revisit in M2

- **Context:** Spec §14: inline Markdown in headings/bullets — tool-structured vs. Typst
  helper. Plan 1.5.2 asks for the M1 decision.
- **Decision:** M1 renders user text as plain text (no Markdown parsing). Direction for M2:
  the *tool* pre-parses inline Markdown into a small typed structure that `data.yaml` carries
  and templates interpret with a Typst helper, keeping user source out of Typst code context.
- **Rationale:** Passing raw Markdown into Typst markup would require generated Typst source
  (spec §6 forbids) or `eval()` (dangerous, and requires the `html`/`eval` feature). A tool-
  side parse keeps templates declarative and user input inert.
- **Flag for reviewer:** Confirm this direction at the M1 review; M2 implements it.

## ADR-0009 — Tagged PDFs on (spec §14 open question, step 1.4.5)

- **Context:** §14 asks whether to use typst-pdf's tagged-PDF support. Checked against
  typst-pdf 0.15.1 sources: `PdfOptions::tagged` (default `true`) emits a tagged PDF with
  structure tree.
- **Decision:** Keep `tagged: true` for M1. Accessibility is a spec goal (§14); PDF/A or
  PDF/UA standards selection is deferred to M6, where `PdfStandards` is finalized.
- **Consequences:** Slightly larger PDFs; no known downside for the one-page use case.

## ADR-0010 — `Section` cannot carry `deny_unknown_fields` (serde limitation)

- **Context:** Spec §13.10-style strictness: every model struct should reject unknown keys.
  `Section` uses `#[serde(flatten)] SectionBody` with an internally tagged enum; serde's
  flatten is incompatible with `deny_unknown_fields` (the tag key `type` is consumed by the
  flattened enum, so `deny_unknown_fields` rejects valid input — verified by test).
- **Decision:** Omit `deny_unknown_fields` on `Section` only. Every other struct keeps it;
  the flattened enum's variant structs still reject unknown item keys, and the unknown-tag
  case is rejected by the closed variant set.
- **Rationale:** Alternative (custom Deserialize for Section) is disproportionate; the
  strictness loss is confined to the `title`+`type` mapping keys.

## ADR-0011 — `cv init` template validation + `--force`

- **Context:** Plan 1.3: `init` writes the starter YAML; spec §3 defines `--template`,
  `--out`, `--force`.
- **Decision:** `init` validates `--template` against the built-in set *before* writing
  (giving "did you mean" hints), refuses to overwrite an existing file without `--force`, and
  writes a fixed starter demonstrating every section type and all design knobs. The starter's
  YAML comment points at the (future) JSON Schema for editor tooling.
- **Consequences:** `cv.schema.json` referenced in the comment does not exist until M2
  (`cv schema`); the comment is aspirational until then — harmless for editors.

## ADR-0012 — `--quiet` suppresses the success line; warnings go to stderr

- **Context:** Spec §3 defines global `--quiet` ("suppress progress output; print only
  essentials") and plan 1.6.3 defines the success line format
  `<output> <n> page(s) fit: scale x.xx (<ms> ms)`.
- **Decision:** The success line goes to stdout and is suppressed under `--quiet`; Typst
  warnings (e.g. unknown font family) go to stderr prefixed `warning:` and are *not*
  suppressed (they are essentials, not progress).
- **Rationale:** Keeps stdout parseable when scripting; stderr keeps diagnostics out of
  redirected output.

## ADR-0013 — No CI/CD pipeline (dropped plan 0.3, spec §12 M6 6.3)

- **Context:** Plan 0.3 asked for a GitHub Actions workflow running the Gates on every
  push/PR. The workflow existed, but GitHub could not run it: the account's Actions usage
  is locked ("account is locked due to a billing issue" — an expired voucher). Every run
  failed at the platform level, before any job started.
- **Decision:** Remove `.github/workflows/ci.yml` entirely and drop CI/CD from the project
  for now. The Gates (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`) are run locally before any "done" declaration (see `AGENTS.md` working
  rule 2). Release builds in M6 (plan 6.3) will be produced locally/manually.
- **Rationale:** A permanently red pipeline is worse than none: it hides real signal.
  Re-introducing CI later is a one-file change; the workflow definition is simple enough
  to recreate exactly.
- **Flag for reviewer:** If Actions access is restored, we can reinstate the pipeline
  (possibly as a proposal in a future PR).

---

## ADR-0014 — YAML spans: typed parse via serde_yaml_ng, span scan via saphyr-parser (M2)

- **Context:** M2 needs span-labeled `miette` diagnostics (spec §5), but `serde_yaml_ng`
  exposes locations only on `Error` (`error.location()`), not per-value spans to
  deserializers — span-carrying typed deserialization is not possible with the pinned
  YAML crate.
- **Decision:** Two-track parsing (plan 1.2.1 allowed this choice). Track 1 (unchanged):
  `serde_yaml_ng` deserializes into the typed model; its errors carry a (line, column)
  location converted into a diagnostic span. Track 2: a lightweight event scan with
  `saphyr-parser 0.3` (`SpannedEventReceiver`) builds a map from value paths (e.g.
  `design.fonts.body`, `cv.sections[0].title`) to byte spans; semantic checks in
  `src/validate/` look up the span of the value they reject.
- **Rationale:** A full span-carrying model duplicate would double the model code; the
  path map only needs to cover scalars, which is ~100 lines. `saphyr-parser` is the
  maintained YAML 1.2 event parser and is a parse-only dependency (no serde coupling).
- **Consequences:** Parse errors (unknown key, wrong type, bad value string) are reported
  one at a time (serde stops at the first); semantic checks collect *all* errors of their
  class (spec §5). New dependency `saphyr-parser` justified in `Cargo.toml`.

## ADR-0015 — Warnings exist; duplicate section titles are a warning (plan 2.1)

- **Context:** Plan 2.1 asks whether duplicate section titles should be an error or a
  warning-level diagnostic; the spec's diagnostics model otherwise only knows errors.
- **Decision:** Validation returns two lists: errors and warnings. Warnings are rendered
  through the same `miette` graphical handler with `severity: Warning`; they are printed
  by `build` and `check` to stderr and never change the exit code. Duplicate section
  titles are the first warning (the first occurrence's span is labeled, or the model
  index when no span is available).
- **Rationale:** A duplicate title renders fine; failing the build would be hostile, but
  silently ignoring it hides real mistakes. This also gives the fit loop (M4) a place to
  put overflow warnings later.

## ADR-0016 — Inline Markdown: flat tool-parsed token list in `data.yaml` (spec §4, §14)

- **Context:** ADR-0008 fixed the direction for M2: the tool pre-parses inline Markdown
  into a small typed structure that `data.yaml` carries, and templates interpret it.
- **Decision:** The tool parses the subset (`**bold**`, `*italic*`, `` `code` ``,
  `[label](url)`) into a flat, non-nested token list
  (`{kind: text|bold|italic|code|link, text: "...", url: "..."}`) and serializes it in
  place of the raw string for the fields that support Markdown: `cv.summary`, `bullets`
  inside `entries` items, `bullets` section items, and `text` section items. Headings,
  orgs, dates, keys and titles stay plain literal text. Templates render tokens via a
  shared helper (`templates/*/lib/inline.typ`). Unknown or unclosed markers are literal
  text; markers inside token contents (no nesting) are literal text.
- **Rationale:** Flat tokens keep both the parser and the Typst helper trivial, and every
  piece of user text still reaches Typst as a `str` value displayed in content — strings
  render literally, so the escaping guarantee (spec §4, plan 2.6) holds without any
  character-level escaping table that could have holes.

## ADR-0017 — URL validation: small built-in checker, no `url` crate (plan 2.1)

- **Context:** Semantic checks must reject malformed link URLs (spec §5).
- **Decision:** A ~30-line validator: scheme must be `http` or `https`, the rest must
  contain a non-empty host part, and no spaces or control characters are allowed.
- **Rationale:** Rule "prefer std; if one small function is all you need, write it and
  test it". A full RFC 3986 parser adds a dependency for a check the spec only asks to be
  "well formed"; false positives would block legitimate URLs, so the checker stays
  conservative (rejects clearly broken input only).

## ADR-0018 — Diagnostic rendering: miette graphical handler, fixed width, warnings as essentials (plan 2.2)

- **Context:** The CLI boundary must render `miette` diagnostics (plan 1.2.2, spec §5
  quality bar) deterministically enough to snapshot-test.
- **Decision:** Errors are rendered with `GraphicalReportHandler` in the unicode,
  no-color theme at a fixed width (100 columns), one report per diagnostic. Color is
  enabled only when stderr is a terminal (`std::io::IsTerminal`); snapshots always render
  without color. Warnings use the same handler.
- **Amendment (after seeing real output):** The plan's "prefixed with `Error:`" does not
  apply: the graphical handler writes its own header per severity (`×` for errors, `⚠`
  for warnings) instead of a text prefix, and shows the source snippet below it. The
  header is accepted as the error marker; the spec §5 bar (message + span + "did you
  mean" hint) is met. Errors of *other* classes (I/O, write, render — no source span)
  still render as plain `Error: <message>` lines.
- **Rationale:** A fixed width and no color make `insta` snapshots stable across machines
  and CI-less environments; keeping one code path for both severities keeps output
  consistent (ADR-0012 already classifies warnings as essentials).

## ADR-0019 — `cv schema` output and `cv init` schema file (plan 2.3)

- **Context:** Plan 2.3: print the derived JSON Schema on `cv schema` and write
  `cv.schema.json` during `init` so the editor header works offline.
- **Decision:** `cv schema` prints `schema_for!(Document)` pretty-printed to stdout.
  `cv init` additionally writes `cv.schema.json` next to the `--out` file (same
  directory); `--force` overwrites both. The exact byte content of both is covered by an
  insta snapshot so accidental schema changes are reviewable.
- **Rationale:** The header line is `$schema=./cv.schema.json` relative to the YAML file,
  so writing the schema beside the YAML keeps editor tooling working for non-default
  `--out` locations too.

## ADR-0020 — Example validation without a JSON-Schema validator crate (plan 2.8)

- **Context:** Plan 2.8 asks that every `examples/` file validates against the schema.
  The schema is *derived* (`schemars`) from the same serde structs the parser uses.
- **Decision:** Examples are validated through the typed parse plus semantic checks
  (`cv check`), not by running the JSON Schema through a validator crate. The `examples/`
  tests also build each example and assert one page, and `tests/errors.rs` covers every
  rejection the schema (and the model) can express.
- **Rationale:** Schema validation via a separate validator would add a dependency
  (`jsonschema`) for a check strictly weaker than the typed parse — any document
  failing the schema fails `Document::from_yaml_str` too, since both share the model.

## ADR-0021 — `cv fonts`: source column from three font books; names from metadata (plan 3.3)

- **Context:** Plan 3.3 wants the available fonts listed "ideally marking source
  (bundled / --font-path / system)". Typst's `FontBook` does not record where each
  face came from once merged, and M2's `bundled_family_names` was a file-name
  heuristic that would drift from the real metadata names (e.g. the variable Plex
  build, where the static "SmBld" family-name split made file-name guesses wrong).
- **Decision:** Build three minimal `TypstEngine` font books — user dirs only, bundled
  only, and the full book (user + bundled + system via typst-kit; verified in the
  pinned `typst-as-lib 0.17` source that system search only happens when
  `search_fonts_with` is passed, so the two restricted books are exact). A family's
  source label is its **winner** in ADR-0007 priority: font-path dirs → bundled →
  system. Family names always come from the parsed font metadata (`FontBook::families`),
  never from file names; the file-name heuristic is deleted. Output: one family per
  line with a padded source column, grouped by source and sorted case-insensitively;
  the listing counts as essential output, so `--quiet` keeps it (ADR-0012).
- **Rationale:** Set-difference over books the renderer itself builds keeps listing,
  validation and rendering on one ground truth; per-family classification by winner
  matches what a build would actually load (a shadowed system font is not "available"
  as itself).
- **Consequences:** `available_font_families` (validation, plan 2.1) is now derived
  from `available_fonts`, so the two cannot drift; a font book is built per source
  on each `cv fonts`/`check`/`build` call — parsing the 19 embedded files twice costs
  milliseconds and system discovery still runs once.

## ADR-0022 — Font-path end-to-end fixture: renamed Inter clone "CvTest Sans" (plan 3.4)

- **Context:** Plan 3.4 requires proof that a `--font-path`/`design.font_paths`
  directory is *discoverable and selectable*. Every font file we already ship is also
  in the bundle, so picking it proves nothing about user dirs; relying on some
  machine-installed family would break the determinism rule (`.AGENTS.md` 18).
- **Decision:** `tests/fixtures/fonts/CvTestSans-Regular.ttf` — the bundled
  Inter-Regular with name-table IDs 1/2/3/4/6 rewritten to a unique family
  "CvTest Sans" (OFL §Revision: the Reserved Font Name is dropped from all records,
  the license ships next to the fixture as `CvTestSans-LICENSE.txt`). The tests show
  that the family (a) fails validation without the directory, (b) validates, lists
  under source `font-path`, builds and embeds into the PDF with `--font-path`, and
  (c) resolves via `design.font_paths` relative to the YAML. Regenerate with
  `fontTools` (dev-only, not a crate dependency):
  `TTFont('fonts/Inter-Regular.ttf')` → `name.setName` for the exact-match strings
  `Inter` / `Inter Regular` / `Inter-Regular` on IDs 1/2/4/6, plus the `Inter-Regular`
  segment of the unique name (ID 3); the copyright notice (ID 0) stays untouched as
  OFL requires → save.
- **Rationale:** A uniquely-named, license-clean clone is the only way to assert
  "resolved *because of* the user dir" deterministically; the negative test (a) rules
  out the fixture family sneaking in via system fonts.
- **Consequences:** ~400 KB test fixture in the repo; no production code paths depend
  on it.

## ADR-0023 — Manifest enforcement is a warning keyed on "written in the YAML" (plan 3.5)

- **Context:** Spec §6: "A template must honor all `design` fields listed in its
  `manifest.yaml`. Fields it ignores must be declared as unsupported so `cv check` can
  warn." Ambiguities: severity, what counts as "using" a field, and grouping.
- **Decision:** `cv check`/`cv build` emit a **warning** (never an error, ADR-0015) per
  design *group* (`page`, `margins`, `fonts`, `sizes`, `spacing`, `colors`) that the
  YAML writes but the template's manifest does not declare in `supported`. "Written"
  means the span scan (ADR-0014) saw any scalar at or under `design.<group>` — so an
  explicit value equal to the default still warns: the user authored it and expects an
  effect. The warning labels the first written value in path order. Tool-level fields
  (`template`, `fit_one_page`, `fit_floor`, `font_paths`) are exempt: the tool consumes
  them, templates cannot ignore them. If the template name itself is invalid the
  support check is skipped — the unknown-template error already covers it.
- **Rationale:** Group granularity matches the manifest vocabulary; presence-of-write
  is the only definition that needs no defaults comparison and works with spans we
  already collect.
- **Consequences:** No shipped built-in can trigger the warning today (`compact`
  declares all six groups — asserted by a test), so the behavior is pinned by unit
  tests against a declared-minimal template and becomes CLI-visible from M5 with custom
  templates.

## ADR-0024 — Fit search shape: coupled tightness, bounded bisection, fixed grid (plan 4.1/4.2)

- **Context:** Spec §7 demands: Phase A (spacing.* and margins, floors 0.6 / 1.0 cm),
  then Phase B (font sizes, floor = the stricter of `fit_floor.min_scale` and
  `min_body_size`), least-aggressive-first, binary search preferred, hard
  iteration/floor/determinism guarantees, scale factors via the `fit` object.
  Ambiguities: how Phase A's two levers (spacing, margins) combine; how "line_height"
  (a dimensionless ratio) participates in a scale floor of 0.6; where the font floor
  is enforced; how many probes.
- **Decision:**
  - Phase A couples both levers into one linear *tightness* `t ∈ [0,1]`:
    `spacing_scale = 1 − t·(1−0.6)` and `margin_scale = 1 − t·(1−margin_min)`, where
    `margin_min = max(1 cm/x, 1 cm/y)` capped at 1.0 — so margins approach but never
    cross 1.0 cm, and a margin already below 1.0 cm is never shrunk further. The
    identity compile doubles as the "t = 0 does not fit" knowledge point (never
    recompiled).
  - `line_height` tightens as `1 + (lh − 1)·spacing_scale`: only the leading *above*
    1.0 scales, so tightening can never overlap lines. Multiplying the ratio by 0.6
    (literal "spacing scale 0.6") would set line height to 0.75 — unreadable; the
    spec's floor is taken to apply to the leading, not to a ratio whose product may
    not fall below 1.0.
  - Phase B (only when A's floors still overflow) bisects `font_scale ∈ [floor, 1)`
    at `t = 1`, `floor = max(min_scale, min_body_size / sizes.body)` clamped to
    `≤ 1.0` (never shrink); nonsensical floor inputs (≥ 1, NaN, body 0) degrade to
    1.0 = font phase disabled. The template multiplies every size by `font_scale`,
    so `min_body_size` bounds exactly the body text (other roles may dip below it —
    they are relative to body by default).
  - Both searches are bisections with a **fixed probe count** (`MAX_PHASE_PROBES = 5`,
    resolution 1/32 ≈ 3.1 % of each phase; for the font phase that means ≈ 0.5 %
    steps, finer than the spec's 0.02 example). Probes are the exact midpoints, so
    the search is a pure function: deterministic and terminating by construction;
    worst case 13 compiles.
  - Least-aggressive reading: minimal `t` found (Phase A), then maximal `f` found
    (Phase B), no re-exploration of Phase A after Phase B succeeds (spec's ordering:
    spacing before fonts). If rounding makes the "fits" predicate non-monotonic
    (possible for layout), bisection may settle on a fitting-but-not-optimal grid
    point — the result still fits, never crosses a floor, and repeats identically.
  - After the searches find nothing, the floor settings are probed explicitly once,
    so `t = 1` / `f = floor` are exact results, not grid approximations.
- **Consequences:** `src/fit.rs` needs no Typst types (it only inspects
  `RenderOutput::page_count`); the engine compiles repeatedly with different
  `sys.inputs.fit`, fonts loaded once (plan 1.4.4). A finer search is a constant
  change, each probe ≈ one compile of cost.

## ADR-0025 — Success line reports the full applied scale triple when tightened (plan 4.1)

- **Context:** Spec §7: "The final output line reports the applied scale." M1/ADR-0012
  fixed the line to `fit: scale 1.00 (…ms)`; with three independent scales that single
  number is ambiguous.
- **Decision:** when Phase A tightened anything, the line lists the triple:
  `fit: scale 0.89 spacing 0.60 margins 0.85`; otherwise it stays the single
  `fit: scale X` — byte-identical to the M2 line (`fit: scale 1.00`) for first-fit
  CVs, so existing snapshots/tests keep their meaning. `--no-fit`/`fit_one_page:
  false` report the applied scales (all 1.00 — nothing was applied).
- **Rationale:** one stable prefix for scripts (`fit: scale `), full observability of
  what the loop did, no behavior change for the common case.

## ADR-0026 — Overflow at the floor: PDF written, exit 0, loud warning (plan 4.3)

- **Context:** Spec §7 step 4 and §14: when content still overflows at the floors,
  the PDF is written and a clear warning printed; the exit code was the open
  question ("0-with-warning vs. a dedicated non-zero code").
- **Decision:** **exit 0 with a warning**. Message (stderr, miette warning handler,
  also under `--quiet` per ADR-0012): `content still overflows at the fit floor:
  about N lines spill onto P extra page(s); cut content (e.g. bullets or paragraphs)
  or relax design.fit_floor / margins`. `N` counts shaped text runs on the overflow
  pages (computed in `src/render/` as `RenderOutput::overflow_lines`; a slight
  over-estimate for style-mixed lines, explicitly worded as "about"). The success
  line still reports the pages and the exact floor scales.
- **Rationale:** spec §3 ties non-zero exit to "any *error*", and an overflowed
  build produces the requested PDF — it is a result, not a failure. ADR-0015
  already fixed warnings as exit-code-neutral; a special code would be a second,
  contradicting rule ("warnings never affect the exit code" vs. "this warning
  does"). CI that must fail on overflow can grep the line or assert the page count
  from the success line; if that turns out to matter in practice, adding a code 2
  later is one line in `main` and does not change this contract's shape.
- **Consequences:** §14 exit-code question closed; the floor probe's PDF (the most
  aggressive compile) is always what gets written on overflow, so the user still
  gets the closest possible one-pager's content, over multiple pages.

## Open questions (spec §14) — status

Assigned to milestones per the implementation plan; each gets an ADR when resolved.

| Question | Status |
|---|---|
| Inline Markdown: tool-structured vs. Typst helper | **decided in M1 (ADR-0008): tool-structured; M1 ships plain text** |
| Exit code when content can't fit at the floor | **decided in M4 (ADR-0026): 0 with a loud warning** |
| Tagged PDF / accessibility support in pinned `typst-pdf` | **checked in M1 (ADR-0009): tagged PDFs on; standards finalized M6** |
| `cv init` role-tuned examples vs. one neutral example | open — decide in M6 (step 6.2) |
| Per-job variants (`--variant`) | out of scope for v1; keep model extensible |
