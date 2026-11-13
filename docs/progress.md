# Progress

Status tracker. Task descriptions live in [`implementation-plan.md`](implementation-plan.md);
this file only records what is done. Mark a task `[x]` only when it is implemented **and**
the Gates pass (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` — run locally, ADR-0013). Strikethrough tasks are dropped decisions.

- **Current state:** M2 implemented — Gates green; M1 human visual check (1.7.4) and the
  M1 review stop still pending, plus the M2 review stop.

## Milestone 0 — Project bootstrap ✅

- [x] 0.1 Repository skeleton (spec §10 layout)
- [x] 0.2 Dependencies in `Cargo.toml` with justifications
- [x] ~~0.3 CI on every push/PR~~ — dropped (ADR-0013): no CI/CD; Gates run locally
- [x] 0.4 CLI commands stubbed, shape testable from day one (ADR-0004)
- [x] 0.5 Gates pass; `cargo run -- --help` shows the full command surface

## Milestone 1 — Skeleton and first PDF 🔜 review

- [x] 1.1.1 Content model (`Document`, `Cv`, `Contact`, `Link`, `Section`, entry types; §4)
- [x] 1.1.2 `design` model with Rust-side defaults; missing `design` → defaults
- [x] 1.1.3 Strict value types (lengths, colors); dates/URLs as strings for M2
- [x] 1.1.4 `JsonSchema` derived on everything
- [x] 1.1.5 Model unit tests (example parses; missing design; unknown keys/types rejected)
- [x] 1.2.1 YAML parsing keeping locations for later `miette` labels
- [x] 1.2.2 `thiserror` → `miette` at the CLI boundary (done with M2 diagnostics)
- [x] 1.3.1 `cv init` starter YAML (`--template`, `--out`, `--force`; ADR-0011)
- [x] 1.3.2 `yaml-language-server` header line
- [x] 1.3.3 Round-trip and no-overwrite tests
- [x] 1.4.1 Typst stack pinned (`typst`/`typst-pdf`/`typst-kit` `=0.15.1`, `typst-as-lib`
      0.17 — ADR-0005)
- [x] 1.4.2 In-memory render interface (`RenderRequest` → `RenderOutput`)
- [x] 1.4.3 Data contract: virtual `data.yaml`, no generated Typst source (ADR-0006)
- [x] 1.4.4 Font priority user → bundled → system (ADR-0007); fonts load once
- [x] 1.4.5 PDF determinism; tagged PDFs on (ADR-0009)
- [x] 1.5.1 `compact` template (`template.typ` + `manifest.yaml`, all section types)
- [x] 1.5.2 Inline-Markdown decision: plain text in M1, tool-structured in M2 (ADR-0008)
- [x] 1.5.3 Built-in template loading (custom paths deferred to M5)
- [x] 1.6.1 `cv build` defaults, `--output`, silent overwrite, atomic write
- [x] 1.6.2 One-line success output (ADR-0012)
- [x] 1.6.3 Global flags `--font-path`, `--quiet`, `--verbose`
- [x] 1.6.4 Template errors labeled distinctly from user errors
- [x] 1.7.1 End-to-end: init → build → 1-page PDF, selectable text
- [x] 1.7.2 Byte-identical determinism test
- [x] 1.7.3 Atomic-write test (failing build leaves PDF untouched)
- [ ] 1.7.4 **Human visual check** — render the starter CV and look at it; then M1 review

## Milestone 2 — Validation and errors 🔜 review

- [x] 2.1 Semantic checks in `src/validate/`: fonts, template, URLs, schema version;
      duplicate titles warn (ADR-0015); all errors collected (ADR-0014 span scan)
- [x] 2.2 `miette` rendering with spans + "did you mean" hints (ADR-0018; spec §5 bar)
- [x] 2.3 `cv schema`; `init` writes `cv.schema.json` (ADR-0019)
- [x] 2.4 `cv check` (warnings never affect the exit code)
- [x] 2.5 Inline Markdown subset (ADR-0016): `src/markdown.rs`, tokens in `data.yaml`,
      `templates/compact/lib/inline.typ`
- [x] 2.6 Escaping guarantee: user text only reaches Typst as literal strings/token
      fields; covered by markdown parser tests + example PDF text extraction
- [x] 2.7 Error snapshot tests (`insta`, `tests/errors.rs`) — message, line:column,
      snippet and hint pinned
- [x] 2.8 Schema snapshot; `examples/minimal.yaml` + `examples/full.yaml` (ADR-0020)
- [x] 2.9 Every user-facing error class has a test (mapping documented in
      `tests/errors.rs` docs)

Measured coverage (`cargo llvm-cov`, end of M2): 97.2% lines; every remaining uncovered
line is a defensive branch carrying an invariant comment (unreachable embedded assets,
span-scan fallbacks, disk-failure paths), per `.AGENTS.md` rule 21.

## Milestone 3 — Design block

- [x] 3.1 All design fields wired to the template
- [x] 3.2 Bundled fonts + `fonts/LICENSES.md`
- [x] 3.3 `cv fonts`
- [x] 3.4 `--font-path` end-to-end
- [x] 3.5 Manifest enforcement in `cv check`
- [x] 3.6 Per-field change tests
- [ ] 3.7 Human visual review of design combos

## Milestone 4 — One-page fit

- [ ] 4.1 Fit loop (Phase A spacing → Phase B font scale; binary search)
- [ ] 4.2 Hard guarantees (max iterations, floor, determinism, least-aggressive)
- [ ] 4.3 Overflow-at-floor behavior + §14 exit-code decision (ADR)
- [ ] 4.4 `--no-fit`
- [ ] 4.5 Fit-loop tests (termination, floor, determinism, no-shrink-when-fits)
- [ ] 4.6 Build < 500 ms; document measured time

## Milestone 5 — Templates and watch

- [ ] 5.1 `classic` template
- [ ] 5.2 `sidebar` template (optional)
- [ ] 5.3 Custom template folders
- [ ] 5.4 Manifest validation for custom templates
- [ ] 5.5 `cv templates`
- [ ] 5.6 `cv watch`
- [ ] 5.7 Custom-template and watch tests
- [ ] 5.8 Human visual review of `classic`

## Milestone 6 — Polish and release

- [ ] 6.1 README (quick start, flags, exit codes)
- [ ] 6.2 Close all §14 open questions
- [ ] 6.3 Release builds for Linux, macOS (x64 + arm64), Windows — locally, no CI
      (ADR-0013)
- [ ] 6.4 Fresh-machine test
- [ ] 6.5 Full audit pass against the spec
