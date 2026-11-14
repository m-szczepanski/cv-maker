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
- **Amendment (M4 review):** the decision text says `data.yaml` carries `cv` + `design` +
  `fit` scales, but the fit scales were *only ever* delivered through `sys.inputs.fit`; the
  serialized `fit:` block was a permanently-identity duplicate that became actively
  misleading once the loop produced non-1.0 scales. `data.yaml` now carries `cv` + `design`
  only; `sys.inputs.fit` is the single fit channel.

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
- **Amendment (M5):** with a second built-in (`classic`) the `--template` flag must
  actually select it or it lies: `cv init --template <name>` now writes the chosen
  name into the starter's `design.template` (the rest of the content is unchanged).
  It still accepts built-in names only — a custom folder is referenced by path in
  the YAML afterwards, not through `init` (spec §3).

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

## ADR-0027 — Template reference grammar and custom-folder loading (plan 5.3)

- **Context:** Spec §6: `design.template` accepts a built-in name or a relative folder
  path (`./my-template`); custom templates use the same contract and must be pure (no
  network, no reads outside their folder, "enforce in the World impl"). Open points: how
  to tell a name from a path, what the folder must contain, and how purity is enforced.
- **Decision:** A reference is a **path** when it contains `/` or `\` or starts with `.`;
  anything else is a built-in name. Path references resolve against the CV file's
  directory (same rule as `design.font_paths`). The loader reads `manifest.yaml`,
  `template.typ`, and every nested `**/*.typ` from disk into the same in-memory
  `Template` shape the built-ins use — `src/render/` did not change. Purity is
  enforcement by construction: the Typst world only ever sees the virtual sources this
  module loaded plus the tool's `data.yaml`; no filesystem resolver is configured and
  the typst-as-lib package resolver (its network + cache path, verified absent in the
  0.17.0 sources unless `with_package_file_resolver` is called) is never enabled, so a
  `@preview` import fails as a template error. Load failures are typed
  `Error::TemplateLoad` messages naming the user-written reference and the exact
  problem (folder missing, no `template.typ`, no/invalid/unusable `manifest.yaml`,
  non-UTF-8 source), surfaced as a span-labeled diagnostic at `design.template` by
  `cv check`/`cv build`; each class has an e2e test.
- **Rationale:** The bare-name-is-builtin rule keeps the snapshot-pinned
  "unknown template 'compactt' / did you mean 'compact'?" UX exactly — a typo never
  silently becomes a folder lookup. Reading from disk into the existing in-memory
  contract beats teaching the World to resolve files: one loader, no Typst-side path
  policy, and purity falls out of the file set instead of being policed.
- **Consequences:** Binary non-`.typ` files (manifest `assets/` of spec §6) are not
  served to the compiler — built-ins need none, and wiring binary virtual files would
  touch the render layer for no current feature. `cv templates` lists built-ins only
  (spec §3). Custom-template Typst warnings are now reachable and reported (the
  reporting point reserved back in M2).

## ADR-0028 — Manifest validation: required fields, version, declared-vs-reads (plan 5.4)

- **Context:** Plan 5.4: custom-template manifests must have name/description/schema
  version present, and "declared supported options consistent with what the template
  reads". The "consistent" part is fuzzy: what a template reads is not statically
  decidable without interpretation.
- **Decision:** Three layers. (1) Presence and shape: serde — `name`, `description`,
  `schema_version` are required, unknown keys rejected (`deny_unknown_fields`).
  (2) Usability, checked on every load (built-in and custom): `name`/`description`
  non-empty after trimming, and `schema_version == 1` (mirrors the CV-level schema
  check; the manifest targets the same data contract). (3) Consistency: per design
  group, compare the manifest's `supported` against whether the template sources
  contain `design.<group>`; either-direction mismatch is a **warning** (ADR-0015
  class) labeled at `design.template`, exit stays 0.
- **Rationale:** The scan is a heuristic — aliasing (`let d = design`) defeats it —
  so it must never fail a build, and it runs for **custom templates only**: shipped
  templates' declared-vs-reads agreement is instead pinned by unit tests
  (`every_builtin_reads_exactly_the_groups_its_manifest_declares`), so no built-in CV
  can ever gain a new warning. A wrong `supported` name matters because the ADR-0023
  warning (written-but-undeclared ⇒ ignored) misfires in both directions: a
  declared-but-never-read group silently ignores user values *without* warning, an
  undeclared-but-read group triggers false warnings.
- **Consequences:** A copied built-in with an honest manifest stays warning-free; a
  manifest that lies (either direction) gets explained in `cv check` output with a
  span on the `design.template` line.

## ADR-0029 — Watch rebuilds on content snapshots, not event paths (plan 5.6)

- **Context:** Spec §3: `cv watch` rebuilds on change of the YAML *or its custom
  template files*, printing each build's result; spec §2 wants `notify` debounced.
  Ambiguities: which events count as a change (backends differ: FSEvents coalesces
  bursts and reports folder-level paths, inotify is per-file; atomic PDF writes land
  in the same directory as the YAML and would echo back), how to debounce without a
  second crate (`.AGENTS.md` 22), and what ends the loop.
- **Decision:** Events are only a *wake* signal. After the queue goes quiet for
  100 ms (drain-until-quiet, implemented directly over `notify`'s channel — no
  debouncer crate), the loop hashes the watched inputs — the YAML and, when
  `design.template` is a path reference, every file under that folder (a
  missing folder is itself one tracked state) — and rebuilds only when the
  snapshot differs from the one recorded before the last build, re-checking after
  each build (a save landing mid-build is caught up, so the PDF always converges
  to the newest content). Because our own PDF/temp writes are not in the snapshot,
  they can never cause a rebuild loop. Watch registration: the YAML's directory
  (non-recursive; missing YAML files still watch) plus the custom template folder
  recursively, re-synced whenever a rebuild was triggered by a YAML change (the
  reference may move). Each rebuild is a verbatim `cv build` run: same result line,
  same warnings, same exit-code-neutral overflow report. Build errors print and
  the loop continues — the watched file is the one being edited. Startup: build
  once, then watch; `--quiet` drops the progress banner and the result lines
  (ADR-0012) but not errors/warnings. Ctrl-C (default signal handling) ends the
  session; v1 ships no graceful-shutdown plumbing. A missing input *directory*
  fails at startup naming it.
- **Rationale:** Trusting event paths for the *decision* would couple correctness
  to backend quirks; content snapshots are backend-independent, dedupe editor
  bursts without timers, make missed events self-healing (the next wake rebuilds
  whatever is actually on disk), and make the rebuild-equality logic unit-testable
  without sleeping (rule 18). The catch-up loop guarantees convergence rather than
  "exactly one build per event", which is strictly better to watch.
- **Consequences:** A rebuild costs one content hash pass over a handful of small
  files (~µs). Creating a missing template folder inside the YAML directory wakes
  the watched dir itself, so the catch-up loop rebuilds and the sync registers the
  new folder in the same pass. The `Burst::Disconnected` arm (channel died while
  the watcher is alive) is unreachable from user input — it reports an internal
  error rather than silently exiting.

## ADR-0030 — `classic` ships without rules (spec §6 amended, plan M6)

- **Context:** The 5.8 visual review found the full-width `line()` rules — the header
  rule and the per-section dividers — heavier than the built-ins should be: "templates
  should be very clean, minimal and slick". Spec §6 described `classic` *as* having
  them, so code and spec disagreed.
- **Decision:** Delete both rules. Fixed the **spec** (§6 now describes `classic` as a
  single-column layout with a right-aligned contact header and title-case accent
  headings, no rules), the manifest description, and the CLI listing test. Hierarchy
  now comes from type, colour and spacing alone: bold title-case accent headings,
  semibold orgs, `section-gap` of air after the header. Both templates were re-rendered
  and inspected (starter, `full`, and a 14×2 overflowing CV fitted to one page).
- **Consequences:** `classic` and `compact` differ by header alignment, heading case and
  weights rather than by rules; no schema, data-contract or fit-loop change, so the
  built-in parity tests still hold.

## ADR-0031 — font names match the way Typst matches them

- **Context:** Audit finding. Typst 0.15.1 lowercases the selector before looking it up
  in the font book (verified in the pinned source), so `design.fonts.body: inter`
  renders Inter — while `cv check` rejected it as an unknown font. Validation was
  *stricter* than rendering, contradicting ADR-0021's shared ground truth.
- **Decision:** Compare family names case-insensitively in `check_fonts`. `cv fonts` and
  the "did you mean" hints keep printing canonical names (they come from font metadata).
  A test pins the consequence: `inter`/`INTER`/`jetbrains mono` validate and build
  **byte-identical** PDFs to the canonical spellings, so case is a spelling, not a style.
- **Consequences:** Input previously rejected is accepted and renders the intended font.
  Genuinely missing families still error with a suggestion (spec §8: never a silent
  fallback to a *different* family).

## ADR-0032 — `--quiet` keeps Typst render warnings (ADR-0012 follow-through)

- **Context:** ADR-0012 classifies warnings as essentials that `--quiet` must not
  suppress, but `cv build` printed the render layer's warnings inside the `!quiet` block
  — so a custom template's Typst warnings disappeared under `--quiet`. The comment
  calling the loop "the reporting point for M5 custom templates" was also stale: M5
  shipped and the loop is reached.
- **Decision:** Move the render-warning loop out of the quiet guard, next to the
  validation warnings. Corrected the comment. A test pins both halves: the warning
  survives `--quiet`, and the success line does not.

## ADR-0033 — Markdown link URLs are checked where they are written, and aliases keep spans honest

- **Context:** Two audit findings. (1) `summary: see [work]()` passed `cv check` and then
  failed `cv build` with **"template error: URL must not be empty"** — a user data mistake
  reported as a template bug (spec §5 says those are distinct), and Typst genuinely
  refuses an empty target. (2) The span scan ignored alias events, so a value written as
  a YAML alias not only lost its own span but shifted every later sibling in that
  mapping: diagnostics pointed at the wrong line.
- **Decision:** (1) `check_markdown_urls` walks the parsed tokens of exactly the
  Markdown-enabled fields (`Cv::markdown_texts`) and splits the severities the way the
  renderer behaves: an **empty** target is an error (the export refuses it), a target no
  PDF reader can open — bare host, path relative to the PDF, embedded whitespace — is a
  **warning** (it renders, ADR-0015), and any absolute scheme (`https:`, `mailto:`,
  `tel:`, `ftp:`) is accepted. `data.yaml` and the check must cover the same fields —
  pinned by a drift test over the fixture in `src/render/data.rs`. (2) The span walker
  treats an alias as a value that takes its frame slot without recording a span, so later
  siblings stay aligned and a check that rejects an aliased value reports the resolved
  value without a snippet — a real, tested path (`diag_at` is the single place that
  decides labeled-or-plain).
- **Consequences:** `cv check` predicts `cv build` for the case that actually breaks the
  build; a `mailto:` CV keeps working (it did before M6 — an earlier draft of this ADR
  applied the contact-link http(s)-only rule to Markdown links and that would have been a
  silent breaking release, caught in review); contact links keep ADR-0017's stricter rule
  on purpose, since they are the CV's primary routes to the candidate; one diagnostic
  helper replaces six near-identical fallback ladders.

## ADR-0034 — `render` never touches the file system; the command layer does

- **Context:** `Engine::build` read the user font directories itself, so every build
  read them twice (validation had already done it) and the render layer carried an I/O
  error arm that no validated input can reach. Spec §9 wants the render layer to be a
  pure compile step over its inputs.
- **Decision:** `validate` reads the user font directories once and hands the bytes back
  in `Outcome::Valid { font_items, .. }`, which `load_cv` passes to
  `RenderRequest.user_font_items`; `src/render/` receives data, not paths. `Engine::build`
  is infallible, and `book_families`/`to_yaml`/`schema_json` (which cannot fail for the
  tool's own data) return values instead of `Result`, panicking with a stated invariant on
  the impossible case (`.AGENTS.md` rule 14) rather than inventing user-facing errors.
  A directory that cannot be read is reported **as a diagnostic among the other checks**
  — an earlier draft returned the I/O error from `load_cv` before validation ran, which
  hid every schema, template and URL problem in the same file behind one message and
  broke spec §5's "all errors of all checks are collected" (caught in review).
- **Consequences:** One font read per build, Typst types and file system policy still
  confined to `src/render/` (spec §13.7), three unreachable error arms are gone, and a
  CV with both a bad `font_paths` entry and other mistakes lists them together.

## ADR-0035 — validation hands the resolved template to the build

- **Context:** `cv build` loaded the template a second time after `load_cv` had already
  validated the reference, so a custom folder was read twice per rebuild (watch rebuilds
  every save) and the reload carried an error arm that validation had ruled out.
- **Decision:** `validate` returns an `Outcome`: `Valid { template, warnings }` or
  `Invalid(errors)`. `LoadedCv` carries the template, and the render uses exactly what
  was judged. Errors are still collected before the decision (spec §5: all errors, not
  the first), so a broken reference is reported alongside font and URL problems.
- **Consequences:** One template read per build; "validated" and "rendered" are the same
  object by construction; no unreachable fallback in the command layer.

## ADR-0036 — unreachable-by-construction code is deleted, injected, or documented

- **Context:** M2–M5 left a documented residue of defensive branches (rule 21) that
  `cargo llvm-cov` reported as uncovered: impossible match arms, error mappings for
  calls that cannot fail, and the whole of `cv watch`'s loop, which the e2e tests could
  only reach by killing a child process — and a killed process cannot flush its
  coverage profile.
- **Decision:** Work through the list rather than keep documenting it. The impossible
  arms are gone or became stated invariants (ADR-0034). `cv watch` is split into
  `watch`/`Watch` with the event channel and the watcher creation injected, so the
  initial build, one wake, the rebuild-equality decision, a refused registration and
  the loop's exit are unit-tested in-process; the e2e tests keep killing a real child
  because that is how a real watch ends. `assemble_builtin` is pure over its file list,
  so broken embedded assets are testable with synthetic ones instead of being
  "unreachable". `WatchFolders` is the seam for the two notify operations, because a
  real backend only refuses a folder on OS watch slots.
- **Consequences:** Measured coverage went from 96.67% lines at the end of M5 to
  **100.00%** (functions 100.00%, regions 99.95%): every source line executes, and
  `--show-missing-lines` reports nothing. The three regions still counted as missed are
  accounting rather than untested behaviour: `write_atomic`'s `?` after the sink write
  (the fault is injected at the seam, not by breaking the disk — its comment says so),
  `cv templates`' `?` after collecting its built-ins (kept because a broken bundle should
  be reported, not panicked — ADR-0042), and a numeric-literal sub-region inside
  `fit.rs`'s floor guards, whose lines execute.
  Test code no longer uses `unwrap_or_else(|| panic!(…))` forms whose failure paths
  show up as uncovered either.

## ADR-0037 — release binaries built locally with `cargo-zigbuild` (ADR-0013)

- **Context:** ADR-0013 dropped CI, so M6's binaries have to come off a development
  machine. This one is Apple silicon macOS with no Docker; the matrix needs Linux
  x86-64/arm64, macOS x86-64/arm64 and Windows x86-64.
- **Decision:** Dev tooling only, no crate dependencies added: `brew install zig` plus
  `cargo install cargo-zigbuild`, then `scripts/build-release.sh` builds
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`,
  `aarch64-apple-darwin` and `x86_64-pc-windows-gnu` into `dist/` (gitignored) as
  `cv-<version>-<triple>.tar.xz` / `.zip` with a `SHA256SUMS.txt`; each archive carries
  the `LICENSE` next to the binary. `cv` is published by hand from that directory;
  nothing is pushed automatically.
- **Consequences:** Cross-compiled binaries are *built* but only the host
  architecture can be *run* here — `scripts/fresh-machine-test.sh` runs the quick start
  from an unpacked archive in an isolated `HOME` with a minimal `PATH` for the host
  triple, and reports the others as structure-checked but not executed. Reproducing the
  whole matrix on another machine needs zig, cargo-zigbuild and the rustup targets.
  **Archives are reproducible** (verified by building the whole matrix twice and
  comparing `SHA256SUMS.txt` byte-for-byte): members go in `LC_ALL=C` order with pinned
  timestamps, because the first version hashed differently on every run even though the
  binary inside was identical. The builder's uid/gid remains in the container, so
  reproducibility holds per build host rather than across arbitrary machines — enough for
  published checksums, and the reason `--sort`/`--mtime` tar flags were dropped (macOS's
  bsdtar rejects them; the first "verified reproducible" claim here was wrong precisely
  because that failed build left the old checksum file in place).
  Which archives are runnable is decided by the **triple in the file name**, matched
  against `scripts/release-targets`, never by trying to exec them. An earlier draft
  treated any failed `--version` as "wrong architecture" and exited 0, so a corrupt or
  Gatekeeper-blocked artifact — the exact failure this check exists to catch — passed the
  acceptance test; and a name whose triple was not in the matrix skipped as "another
  architecture" too. Now a host-triple artifact that will not run is a hard failure, an
  unclassifiable name is refused, and `.zip` archives are unpacked with `unzip` so the
  script works on Linux as well.
- **Hardening round over those fixes** (found by re-reading the new code as an attacker
  would): the triple was still matched as a *regex* built from the file name, and
  `cv-1-aarch64.*darwin.tar.xz` matches the real triple `aarch64-apple-darwin` — the
  silent skip again, one name away. It is a fixed-string whole-line match now
  (`grep -qxs -F`); the first fix attempt even forgot the `-F`, which the probe battery
  caught because the crafted file only failed later on its truncated bytes. The
  `build-release.sh` target argument is compared as an exact string for the same reason
  (a case pattern built from user input treats glob metacharacters as part of the
  matrix), the checksum step fails loudly when no artifact was built (before: an
  errexit abort or a hash of an empty stream, after the checksum file was already
  deleted), and both scripts fall back from `shasum` to `sha256sum`, which makes the
  Linux claim above true on machines without perl's `shasum`. The checksum block also
  carried dead code from its first draft (`sed` normalizing `./` names it no longer
  produced) — deleted.

## ADR-0038 — tagged PDF 1.7, no PDF/A or PDF/UA validator (§14 closed)

- **Context:** ADR-0009 turned tagged PDFs on and deferred the standards choice to M6.
  Pinned `typst-pdf 0.15.1`: `PdfStandards::default()` is PDF **1.7** with no archival
  or accessibility validator, `tagged: true` writes the structure tree, and
  `PdfStandards::new(&[PdfStandard::A_1b | Ua_1])` would add a validator that can
  *refuse* the export.
- **Decision:** Keep PDF 1.7 + tagged, no PDF/A or PDF/UA conformance. Reasons: the
  accessibility gain (structure tree, real selectable text, correct reading order,
  embedded subsets) is already delivered by `tagged`; a validator can turn a CV that
  renders today into a build failure, which is the wrong trade for a one-page document;
  and PDF/A would force colour/annotation rules that Typst's own metadata (no
  `InfoID` beyond a title/author-derived hash) does not need.
- **Consequences:** The §14 question is closed. If a reviewer later wants
  `PDF/UA-1`, it is a one-line change in `Engine::build` plus a decision about what the
  tool does when the validator rejects a document.

## ADR-0039 — one neutral `cv init` example, not role-tuned ones (§14 closed)

- **Context:** §14 asked whether `cv init` should offer examples tuned for backend,
  frontend or data roles.
- **Decision:** One neutral example, now extended to show the fit knobs
  (`fit_one_page`, `fit_floor`) so the starter demonstrates every section type and
  every design group, which is what makes it usable as documentation and as the model's
  end-to-end parse fixture (ADR-0011).
- **Rationale:** Role variants multiply the embedded assets and the tests without
  changing what the tool does: the difference between a backend and a design CV is the
  *content*, which is the user's to write. `--template` already selects the layout, and
  a role flag would be a second, overlapping way to pick one.
- **Consequences:** §14 is fully closed: two questions decided in M1/M4, two closed
  here, per-job variants still out of scope for v1 — nothing in the model blocks a
  later `--variant` (`schema`/`cv`/`design` stay independent keys).

## ADR-0040 — a PDF the exporter refuses is its own error class

- **Context:** The audit assumed `typst_pdf::pdf` can only fail on "options misuse",
  since our `PdfOptions` are constants. That is wrong (pinned 0.15.1 sources): the
  exporter inspects the *document* while writing it — `src/attach.rs:30` rejects a
  malformed `mime-type`, `src/convert.rs:462` an image it cannot encode, `src/link.rs`
  and `PdfError::VersionMismatch` others. A custom template can call any Typst
  function (spec §6 puts no whitelist on it), so a CV whose document compiles cleanly
  can still be refused on the way out, and that was an untested, undocumented arm.
- **Decision:** Keep it as `RenderError::Pdf` — a distinct class from a compile failure
  (`Template`) and from a user data error (`Input`): the Typst source was valid, the
  PDF was not. Two tests pin it: a unit test over `render()` and an e2e `cv build` on a
  template that does `#pdf.attach("data.yaml", mime-type: "not a mime type")`, asserting
  exit 1, "PDF export failed", the exporter's own reason, and no PDF left behind.
  Related guarantee, same audit: the atomic write now *flushes before it renames*,
  because a buffered sink would otherwise let a truncated PDF be published — which is
  exactly what spec §3 forbids.
- **Consequences:** `src/render/` no longer claims an impossibility it cannot keep; the
  last unreachable-by-assumption arm in the render layer is a tested error class; a
  template author gets the exporter's message rather than a mystery.

## ADR-0041 — a hung-up reader stops the writing, on both streams (spec §3)

- **Context:** M6 made `cv fonts | head -1` exit 0 instead of panicking, but only for
  stdout (`out_line`). Review found the same crash on the far more common stderr path:
  `cv check broken.yaml 2>&1 | head -1` panics (exit 101) as soon as the report is larger
  than the ~64 KB pipe buffer — a file with 300 duplicate titles writes 157 KB — because
  Rust ignores `SIGPIPE` and `eprint!` turns the resulting `EPIPE` into a panic. The
  affected sites were every diagnostic writer: `report_warnings`, `report_overflow`, the
  fatal report in `main`, `cv watch`'s per-build report and its folder-watch warning, and
  the `--verbose` line. `err_line` existed but was used in two places.
- **Decision:** Every byte the CLI writes to a stream goes through the best-effort
  helpers in `src/commands/mod.rs` — `out_line`, `err_line`, and a new `err_text` for
  pre-rendered multi-line reports — and no `print!`/`eprint!` remains in `src/` outside
  test code. Hanging up is the reader's choice, not an error to report and never a crash;
  the exit code still reflects the command's own result. Pinned by two e2e tests that run
  a real pipeline: the listing (stdout) and a 157 KB diagnostic stream (stderr).
- **Consequences:** `cv | head`, `cv | grep`, and editors that kill the reader mid-report
  behave; the panic cannot come back silently, because both streams have a test.

## ADR-0042 — only loadable folders are built-in names

- **Context:** Closing a coverage gap first made `cv templates` call
  `load_builtin(name).expect("a listed built-in loads")`. Review pointed out that
  `builtin_names()` returned *every* folder embedded under `templates/`, so the
  invariant behind that `expect` was only held by the parity tests: a stray folder
  (`templates/shared/`, a half-added template) would turn a listing command into a panic.
- **Decision:** One predicate says what a built-in is — `has_entry_point`, a
  `template.typ` at the folder root — and both `builtin_names()` and `load_builtin` use
  it: a folder without an entry point is not a template, so it is neither listed nor
  offered as a did-you-mean, and asking for it by name gives the user-facing
  `unknown template` error with the real built-ins listed. `cv templates` keeps the typed
  error path for a bundle that *is* a template but fails to load (undecodable source,
  unusable manifest), reported as `internal error: built-in template 'x' … (this is a bug
  in cv)`.
- **Consequences:** No panic reachable from the shipped asset tree's shape; the cost is
  one never-taken error continuation in `cv templates`, which is the accepted trade over
  a crash (`.AGENTS.md` rule 21 prefers the typed report).

## ADR-0043 — `design.pages`: the fit loop targets a user-chosen page count

- **Context:** The spec's v1 non-goal said "no multi-page CVs. One page is the product."
  After the release the project owner asked for the target to be configurable: default 1,
  user-selectable otherwise. The fit loop (§7), the overflow warning, and the templates
  all hard-coded the one-page assumption (`fits()` compared `page_count <= 1`, overflow
  runs counted past page 1).
- **Decision:** A new design field, `pages` (the owner picked the name over
  `page_count`/`max_pages`; `design.page` remains the page *size*). It is a
  `NonZeroU32` with default 1, so `pages: 0` is a parse error naming the field and the
  line, and the derived JSON Schema carries `minimum: 1` — no hand-written validation.
  The field is a *target*, not a quota: the fit loop (§7) tightens spacing, margins, and
  fonts until the document fits within `pages` pages, with the same floors, the same
  13-compile budget, and the same determinism as before; overflow is measured against
  the target (`page_count - pages` extra pages), and the warning now suggests raising
  `design.pages` alongside cutting content and relaxing `fit_floor`. There is no CLI
  flag: `--no-fit` and `fit_one_page` remain the loop's opt-outs, and `pages` belongs to
  the file like every other design field. Templates need no change — Typst paginates
  naturally, and the field reaches them through `data.yaml` (`design.pages`) like the
  other tool-level design values.
- **Consequences:** The non-goal in spec §1 is revised rather than kept (recorded there);
  `fit_one_page` keeps its name as the loop's on/off switch even when the target is not
  one page. One template-behavior caveat surfaced during testing: a built-in template's
  section block is unbreakable, so one enormous section lays out as an overflowing page
  rather than splitting — multi-page CVs should use several sections; this is
  pre-existing layout behavior, not part of this change. (Closed by ADR-0046: sections
  are now kept whole when they fit a page and flow honestly when they do not.)
## ADR-0044 — the fill phase: a fitting multi-page CV grows its scales until the pages are used

- **Context:** `design.pages` (ADR-0043) lets a user ask for two pages, and the fit loop
  guarantees the content *fits* — but "fits" and "fills" are different. A short CV with
  a two-page target rendered as a full first page and two stranded lines on the second;
  the owner asked for pages to be filled, ideally to the bottom margin, instead.
- **Decision:** Phase F of the loop (spec §7). When the identity compile already fits
  and the target is above one page, the loop runs bounded *growth* searches, each
  entered only when the previous one ended at its cap with the document still fitting:
  `spacing_scale` toward 3.0, then `font_scale` toward 1.25, then `margin_scale` toward
  1.4 — margins last because growing them shrinks the text area, the only lever that
  fills without touching typography at all. Each stage bisects its range like the
  tighten phases do (deterministic midpoints, same probe count), so the fill path is
  bounded at `1 + 3 * (MAX_PHASE_PROBES + 1) = 19` compiles, mutually exclusive with
  the tighten path's 13. The caps are sanity limits, not fill guarantees: content far
  below the target cannot fill two pages gracefully at any setting (a CV that needs 3×
  rhythm and 25% larger type to reach page two was the wrong length to begin with — the
  warning path already says what to do). A one-page target never fills: its layout is
  the top-flow identity. The success line prints every scale that moved (`fit: scale
  1.22 spacing 3.00 margins 1.00`), so a grown CV is visible.
- **Consequences:** `fit_note` can no longer treat "scale ≥ 1.0" as "untouched" — it
  compares against the identity exactly; the fill path is new, so every guard a tighten
  test has (determinism, compile error propagation, floors) has a fill twin. Growing
  fonts past the design size interacts with `fit_floor`, which floors *shrinking* only;
  the caps are the growth-side equivalent, deliberately not user-configurable.

## ADR-0045 — `cv.gdpr`: a consent statement at the bottom of the last page

- **Context:** Applications in Poland (and EU hiring generally) expect a GDPR consent
  clause at the foot of the CV. The owner asked for the standard clause rendered at the
  bottom of the last page — the bottom of the only page for a one-page CV — and
  provided the wording to ship as the default.
- **Decision:** A content field, `cv.gdpr` (the owner picked `gdpr` over `privacy`/
  `legal`): literal text — legal wording must never be Markdown-processed — typed by the
  user, and *defaulted* to the standard recruitment clause (`DEFAULT_GDPR`), since the
  footer is expected in the target market and absence would surprise more than presence;
  `gdpr: null` opts out. The built-in templates render it small and muted after all
  content, with `v(0.9em) #v(1fr)` above: the stretchable space pins it to the bottom
  margin of whatever page the content ends on (verified against real frames in
  `src/render` — `v(1fr)` collapses at a page break but fills a page's leftover, which
  is exactly what anchoring needs). The statement is part of the document the fit loop
  measures, so a page that is full tightens rather than pushing the footer onto a page
  of its own. Custom templates get the value through `data.yaml` and decide for
  themselves.
- **Consequences:** Every CV built without `gdpr` now carries the clause — users
  targeting markets where it is unexpected must write `gdpr: null`; that trade was made
  deliberately, on the owner's request, and is stated in the README. `DEFAULT_GDPR` is
  a legal string in the binary: changing wording is a content decision, not a refactor,
  and it shifts the schema snapshot (schemars emits the default).

## ADR-0046 — a section that fits one page is kept whole

- **Context:** The fill phase (ADR-0044) needed sections to flow across pages to fill
  them, and the first cut of ADR-0044 made built-in template sections breakable. The
  owner rejected the result: a section cut in half at a page boundary is visibly broken
  in a CV, whatever the fill statistics say. ADR-0043's caveat (one enormous section
  lays out as an overflowing page) had to be resolved too, not just documented.
- **Decision:** Both, decided per section at layout time by the template, not by the
  tool: `measure` (Typst ≥ 0.15 scripting API, verified against the pinned 0.15.1) the
  section's height against the content area; if it fits, wrap it in
  `block(breakable: false)` so Typst moves the whole section to the next page; if it
  does not fit one page, let it flow — the only alternative is content past the page
  edge, which is worse. The heading stays `sticky` (ADR-0044) so a flowing section
  never starts with the heading alone on the previous page. This closes the ADR-0043
  caveat: an enormous section now splits honestly across pages instead of overflowing.
- **Consequences:** Page fill can no longer be perfect in general — a whole-section
  rule leaves a gap whenever the next section does not fit the remaining space, and the
  fill phase closes it with rhythm, type, and margins up to their caps, which is the
  best a document with unbreakable sections can do. The decision lives in the templates
  (`section-block`), where the page geometry is known; the tool's fit loop still only
  sees page counts. `measure` runs an extra layout pass per section per compile —
  visible in build times (a multi-section two-page fit run went from ~3 s to ~6 s in
  debug builds), accepted as the cost of correct pagination.

## Open questions (spec §14) — status

All closed as of M6; each carries the ADR that resolved it.

| Question | Status |
|---|---|
| Inline Markdown: tool-structured vs. Typst helper | **decided in M1 (ADR-0008), implemented in M2 (ADR-0016): tool-structured flat tokens in `data.yaml`** |
| Exit code when content can't fit at the floor | **decided in M4 (ADR-0026): 0 with a loud warning** |
| Tagged PDF / accessibility support in pinned `typst-pdf` | **closed in M6 (ADR-0009 + ADR-0038): tagged PDF 1.7, no PDF/A or PDF/UA validator** |
| `cv init` role-tuned examples vs. one neutral example | **closed in M6 (ADR-0039): one neutral example, now showing the fit knobs** |
| Per-job variants (`--variant`) | **out of scope for v1; nothing in the model blocks it (ADR-0039)** |

