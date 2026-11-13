# M4 — One-page fit: working plan

Branch: `feature/m4-one-page-fit`. Spec §7 is the contract; §14's exit-code question is
resolved by ADR in task 4.3. Typst types stay confined to `src/render/`: the fit module
only ever handles `RenderOutput`/`FitParams`/`Engine` (our own types).

## Design decisions (each becomes an ADR)

**ADR-0024 — Search shape.** Two phases per spec order, driven by one "tightness"
parameter `t ∈ [0,1]` that couples spacing and margins linearly
(`spacing_scale = 1 − t·(1−0.6)`, `margin_scale = 1 − t·(1−margin_min)`; margin_min =
`max(1cm/x, 1cm/y)` capped at 1.0 so scaled margins never go below 1.0 cm).

1. Compile at identity. Page count ≤ 1 → return immediately, **never shrink**
   (the identity compile doubles as the `t=0` knowledge point — no recompile).
2. Phase A: bounded bisection (5 probes, deterministic midpoints `(lo+hi)/2`) for the
   **least aggressive** `t` that fits; then one explicit `t=1` probe if bisection found
   nothing.
3. Phase B (only when A's floors don't fit): bisection over `font_scale ∈ [floor, 1)`
   at `t=1`, largest fitting value = least aggressive; then one explicit floor probe.
   The font floor is `max(fit_floor.min_scale, min_body_size / sizes.body)` —
   "whichever is stricter" = the higher lower bound; clamped to ≤ 1.0, NaN-safe.
4. Termination is structural: at most `1 + (5+1) + (5+1) = 13` compiles per fit.
   `MAX_PHASE_PROBES = 5` → grid resolution 3.1 % of the range (≤ 0.5 % for the font
   scale — finer than the spec's 0.02 example step). No wall-clock, no
   input-dependent branching: deterministic.
   **Caveat documented:** bisection presumes monotonic "shrinks more ⇒ fits";
   rounding-induced non-monotonicity (rare) still yields a *fitting* result,
   possibly not the least aggressive.
   After Phase B succeeds, Phase A is not re-explored (spec's ordering ⇒ spacing
   before font size).
5. Line-height tightening: `line-height' = 1 + (design − 1)·spacing_scale` in the
   template — never below 1.0 (a literal `line_height × 0.6` would overlap lines);
   documented as the interpretation of "spacing.*" for the dimensionless knob.

**ADR-0025 — Success line (amends ADR-0012).** `fit: scale F` when only fonts moved
(or nothing); `fit: scale F spacing S margins M` when phase A tightened anything.
`F = 1.00` ⇒ not shrunk ⇒ single number, byte-compatible with today's line for
first-fit CVs. When fitting is disabled (`--no-fit` / `fit_one_page: false`) the line
still reports the applied scales (`scale 1.00`).

**ADR-0026 — Overflow at floor (resolves §14 question).** **Exit 0 with a clear
warning**, PDF written: consistent with §3 ("non-zero on any *error*" — an overflowed
build is a produced document, not an error) and with ADR-0015 (warnings never affect
the exit code). Warning text: `content still overflows at the fit floor: about N lines
spill onto P extra page(s); cut content (e.g. bullets/paragraphs) or relax
design.fit_floor / margins.` — `N` is the count of shaped text runs (`FrameItem::Text`)
on pages 2..n, computed inside `src/render/` and exposed as a new
`RenderOutput::overflow_lines` field; it slightly overestimates mixed-style lines
(documented approximation).

## Tasks → commits

| # | Deliverable |
|---|---|
| 4.1 | `src/fit.rs` (phases + searches + `FitConfig::from_design`), `Length::to_pt` in the model, build path reuses one `Engine` across compiles, template line-height scaling, ADR-0024/0025. First e2e tests: long CV fits after shrinking, short CV untouched |
| 4.2 | Hard guarantees pinned: unit tests for the floors, probe-count bounds, least-aggressive selection, determinism, error propagation through the searches |
| 4.3 | `overflow_lines` counting in `src/render/`, build-time warning, exit code 0, ADR-0026 + open-question table update, e2e impossible-CV test |
| 4.4 | `--no-fit` + `design.fit_one_page: false` wiring and tests (no loop, no overflow warning, scale 1.00) |
| 4.5 | Test sweep: adversarial/huge CV termination e2e, byte-identical overflowing builds, floor-anchored success line (`0.89/0.60/0.83` for defaults), margin-floor behaviour |
| 4.6 | Release-build timing of a typical + an overflowing CV, measured numbers documented |

## Files touched

- new: `src/fit.rs`, `tests/fit.rs`
- changed: `src/lib.rs`, `src/model/value.rs`, `src/render/mod.rs`, `src/commands/build.rs`,
  `templates/compact/template.typ`, docs
- risk notes for the human reviewer: `src/fit.rs` termination/floor math (spec §7,
  AGENTS review-sensitive), `overflow_text_runs` frame walk (new Typst API use, plan
  1.4.1 rule), template line-height change (visual quality, 3.7-style check).

## Gates

`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` per
commit; `cargo llvm-cov` at milestone end; performance measured on `--release`.
