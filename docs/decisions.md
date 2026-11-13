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

## Open questions (spec §14) — status

Assigned to milestones per the implementation plan; each gets an ADR when resolved.

| Question | Status |
|---|---|
| Inline Markdown: tool-structured vs. Typst helper | **decided in M1 (ADR-0008): tool-structured; M1 ships plain text** |
| Exit code when content can't fit at the floor | open — decide in M4 (step 4.3) |
| Tagged PDF / accessibility support in pinned `typst-pdf` | **checked in M1 (ADR-0009): tagged PDFs on; standards finalized M6** |
| `cv init` role-tuned examples vs. one neutral example | open — decide in M6 (step 6.2) |
| Per-job variants (`--variant`) | out of scope for v1; keep model extensible |
