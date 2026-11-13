# Implementation Plan: `cv` — code-first YAML to one-page PDF

Derived from `docs/details.md` (the spec, v0.1 draft). The spec is the source of truth; this
file only sequences the work. Each step is written so it can be turned into a task, reviewed,
and checked off. Steps are ordered; within a milestone, do them roughly in order.

Conventions used below:

- **Gates** = `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
  must pass before any milestone is declared done (spec §11).
- **Decisions** = anything ambiguous gets resolved, written to `docs/decisions.md`, and flagged
  for review (spec §13.4). Open questions from spec §14 are assigned to the milestone where
  they first block work.
- **No invented APIs** = before writing any Typst / `typst-pdf` / `typst-as-lib` integration
  code, read the docs.rs page for the exact pinned version or compile a tiny experiment crate.

---

## Milestone 0 — Project bootstrap (prerequisite, no review stop)

The spec starts at M1, but the repo needs scaffolding first. Keep this small.

- [ ] 0.1 Create the repository skeleton per spec §10:
      `Cargo.toml`, `rust-toolchain.toml` (pin stable toolchain + 2021/2024 edition),
      `AGENTS.md` (short working rules pointing at the spec),
      `docs/decisions.md` (empty ADR log), `src/`, `tests/`, `templates/`, `fonts/`, `examples/`.
- [ ] 0.2 Add dependencies to `Cargo.toml`, each with a one-sentence justification comment
      (spec §2 rules): `clap` (derive), `serde`, `serde_yaml_ng`, `schemars`, `miette`,
      `thiserror`, `include_dir` or `rust-embed` (pick one, record why), `notify`.
      Add `typst`, `typst-pdf`, `typst-kit` (and `typst-as-lib` only if it supports the pinned
      Typst version — verify, don't assume) **in M1 step 1.4**, not now, so the skeleton
      compiles without them.
- [ ] 0.3 ~~Set up CI to run the Gates on every push/PR~~ — dropped (ADR-0013): no CI/CD
      pipeline for now; run the Gates locally instead (see `AGENTS.md`).
- [ ] 0.4 Write `src/main.rs` + `src/cli.rs` with all commands from spec §3 stubbed as `todo!`
      — except make `cv schema` and `cv templates` real enough to not lie (they can print
      "not implemented yet" and exit 1). Actually: prefer real stubs returning errors, so the
      CLI shape is testable from day one.
- [ ] 0.5 First commit passes Gates (trivially). Verify `cargo run -- --help` shows the full
      command surface.

**Accept for M0:** CLI help output matches spec §3 command table; Gates pass locally.

---

## Milestone 1 — Skeleton and first PDF

Goal (spec §12 M1): `cv init && cv build` produces a one-page PDF with selectable text.

### 1.1 Data model

- [ ] 1.1.1 Define the content model in `src/model/` as serde structs with
      `deny_unknown_fields` (spec §4): `Document { schema, cv, design }`, `Cv`, `Contact`,
      `Link`, `Section`, `EntryItem`, `KeyValueItem`. Section types: `entries`, `key_values`,
      `bullets`, `text` (closed enum in v1).
- [ ] 1.1.2 Define the full `design` model with **defaults defined in Rust** for every field
      (spec §4): `template`, `page` (a4|letter), `margins`, `fonts`, `sizes`, `spacing`,
      `colors`, `fit_one_page`, `fit_floor`, `font_paths`. A missing `design` section must
      deserialize to defaults.
- [ ] 1.1.3 Value types that parse strictly and remember their span source where needed:
      lengths (`1.5cm`, `9.5pt`), colors (`#2563eb`), font family names. Represent dates and
      URLs as plain strings for now (semantic checks come in M2).
- [ ] 1.1.4 Derive `JsonSchema` (`schemars`) on everything, even though `cv schema` lands in
      M2 — deriving now avoids retrofitting.
- [ ] 1.1.5 Unit tests: parse the spec §4 example YAML end-to-end; missing `design` works;
      unknown key is rejected; wrong section type is rejected.

### 1.2 YAML parsing with spans

- [ ] 1.2.1 Parse YAML retaining spans (`serde_yaml_ng` locations) into a raw form, then
      deserialize into the typed model; keep enough span info to point `miette` labels at the
      right line/column later. (Exact mechanism — span-carrying wrapper vs. re-parse on error —
      is an implementation choice; record it in `docs/decisions.md` if non-obvious.)
