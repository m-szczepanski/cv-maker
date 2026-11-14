# Progress

Status tracker. Task descriptions live in [`implementation-plan.md`](implementation-plan.md);
this file only records what is done. Mark a task `[x]` only when it is implemented **and**
the Gates pass (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` — run locally, ADR-0013). Strikethrough tasks are dropped decisions.

- **Current state:** M6 implemented — all tasks done, Gates green; M6 ready for
  review/merge. The M1 visual check (1.7.4) and the M1/M2 review stops are still open.

## Milestone 0 — Project bootstrap 

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

## Milestone 3 — Design block 🔜 review

- [x] 3.1 All design fields wired to the template
- [x] 3.2 Bundled fonts + `fonts/LICENSES.md`
- [x] 3.3 `cv fonts`
- [x] 3.4 `--font-path` end-to-end
- [x] 3.5 Manifest enforcement in `cv check`
- [x] 3.6 Per-field change tests
- [x] 3.7 Human visual review of design combos — eight combos rendered (baseline,
      letter, Source Sans 3, IBM Plex Sans variable, red accent, tight, large,
      Plex Mono code spans) on 2026-11-13; human pass completed, approved.

Measured coverage (`cargo llvm-cov`, end of M3): 97.4% lines; every remaining
uncovered line is a defensive branch carrying an invariant comment (unreachable
embedded assets, span-scan fallbacks, disk-failure paths), per `.AGENTS.md` rule 21.
The `cv fonts` listing, manifest enforcement and the per-field tests are fully
covered.

## Milestone 4 — One-page fit 🔜 review

- [x] 4.1 Fit loop (Phase A spacing → Phase B font scale; binary search)
- [x] 4.2 Hard guarantees (max iterations, floor, determinism, least-aggressive)
- [x] 4.3 Overflow-at-floor behavior + §14 exit-code decision (ADR)
- [x] 4.4 `--no-fit`
- [x] 4.5 Fit-loop tests (termination, floor, determinism, no-shrink-when-fits)
- [x] 4.6 Build < 500 ms; document measured time — release build on the dev machine
      (Apple silicon): typical first-fit **197 ms** (233 ms first run incl. warm-up),
      a fitted shrink build 216 ms, worst-case 13-compile overflow fit 309 ms.
      Spec §12 M4 target met; the extra probes are cheap because fonts, the template
      and the compiler state load once per build (plan 1.4.4).

Measured coverage (`cargo llvm-cov`, end of M4): 97.8% lines; the residual uncovered
lines are all pre-existing defensive branches carrying invariant comments (unreachable
embedded assets, span-scan fallbacks, disk-failure paths, the M5-reserved Typst-warning
loop), per `.AGENTS.md` rule 21. The fit loop, overflow warning and floor math are
fully covered — the first coverage pass flagged three reachable gaps (`body: 0pt`
floor guard, the Phase-A floor-probe return, the verbose telemetry line), closed by
tests in this commit.

## Milestone 5 — Templates and watch 🔜 review

- [x] 5.1 `classic` template (header rule + section dividers; all four section
      types; all six design groups; same fit contract as `compact` — pinned by
      `the_fit_loop_shrinks_classic_like_compact` and the built-in parity tests)
- [x] 5.2 ~~`sidebar` template~~ — dropped (optional per spec; acceptance needs
      custom templates, not a third unreviewed visual)
- [x] 5.3 Custom template folders (`design.template: ./my-template`, resolved
      against the CV file; pure by construction — no fs/package resolver; ADR-0027)
- [x] 5.4 Manifest validation (required fields via serde; usability — version +
      non-empty identity — on every load; declared-vs-reads warnings for custom
      templates; built-in agreement pinned by unit tests; ADR-0028)
- [x] 5.5 `cv templates` (aligned built-in listing; `init --template` now writes
      the chosen name — ADR-0011 amendment)
- [x] 5.6 `cv watch` (notify + drain-until-quiet debounce; content-snapshot dedupe
      so PDF writes can't loop; catch-up rebuild; target re-derivation; ADR-0029)
- [x] 5.7 Custom-template and watch tests (`tests/custom_template.rs`,
      `tests/watch.rs`; every user-facing template-load error class has a test)
- [x] 5.8 **Human visual review of `classic`** — starter (A4 + letter/red), the
      `full` example, and a 14×2 overflowing CV fitted to one page were rendered;
      agent inspection plus human pass 2026-11-13, approved. One reviewer note:
      the full-width divider lines in the templates are too heavy for taste —
      deferred to a later styling pass (see M6 note below).

M6 reviewer note (carried from the 5.8 pass): soften/remove the full-width
`line()` dividers in the built-in templates (header rule + section dividers) —
reviewer would like them lighter or gone; treat as template styling polish, not
a schema/contract change.

Measured coverage (`cargo llvm-cov`, end of M5): 96.6% lines. The residual
uncovered lines are the documented defensive classes (unreachable embedded assets,
span-scan fallbacks, disk-failure paths) plus the Typst-warning loop, now reached
by a custom template. The `cv watch` `run` glue reads as 0% only because llvm-cov
cannot collect the profile of the SIGKILLed e2e subprocess; every decision it makes
(wake, snapshot equality, catch-up convergence, target re-pointing, watch sync) is
unit-tested in `src/commands/watch.rs` (see its coverage note).

## Milestone 6 — Polish and release 🔜 review

- [x] 6.1 README (`README.md`): user-centred — what the tool does, install (release
      archives per platform, or `cargo build --release`), the 60-second quick start, the
      `design`/section/formatting reference, commands and flags, how the one-page fit
      behaves and what its lines mean, templates (built-in and custom folders), fonts,
      the error quality bar with an example, and the exit-code table (0 / 1 / 2).
      Architecture and crate details deliberately left out.
- [x] 6.2 §14 open questions closed in `docs/decisions.md`: tagged PDF 1.7 with no
      PDF/A or PDF/UA validator (ADR-0038), one neutral `cv init` example now showing
      the fit knobs (ADR-0039), variants confirmed non-blocking; the status table has
      no open rows left.
- [x] 6.3 Release builds, local matrix (ADR-0037): `scripts/build-release.sh` +
      `cargo-zigbuild`/`zig` (dev tooling only; no crate dependencies added) produce
      `dist/cv-0.1.0-{x86_64,aarch64}-apple-darwin.tar.xz`,
      `dist/cv-0.1.0-{x86_64,aarch64}-unknown-linux-gnu.tar.xz`,
      `dist/cv-0.1.0-x86_64-pc-windows-gnu.zip` and `dist/SHA256SUMS.txt`. Not published
      (reviewer's call), `dist/` is gitignored.
- [x] 6.4 Fresh-machine test: `scripts/fresh-machine-test.sh` unpacks an archive, verifies
      its checksum, then runs the README quick start from the binary alone in an isolated
      `HOME` with `PATH=/usr/bin:/bin` and no access to the repository (the harness
      itself needs no Rust toolchain: it reads the version from `Cargo.toml`): `templates`,
      `init`, `check`, `build` → one-page PDF, byte-identical rebuild, starter schema
      written. Passes for the host triple (Apple silicon); the x86_64 and cross-OS
      artifacts are built and checksum-verified but only *structure-checked* here, since
      this machine has no Rosetta, Docker or Wine (ADR-0037 consequence).
- [x] 6.5 Full audit pass against spec §3–§11, gaps fixed with tests:
      - §5/§8: `cv check` was *stricter* than rendering on font case (Typst matches
        family names case-insensitively) — validation now agrees, and `inter` builds
        byte-identically to `Inter` (ADR-0031).
      - §5: a Markdown link with an empty URL passed `cv check` and then
        failed `cv build` as a *template* error; it is reported by `cv check` now, at the
        field where it is written (ADR-0033).
      - §5: the span scan lost alignment on YAML aliases, so a diagnostic for a value
        after an aliased one pointed at the wrong line — aliases now take their frame
        slot and report without a snippet (ADR-0033).
      - §7/ADR-0012: `cv build --quiet` swallowed Typst render warnings (progress and
        essentials were conflated) — fixed (ADR-0032).
      - §11: the required escaping class had no e2e test — `tests/escaping.rs` renders
        `#`, `$`, `*`, `_`, `\`, `@`, backticks and `#panic(...)`/`#show`/`#{ … }`
        constructs and asserts they stay literal text and one page.
      - §3: piping a listing (`cv fonts | head -1`) panicked on the closed pipe; stdout
        and stderr writes are best-effort now and the run exits 0 (test added; the review
        round found the stderr half was not actually covered — see 6.8).
      - §9: templates and user fonts were each read twice per build (validation, then
        render) — the resolved template and the font files now travel with `LoadedCv`,
        so a build reads each once and the render layer holds no unreachable I/O arms
        (ADR-0034/0035).
      - §9: the claim that `typst_pdf::pdf` can only fail on option misuse was wrong —
        it inspects the document while writing it, so a template calling `pdf.attach`
        with a bad MIME type compiles and is then refused. That is its own error class,
        tested at the unit and CLI level; the atomic write also flushes before it
        renames, so no buffered tail can be published (ADR-0040).
      - §3: `--help` was quoting our own documents (`(spec §3)`, `(M5)`) at users; the
        wording is user-facing now and the spec mapping stays in the code comments.
      - Spec §6 text and the `classic` manifest description now match the shipped
        template; §3 documents the exit codes the README lists.
- [x] 6.6 Code-quality and comment pass (plan item, plus the 5.8 reviewer note): the
      full-width `line()` rules are gone from `classic` — both templates were re-rendered
      and inspected (starter, `full`, and a 14×2 overflowing CV fitted to one page) —
      ADR-0030. Defensive branches that could not be reached were deleted, turned into
      stated invariants, or given a test seam (`assemble_builtin`, `WatchFolders`,
      injected watcher factory) instead of being documented as exceptions (ADR-0036).
      Stale milestone narration was removed from doc comments; `matches!`-style test
      assertions that could only ever show a dead branch were rewritten.
- [x] 6.7 Build time re-measured after the M6 changes (M4 target < 500 ms): release build
      on this machine, typical first-fit **145–201 ms** on a warm tree (232 ms cold), so
      the extra fidelity in the fit loop and the single-read refactor cost nothing
      measurable. Re-measured once more on the review-round artifacts.
- [x] 6.8 Review round — eight findings from reading the branch back, each reproduced
      before being fixed and each with a test now:
      - **stderr panicked on a hung-up reader** (`cv check bad.yaml 2>&1 | head -1`,
        157 KB of diagnostics, exit 101). The M6 pipe fix had only covered stdout. Every
        diagnostic path now writes through `out_line`/`err_line`/`err_text` and ignores
        `EPIPE` (ADR-0041); the regression test runs the real pipeline on both streams.
      - **An unreadable font directory hid every other error**: `load_cv` returned the
        I/O error before validation ran, so a stale `design.font_paths` entry masked
        schema and URL problems (spec §5 wants all of them). Validation reads the
        directories and reports the failure as a diagnostic among the rest, handing the
        bytes back on success (ADR-0034 amended).
      - **The fresh-machine harness could pass a broken artifact**: it decided "wrong
        architecture" by failing to exec, so a corrupt or Gatekeeper-blocked host binary
        exited 0. Runnability is now read from the triple in the file name; a host-triple
        artifact that will not run is a hard failure, and `.zip` archives unpack with
        `unzip` so the script works on Linux too (ADR-0037 amended, verified against a
        deliberately corrupt archive).
      - **`mailto:` links were rejected** — the first version of the Markdown URL check
        applied the contact-link http(s)-only rule, which outlawed content that rendered
        and worked (a CV built at M5 stopped building). Severity now follows the
        renderer: empty target = error, unresolvable target = warning, any absolute
        scheme = accepted (ADR-0033 amended).
      - **`cv templates` could panic** on a stray embedded folder, because
        `builtin_names()` listed every directory under `templates/`. One predicate
        (`template.typ` at the folder root) now defines what a built-in is, used by both
        the listing and `load_builtin`, so a non-template folder is simply not a known
        name — and a bundle that is a template but fails to load stays a typed internal
        error rather than a crash (ADR-0042).
      - The harness's own version-prefix strip then misread an archive labelled with a
        different version than the checkout; the triple is parsed by position instead, and
        — because a name it cannot parse silently became an accepted "not my architecture"
        skip — it is checked against `scripts/release-targets`, which both release scripts
        read. Verified: a corrupt host artifact exits 1, an unclassifiable name exits 1, a
        foreign triple skips, and `build-release.sh` rejects an unknown target.
      - A robustness pass over those two fixes found four more weak spots in the new code
        itself (ADR-0037): the triple match was still a regex built from a file name, so
        `cv-1-aarch64.*darwin.tar.xz` impersonated `aarch64-apple-darwin` and would have
        been skipped as "another architecture" — fixed-string matching now; the build
        script's target argument had the same pattern-from-user-input shape; an empty
        artifact glob aborted the checksum step (or hashed an empty stream) after the
        checksum file was deleted; and `shasum` made the Linux claim depend on perl.
        Verified with a battery of eight probes: host artifact runs, foreign triples skip,
        crafted name refused, tampered archive mismatch-refused, not-listed name refused,
        metachar target arguments refused, one-target rebuild reproduces the committed
        checksums byte for byte, and `shasum -a 256 -c` works as the README says.
      - **Release archives were not reproducible**: rebuilding the same commit hashed
        differently although the binary inside was byte-identical (member order, mtimes,
        uid). Members are now sorted with pinned timestamps and two full matrix builds
        produce an identical `SHA256SUMS.txt` (ADR-0037). The first attempt used
        GNU-tar-only flags that macOS's bsdtar rejected — and my check of it passed for
        the wrong reason, comparing a checksum file the failed build had never rewritten,
        which is why the verification now asserts on a successful build.
      - Three low-severity accuracy fixes: ADR cross-references that cited ADR-0034 for
        ADR-0033's work, a `to_yaml` invariant comment claiming all numbers are finite
        (they are not — nonsense fit floors serialize as `.nan` and are clamped by the
        fit loop), and README claims on link targets and on byte-identical output across
        machines (true for bundled and user-supplied fonts, not for system-resolved ones).

Measured coverage (`cargo llvm-cov`, re-run against the committed M6 tree after the
review round): **100.00% lines and 100.00% functions** (4028 lines, 555 functions; lines
were 96.67% at the end of M5), 99.95% regions — `--show-missing-lines` reports nothing at
all. The three regions still counted as missed are not untested code: the `?`
continuation on `write_atomic`'s call into the write sink (`src/commands/build.rs:108`),
where the disk fault is injected one level down instead (the `write_to` sink tests); the
`?` on `cv templates`' collect (`src/commands/templates.rs:15`), which is the price of
reporting a broken bundle as a typed error instead of panicking (ADR-0042); and a
numeric-literal sub-region inside `fit.rs`'s floor guards, whose lines execute (`80|1`,
`97|1`) and whose behaviour is asserted by
`margins_already_below_the_floor_are_never_shrunk` and
`a_zero_body_size_floors_the_font_phase_at_the_original_scale`.

Everything previously parked as "defensive branch, cannot reach" is now covered by a
real test rather than a comment: broken built-in asset shapes via `assemble_builtin` over
synthetic file lists; span-less aliased values via `diag_at` plus an e2e snapshot; the
watch session's start, wake, rebuild-equality, catch-up, refused registration and dead
channel exits in-process; a full/failing disk via the write sink; and a PDF the exporter
refuses via `pdf.attach` with a malformed MIME type (ADR-0040).
