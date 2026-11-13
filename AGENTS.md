# AGENTS.md — working rules for agents and contributors

The spec is the source of truth: [`docs/details.md`](docs/details.md).
The work sequence lives in [`docs/implementation-plan.md`](docs/implementation-plan.md).
Progress is tracked in [`docs/progress.md`](docs/progress.md).
Decisions on ambiguous points are logged in [`docs/decisions.md`](docs/decisions.md).
Coding rules (how to write the code) live in [`.AGENTS.md`](.AGENTS.md).

## Working rules

1. Work one milestone at a time (spec §12). Stop for review after each milestone; M0 is the
   only exception.
2. Before declaring any task done, run the Gates and make them pass:

   ```
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

3. Never recall Typst or other crate APIs from memory. Check the docs.rs page for the exact
   pinned version, or compile a small experiment first (spec §2, §13.6).
4. Keep Typst types confined to `src/render/` (spec §13.7). The rest of the codebase must not
   import them.
5. No `unwrap()` / `expect()` outside tests and `main` startup, unless commented with the
   invariant (spec §13.8).
6. Every user-facing error must have a test (spec §13.9).
7. The code must be tested, aiming for 100% line coverage (`.AGENTS.md`, rule 21). Measure
   with `cargo llvm-cov` before declaring work done; unreached code needs a documented
   invariant comment.
8. When the spec is ambiguous, pick the simplest option that fits the goals, record it in
   `docs/decisions.md`, and flag it for review. Do not silently expand scope.
9. Do not add features outside the spec; propose them in the PR description instead (spec §13.5).
10. Every new crate dependency needs a one-sentence justification in the PR (spec §2).
11. Report at the end of each task: what changed, what was tested, what is uncertain (spec §13.10).

## Review-sensitive areas

- The Typst integration layer (`src/render/`) — spec §13.
- The one-page fit loop (termination, floor, determinism) — spec §7.
- Template visual quality: passing tests do not mean a template looks good; render and look at it.