- [ ] 1.2.2 Error type in `thiserror`, converted to `miette` diagnostics at the CLI boundary.

### 1.3 `cv init`

- [ ] 1.3.1 Embed a starter YAML as a `const &str` (this is generated content, not user data —
      a plain string is fine). Support `--template <name>` (only `compact` exists; reject
      others with a clear message), `--out <file>`, and refuse to overwrite without `--force`.
- [ ] 1.3.2 First line: `# yaml-language-server: $schema=./cv.schema.json`. The schema file
      itself is written in M2; until then the header still works as a comment.
- [ ] 1.3.3 Test: `init` in a temp dir → parse the result back with the model → round-trips.
      Second `init` without `--force` fails.

### 1.4 Typst integration core (`src/render/`)

**Highest-risk module. Isolate Typst types here; nothing else imports them (spec §9).**

- [ ] 1.4.1 Pin exact `typst` / `typst-pdf` versions. Read their docs.rs pages. If
      `typst-as-lib` supports the pinned version, use it; otherwise plan a hand-written
      `World` impl. Record the choice + pinned versions in `docs/decisions.md`.
- [ ] 1.4.2 Implement the interface from spec §9:
      `RenderRequest { template, resolved data, fit params, font config }` →
      `RenderOutput { pdf: Vec<u8>, page_count, warnings }`. Compile in memory, no temp files.
- [ ] 1.4.3 Provide fully resolved `cv` + `design` to Typst as a virtual file (`data.yaml` or
      `data.json` — pick one, record it). **The tool must not generate Typst source from user
      data** (spec §6 contract).
- [ ] 1.4.4 Font loading: bundled fonts via the embed crate + `typst-kit` system discovery,
      resolution order `font_paths`/`--font-path` → bundled → system (spec §8). Missing
      family is an error with suggestions, never silent fallback. Fonts load once; reuse
      across fit-loop iterations (design the API so this is possible even if unused until M4).
- [ ] 1.4.5 PDF determinism from the start: fixed/absent creation timestamp, document title
      and author from `cv.name`. Check what tagged-PDF support exists in the pinned
      `typst-pdf` and note it for the §14 open question.

### 1.5 `compact` template

- [ ] 1.5.1 Create `templates/compact/` per spec §6: `template.typ`, `manifest.yaml`
      (name, description, supported design options, schema version). Template reads the
      virtual data file with Typst's `yaml()`/`json()`; honors all v1 design fields it
      supports; supports all four section types.
- [ ] 1.5.2 Decide inline-Markdown handling **now** (tool-structured vs. Typst helper
      `lib/inline.typ`) — it's a §14 open question that blocks template text rendering.
      For M1 the simplest path is acceptable; record the decision and revisit in M2.
- [ ] 1.5.3 Template loading in `src/templates/`: built-in from the embedded assets;
      custom path support deferred to M5 but keep the loader shape ready for it.

### 1.6 `cv build`

- [ ] 1.6.1 Default input `cv.yaml`; output `<stem>.pdf` next to the input. `--output` override.
      Overwrite silently. **Atomic write:** temp file in same directory → rename.
- [ ] 1.6.2 One-line success output: `cv.pdf  1 page  fit: scale 1.00  (118 ms)`.
- [ ] 1.6.3 Global flags wired: `--font-path` (repeatable), `--quiet`, `--verbose`.
- [ ] 1.6.4 Template errors (Typst compiler diagnostics) reported separately and labeled as
      template errors, distinct from user errors (spec §5).

### 1.7 Tests for M1

- [ ] 1.7.1 `tests/build_examples.rs`: build the init-starter YAML, assert PDF exists,
      exactly 1 page, extract text and assert on name + one heading + one bullet.
- [ ] 1.7.2 Determinism test: two builds → byte-identical PDFs.
- [ ] 1.7.3 Atomic-write test: failing build leaves an existing PDF untouched.
- [ ] 1.7.4 Visual check (human): render the starter CV and look at it (spec §13 reviewer note).

**Accept (spec M1):** `cv init` → `cv build` → one-page PDF, selectable text, Gates green.

---

## Milestone 2 — Validation and errors

Goal: `schemars` schema, `cv schema`, `cv check`, `miette` diagnostics for all §5 error
classes; inline Markdown subset + escaping.

