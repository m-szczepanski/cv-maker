# Decisions

ADR-style log. Each entry: context, decision, consequences. The spec (`docs/details.md`) is
the source of truth; if a decision here contradicts it, fix one of them and say which
(spec header note).

## ADR-0001 — Toolchain: stable channel, edition 2024

- **Context:** Spec §2 says "Rust (stable, 2021 or 2024 edition), pin with `rust-toolchain.toml`".
- **Decision:** `rust-toolchain.toml` pins `channel = "stable"` (not an exact version); the
  crate uses edition 2024. CI installs the stable toolchain.
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
- **Consequences:** CI exercises the actual CLI surface in every run from M0 on.

---

## Open questions (spec §14) — status

Assigned to milestones per the implementation plan; each gets an ADR when resolved.

| Question | Status |
|---|---|
| Inline Markdown: tool-structured vs. Typst helper | open — decide in M1 (step 1.5.2) |
| Exit code when content can't fit at the floor | open — decide in M4 (step 4.3) |
| Tagged PDF / accessibility support in pinned `typst-pdf` | open — check in M1 (step 1.4.5), finalize M6 |
| `cv init` role-tuned examples vs. one neutral example | open — decide in M6 (step 6.2) |
| Per-job variants (`--variant`) | out of scope for v1; keep model extensible |