- [ ] 2.1 Semantic checks in `src/validate/` (everything the types can't express): font family
      exists (against the resolver), template exists, lengths/colors parse, URLs well formed,
      duplicate section titles (warning-level if we choose to have warnings — decide + record).
      Collect **all** errors, not just the first.
- [ ] 2.2 `miette` rendering with the YAML source and labeled spans; "did you mean" hints for
      unknown keys and near-miss font names (e.g. `Inte` → `Inter`). Match the quality bar of
      the §5 example output.
- [ ] 2.3 `cv schema`: print JSON Schema (derived, never hand-written) to stdout. Write
      `cv.schema.json` next to the YAML during `cv init` so the editor header works offline.
- [ ] 2.4 `cv check`: validate only, no PDF. Human-readable OK output; errors as above.
- [ ] 2.5 Inline Markdown subset (`**bold**`, `*italic*`, `` `code` ``, `[label](url)`),
      everything else literal. Implement per the decision from 1.5.2 (if tool-structured:
      parser + converter in the tool; if Typst-helper: shared `lib/inline.typ`).
- [ ] 2.6 Escaping guarantee: all Typst-special characters in user text escaped; user text can
      never inject Typst code. This is security-relevant — treat escaping tests as mandatory.
- [ ] 2.7 Error snapshot tests (`insta`) in `tests/errors.rs` for: unknown key, bad font,
      bad color, missing required field, wrong type — asserting line and column.
- [ ] 2.8 `cv schema` output matches a committed snapshot; all `examples/` files validate
      against the schema. Create `examples/minimal.yaml` and `examples/full.yaml` (spec §10).
- [ ] 2.9 Every user-facing error class has a test (spec §13.9).

**Accept (spec M2):** error snapshot tests pass; the editor-schema header autocompletes in
VS Code (verify manually with the written `cv.schema.json`).

---

## Milestone 3 — Design block

Goal: every `design` field visibly changes the PDF; bundled fonts embedded.

- [ ] 3.1 Wire all design fields through to the template: page size, margins, fonts (body/
      heading/mono), sizes, spacing, colors. Resolved (default-filled) `design` reaches the
      template — never missing values (spec §6).
- [ ] 3.2 Bundle fonts: Inter, IBM Plex Sans, IBM Plex Mono, Source Sans 3, JetBrains Mono.
      Record each license in `fonts/LICENSES.md`. Bundled fonts take priority over system
      fonts for reproducibility.
- [ ] 3.3 `cv fonts`: list available fonts (bundled + discovered), ideally marking source
      (bundled / `--font-path` / system).
- [ ] 3.4 `--font-path` (done in 1.6.3 as plumbing) verified end-to-end: a user font dir is
      discoverable and selectable.
- [ ] 3.5 Manifest enforcement: `cv check` warns when a template ignores design fields it
      declared unsupported (spec §6).
- [ ] 3.6 Tests: for each design field, a snapshot/diff test proving the PDF changes when the
      field changes (e.g. build twice with different accent color → different bytes; extract
      text-size or color where feasible; otherwise page-image snapshots for humans).
- [ ] 3.7 Re-render starter CV with several design combos and look at them (human review).

**Accept (spec M3):** changing each design field in YAML visibly changes the PDF; snapshot
tests cover it; `cv fonts` works.

---

## Milestone 4 — One-page fit

Goal: the fit loop per spec §7, `--no-fit`, overflow warnings. Termination and determinism
are the review-sensitive parts.

- [ ] 4.1 Implement `src/fit.rs` behind the render interface:
      1. First compile as-is. If it fits (1 page) → never shrink (hard requirement).
      2. Phase A: tighten `spacing.*` and margins in steps, floors: spacing scale 0.6,
         margins ≥ 1.0 cm.
      3. Phase B: scale font sizes in small steps (e.g. 0.02) down to the floor
         (`fit_floor.min_body_size` or `min_scale`, whichever is stricter).
      4. Prefer binary search over linear walk to limit compiles; pass current scale factors
         to the template via the `fit` object; templates multiply their own sizes/spacings.
- [ ] 4.2 Hard guarantees: maximum iteration count (define the constant, document it), never
      below the floor, deterministic result, pick the **least aggressive** setting that fits.
- [ ] 4.3 Overflow-at-floor behavior: still write the PDF, print a clear warning (lines
      overflowed + what to cut). Resolve the §14 exit-code question (0-with-warning vs.
      dedicated non-zero) and record it in `docs/decisions.md`.
- [ ] 4.4 `--no-fit` flag disables the loop for the run; success line reports applied scale.
- [ ] 4.5 Tests: overflowing CV fits after fitting; impossible CV hits the floor and warns;
      short CV is not shrunk; loop terminates within the max iteration count on adversarial
      input; same input → same scale factor (determinism).
- [ ] 4.6 Performance: typical CV build < 500 ms on a normal laptop (spec §12 M4). If over,
      optimize the fit loop first (compiler state reuse, fewer iterations), then font loading.
      Document the measured time.

**Accept (spec M4):** all fit-loop tests pass; build time under 500 ms.

---

## Milestone 5 — Templates and watch (working plan, M4 review merged)

Goal (spec §12 M5): `classic` template, custom template folders, manifest checks,
`cv templates`, `cv watch`. **Accept:** a user copies a built-in template folder, edits
it, and uses it via `design.template: ./my-template`.

Branch: `feature/m5-template-and-watch`. Typst types stay confined to `src/render/`: the
template loader and the watch loop only handle our own types (`Template`, `RenderOutput`).
The `notify` API is used per its pinned 8.2.2 docs (verified, rule 6), not from memory.

### Design decisions (each becomes an ADR)

**ADR-0027 — Template references and custom loading.** `design.template` is either a
built-in name or a path: the reference is a path when it contains `/` or `\` or starts
with `.` (`./my-template`, `../x`, `/abs/x`); anything else must name a built-in, so a
typo like `compactt` keeps its span-labeled "did you mean" diagnostic (snapshot-pinned).
Path references resolve against the YAML file's directory. The loader reads
`manifest.yaml` + `template.typ` + every `**/*.typ` in the folder into the same in-memory
`Template` shape built-ins use, so `src/render/` does not change. Purity (spec §6: no
network, no reads outside the folder) is enforcement by construction: the Typst world
only ever sees the virtual files the tool loaded, and the typst-as-lib package resolver
(network + fs cache) is never configured — a `@preview` import fails as a template error.
Missing folder / missing `template.typ` / missing or invalid `manifest.yaml` / non-UTF-8
sources are user-facing errors naming the folder and the file (each with a test).

**ADR-0028 — Manifest validation.** `name`, `description`, `schema_version` are required
by the serde type (unknown keys rejected). A template is only usable when `name` and
`description` are non-empty and `schema_version` is a version the tool understands (1),
checked on every load — mirrors the CV-level schema-version check. The plan's
"declared supported options consistent with what the template reads" is a heuristic run
for **custom templates only** (built-ins are pinned by unit tests instead, so shipped
templates can never gain new warnings): per design group, compare the manifest's
`supported` list against whether the sources contain `design.<group>`. A mismatch either
way is a warning (ADR-0015 class) at the `design.template` span.

**ADR-0029 — Watch semantics.** `cv watch` builds once at start, then loops: block on
the notify channel, drain events until 100 ms of quiet (the debounce), take a content
snapshot (per-file hash) of the YAML and the custom template folder, and rebuild only
when the snapshot actually changed. Event *paths* are not trusted to identify what
changed (FSEvents coalesces; inotify does not) — the snapshot does, which also kills any
rebuild loop from our own atomic PDF writes. A YAML-content change additionally
re-derives the watch set (the manifest reference may now point elsewhere) and
re-registers the watcher. Every iteration reuses `cv build`'s code path verbatim
(result line, warnings, exit-code-neutral overflow warning); build/render errors are
reported and never stop the loop — the user is editing the very file that is broken.
Ctrl-C (default signal handling) stops watching; no graceful-shutdown plumbing in v1.

**ADR-0011 amendment.** `cv init --template <name>` now writes the chosen built-in name
into the starter's `design.template` instead of always `compact` — with a second built-in
the flag has to select the template or it lies. It still accepts built-in names only.

### Tasks → commits

| # | Deliverable |
|---|---|
| 5.1 | `templates/classic/` (header rule, section dividers, all four section types, all six design groups, fit-scale contract identical to `compact`) + manifest + built-in parity unit tests + every-example-every-builtin build test (spec §11) |
| 5.2 | ~~`sidebar` template~~ — dropped: spec makes it optional ("skip without ceremony"), M5 acceptance does not need it, and a template nobody reviewed visually is liability, not feature |
| 5.3 | Reference grammar + `templates::load(reference, base)` dispatch + disk loader + `Error::TemplateLoad` + validation/build integration + ADR-0027; custom-template e2e suite (copy, tweak, build; every load error class) |
| 5.4 | Manifest usability checks (version, non-empty name/description) + declared-vs-reads warnings for custom templates + ADR-0028; tests |
| 5.5 | `cv templates` listing (padded columns like `cv fonts`) + `init --template` wiring + CLI surface test updates |
| 5.6 | `src/commands/watch.rs`: notify glue, quiet-period drain, content-snapshot dedupe, watch-set re-derivation + ADR-0029; unit tests for the snapshot/relevance logic |
| 5.7 | `tests/watch.rs` e2e: YAML edit triggers rebuild; custom-template edit triggers rebuild (bounded blocking reads, no sleeps) |
| 5.8 | Visual review of `classic`: render starter + examples, inspect, hand the PDFs to the human reviewer |

### Files touched

- new: `templates/classic/`, `src/commands/templates.rs`, `src/commands/watch.rs`,
  `tests/custom_template.rs`, `tests/watch.rs`
- changed: `src/templates/mod.rs`, `src/templates/manifest.rs`, `src/error.rs`,
  `src/validate/mod.rs`, `src/commands/{mod,build,init}.rs`, `src/cli.rs`,
  `src/lib.rs`(nothing expected), `tests/cli.rs`, docs
- risk notes for the human reviewer: `classic` visual quality (spec §13 reviewer note —
  tests do not prove it looks good); the watch loop's termination/rebuild-equality
  story; the custom-template error taxonomy.

### Gates

`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` per
commit; `cargo llvm-cov` at milestone end; no new crate dependencies (`notify` shipped
from M0 for exactly this task).

---

## Milestone 6 — Polish and release

Goal: README, exit codes, release binaries.

- [ ] 6.1 README: 60-second quick start (`init` → edit → `build`), example outputs (PDF
      screenshots or linked PDFs), full command/flag reference, documented exit codes table
      (including the fit-overflow code decided in 4.3).
- [ ] 6.2 Finalize `docs/decisions.md`: every §14 open question answered or explicitly
      deferred, each with a short rationale.
- [ ] 6.3 Fresh-machine test: download a release binary, run the quick start end-to-end.
- [ ] 6.4 Full audit pass against the spec: walk §3–§11 section by section and confirm each
      requirement has an implementation and (where applicable) a test. Fix or file gaps.
- [ ] 6.5 Review the code if it comes to code comments and code quality.
- [ ] Remove the horizontal line dividers in templates as we do not want that as our templates should be very clean, minimal and slick.

**Accept (spec M6):** a fresh machine can run the quick start using only the downloaded binary.

---

## Cross-cutting rules (apply to every milestone)

1. Gates before "done" (spec §11). Run them, don't assume.
2. Any new crate gets a one-sentence justification in the PR (spec §2).
3. Typst types stay in `src/render/` (spec §13.7).
4. No `unwrap()`/`expect()` outside tests and `main` startup, unless the invariant is
   commented (spec §13.8).
5. Ambiguity → simplest option, record in `docs/decisions.md`, flag for review.
6. Stop and wait for review after each milestone (spec §13.2). M0 is the only exception.

## Open-question assignment (spec §14)

| Question | Resolve by |
|---|---|
| Inline Markdown: tool-structured vs. Typst helper | M1 (step 1.5.2), blocks template text |
| Exit code when content can't fit at the floor | M4 (step 4.3) |
| Tagged PDF / accessibility support in pinned `typst-pdf` | M1 (step 1.4.5), finalize docs in M6 |
| `cv init` role-tuned examples vs. one neutral example | M6 (step 6.2) |
| Per-job variants (`--variant`) | Out of scope for v1; keep model extensible (note in M1 step 1.1.1) |

## Dependency notes

- `serde_yaml` is archived — use `serde_yaml_ng` (spec §2).
- Do not recall Typst APIs from memory; verify against the pinned version's docs before
  writing integration code (spec §2, §13.6).
- `typst-as-lib` only if it supports the pinned Typst version; otherwise a hand-written
  `World` in an isolated module (spec §2, §9).
