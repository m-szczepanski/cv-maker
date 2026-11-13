//! One-page fitting loop (spec §7, plan 4.1–4.2; search shape in ADR-0024).
//!
//! Strategy, in spec order:
//!
//! 1. Compile as-is. If it fits (one page), **never shrink** — the identity compile
//!    doubles as the "t = 0 is infeasible" knowledge for the search.
//! 2. Phase A — tighten `spacing.*` and margins together along one linear "tightness"
//!    `t`, floors: spacing scale 0.6, margins never below 1.0 cm.
//! 3. Phase B — only if A's floors still overflow: shrink `font_scale` toward its
//!    floor (`fit_floor.min_scale` and `min_body_size / sizes.body`, whichever is
//!    stricter, i.e. the higher lower bound) at `t = 1`.
//! 4. The searches are bounded bisections probing deterministic midpoints
//!    (`MAX_PHASE_PROBES` each, plus one explicit floor probe), so they terminate by
//!    construction, never cross a floor, and return the least aggressive *grid
//!    point* that fits (monotonicity of "shrinks more ⇒ fits" is assumed; where
//!    rounding breaks it locally the result still fits, see ADR-0024).
//!
//! No Typst types here: the loop only sees page counts of [`RenderOutput`]s, the
//! scales reach the template through the `fit` inputs (`src/render/`), and the
//! compile step is an injectable closure so the review-sensitive guarantees
//! (termination, floors, least aggressiveness, determinism) are pinned by unit
//! tests.

use crate::model::design::{Design, Margins};
use crate::model::value::Length;
use crate::render::{Engine, FitParams, RenderError, RenderOutput};

/// Phase A floor for `spacing_scale` (spec §7: "spacing scale 0.6").
pub const SPACING_FLOOR: f64 = 0.6;

/// Phase A floor for every page margin (spec §7: "margins ≥ 1.0 cm"), in points.
pub const MARGIN_FLOOR_PT: f64 = 72.0 / 2.54;

/// Interior bisection probes per phase. 5 probes resolve the phase range to
/// 1/32 ≈ 3.1 % (font scale: ≤ 0.5 % of its range, finer than the spec's 0.02
/// example step). Worst-case compile count: `1 + (5 + 1) + (5 + 1) = 13`.
pub const MAX_PHASE_PROBES: usize = 5;

/// Everything the loop needs from the (resolved) design block, precomputed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitConfig {
    /// Run the loop at all (`design.fit_one_page`, overridable by `--no-fit`).
    pub enabled: bool,
    /// Smallest `margin_scale` Phase A may use, so margins stay ≥ 1.0 cm.
    pub margin_min_scale: f64,
    /// Smallest `font_scale` Phase B may use.
    pub font_floor: f64,
}

impl FitConfig {
    /// Derive the loop parameters from a fully resolved design block.
    pub fn from_design(design: &Design) -> Self {
        Self {
            enabled: design.fit_one_page,
            margin_min_scale: margin_min_scale(&design.margins),
            font_floor: font_floor_scale(design),
        }
    }
}

/// The chosen scales, the PDF they produced, and loop telemetry.
#[derive(Debug)]
pub struct FitOutcome {
    pub output: RenderOutput,
    pub applied: FitParams,
    /// Compiles performed — bounded by `1 + 2 * (MAX_PHASE_PROBES + 1)`.
    pub compiles: usize,
    /// Content still overflowed at every floor (spec §7 step 4; exit stays 0,
    /// ADR-0026).
    pub overflowed: bool,
}

/// `margin_scale` floor: both margins stay ≥ `MARGIN_FLOOR_PT` after scaling
/// (the binding constraint is the *smaller* margin), and shrinking never starts
/// below the original size (cap 1.0).
fn margin_min_scale(margins: &Margins) -> f64 {
    let per = |m: Length| -> f64 {
        let pt = m.to_pt();
        if !pt.is_finite() || pt <= MARGIN_FLOOR_PT {
            1.0
        } else {
            MARGIN_FLOOR_PT / pt
        }
    };
    per(margins.x).max(per(margins.y)).clamp(0.0, 1.0)
}

/// Font floor: `min_body_size / body_size` and `min_scale`, whichever is the
/// higher lower bound (spec §7 "whichever is stricter"), clamped so a nonsensical
/// value (≥ 1 or not finite) can only mean "never shrink fonts".
fn font_floor_scale(design: &Design) -> f64 {
    let body = design.sizes.body.to_pt();
    let min_body = design.fit_floor.min_body_size.to_pt();
    let size_floor = if body > 0.0 && min_body.is_finite() {
        (min_body / body).clamp(0.0, 1.0)
    } else {
        1.0
    };
    size_floor.max(design.fit_floor.min_scale.clamp(0.0, 1.0))
}

/// Run the fitting loop against a prepared engine (fonts and sources stay loaded;
/// plan 1.4.4). With `config.enabled == false` this is exactly one identity compile.
pub fn fit(engine: &Engine, config: &FitConfig) -> std::result::Result<FitOutcome, RenderError> {
    Session::new(config, |params| engine.compile(params)).run()
}

/// Scale triple for a tightness `t` and font `font_scale` (ADR-0024): at `t = 0`
/// nothing is tightened, at `t = 1` both Phase A floors are reached, in between
/// the two scales move linearly together.
fn scales_at(margin_min_scale: f64, font_scale: f64, t: f64) -> FitParams {
    FitParams {
        font_scale,
        spacing_scale: 1.0 - t * (1.0 - SPACING_FLOOR),
        margin_scale: 1.0 - t * (1.0 - margin_min_scale),
    }
}

fn fits(output: &RenderOutput) -> bool {
    // A paged document always has ≥ 1 page; `<= 1` only guards the impossible 0.
    output.page_count <= 1
}

struct Session<'a, C>
where
    C: FnMut(FitParams) -> std::result::Result<RenderOutput, RenderError>,
{
    config: &'a FitConfig,
    compile: C,
    compiles: usize,
}

impl<'a, C> Session<'a, C>
where
    C: FnMut(FitParams) -> std::result::Result<RenderOutput, RenderError>,
{
    fn new(config: &'a FitConfig, compile: C) -> Self {
        Self {
            config,
            compile,
            compiles: 0,
        }
    }

    /// One compile at the mapped scales, counted for telemetry.
    fn call(&mut self, font_scale: f64, t: f64) -> std::result::Result<RenderOutput, RenderError> {
        self.compiles += 1;
        (self.compile)(scales_at(self.config.margin_min_scale, font_scale, t))
    }

    /// Compile split into `(fits, output)` for the searches.
    fn probe(
        &mut self,
        font_scale: f64,
        t: f64,
    ) -> std::result::Result<(bool, RenderOutput), RenderError> {
        let output = self.call(font_scale, t)?;
        Ok((fits(&output), output))
    }

    fn outcome(&self, output: RenderOutput, applied: FitParams, overflowed: bool) -> FitOutcome {
        FitOutcome {
            output,
            applied,
            compiles: self.compiles,
            overflowed,
        }
    }

    fn run(&mut self) -> std::result::Result<FitOutcome, RenderError> {
        let identity = self.call(1.0, 0.0)?;
        if !self.config.enabled || fits(&identity) {
            // Hard requirement (spec §7): never shrink what already fits.
            return Ok(self.outcome(identity, FitParams::default(), false));
        }

        let margin_min = self.config.margin_min_scale;

        // Phase A: least aggressive tightness that fits, fonts at full size.
        if let Some((t, output)) =
            search_min_feasible(MAX_PHASE_PROBES, &mut |t| self.probe(1.0, t))?
        {
            return Ok(self.outcome(output, scales_at(margin_min, 1.0, t), false));
        }

        let (floors_fit, at_floors) = self.probe(1.0, 1.0)?;
        if floors_fit {
            return Ok(self.outcome(at_floors, scales_at(margin_min, 1.0, 1.0), false));
        }

        // Phase B: largest font scale that fits at full tightness.
        let floor = self.config.font_floor;
        if floor >= 1.0 {
            // Font shrinking is floored at the original size: the Phase A floor
            // probe is already the most aggressive compile (spec §7 never lets
            // the loop grow content).
            return Ok(self.outcome(at_floors, scales_at(margin_min, 1.0, 1.0), true));
        }
        if let Some((f, output)) =
            search_max_feasible(floor, MAX_PHASE_PROBES, &mut |f| self.probe(f, 1.0))?
        {
            return Ok(self.outcome(output, scales_at(margin_min, f, 1.0), false));
        }

        let (fits_floor, at_floor) = self.probe(floor, 1.0)?;
        Ok(self.outcome(at_floor, scales_at(margin_min, floor, 1.0), !fits_floor))
    }
}

/// Smallest `t` in `(0, 1]` that the probe accepts, probed at deterministic
/// bisection midpoints. Assumes `t = 0` is infeasible. Returns `None` when no
/// probed point fits.
fn search_min_feasible<T, E>(
    max_probes: usize,
    probe: &mut impl FnMut(f64) -> std::result::Result<(bool, T), E>,
) -> std::result::Result<Option<(f64, T)>, E> {
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let mut best: Option<(f64, T)> = None;
    for _ in 0..max_probes {
        let mid = (lo + hi) / 2.0;
        let (ok, value) = probe(mid)?;
        if ok {
            hi = mid;
            best = Some((mid, value));
        } else {
            lo = mid;
        }
    }
    Ok(best)
}

/// Largest `f` in `(floor, 1)` that the probe accepts, at deterministic
/// bisection midpoints. Assumes `f = 1` is infeasible; the caller probes the
/// floor itself when this returns `None`.
fn search_max_feasible<T, E>(
    floor: f64,
    max_probes: usize,
    probe: &mut impl FnMut(f64) -> std::result::Result<(bool, T), E>,
) -> std::result::Result<Option<(f64, T)>, E> {
    let (mut lo, mut hi) = (floor, 1.0_f64);
    let mut best: Option<(f64, T)> = None;
    for _ in 0..max_probes {
        let mid = (lo + hi) / 2.0;
        let (ok, value) = probe(mid)?;
        if ok {
            lo = mid;
            best = Some((mid, value));
        } else {
            hi = mid;
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::value::LengthUnit;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn design_with(f: impl FnOnce(&mut Design)) -> Design {
        let mut design = Design::default();
        f(&mut design);
        design
    }

    fn output(page_count: usize) -> RenderOutput {
        RenderOutput {
            pdf: vec![b'%'],
            page_count,
            warnings: Vec::new(),
            overflow_lines: page_count.saturating_sub(1) * 3,
        }
    }

    /// The floors derived from the design defaults: margins 1.5/1.2 cm,
    /// body 9.5pt with min_body 8.5pt and min_scale 0.85.
    fn default_config(enabled: bool) -> FitConfig {
        FitConfig {
            enabled,
            ..FitConfig::from_design(&Design::default())
        }
    }

    /// Session over a fake compiler that fits exactly when `predicate` says so,
    /// recording every probed `FitParams`.
    fn fake_session(
        config: &FitConfig,
        predicate: impl Fn(&FitParams) -> bool,
        probes: Rc<RefCell<Vec<FitParams>>>,
    ) -> Session<'_, impl FnMut(FitParams) -> Result<RenderOutput, RenderError>> {
        Session::new(config, move |params| {
            probes.borrow_mut().push(params);
            Ok(output(if predicate(&params) { 1 } else { 2 }))
        })
    }

    // ---- floors and parameter mapping (plan 4.2: never below the floor) ----

    #[test]
    fn margin_floor_is_set_by_the_smaller_margin() {
        let margins = Margins {
            x: Length::new(1.5, LengthUnit::Cm),
            y: Length::new(1.2, LengthUnit::Cm),
        };
        assert!((margin_min_scale(&margins) - 1.0 / 1.2).abs() < 1e-9);
    }

    #[test]
    fn margins_already_below_the_floor_are_never_shrunk() {
        let margins = Margins {
            x: Length::new(0.8, LengthUnit::Cm),
            y: Length::new(2.0, LengthUnit::Cm),
        };
        assert_eq!(margin_min_scale(&margins), 1.0);
    }

    #[test]
    fn font_floor_takes_the_stricter_of_size_and_scale_floors() {
        let design = design_with(|d| {
            d.sizes.body = Length::new(9.5, LengthUnit::Pt);
            d.fit_floor.min_body_size = Length::new(8.5, LengthUnit::Pt);
            d.fit_floor.min_scale = 0.85;
        });
        assert!((font_floor_scale(&design) - 8.5 / 9.5).abs() < 1e-9);

        let design = design_with(|d| {
            d.sizes.body = Length::new(9.5, LengthUnit::Pt);
            d.fit_floor.min_scale = 0.95;
        });
        assert!((font_floor_scale(&design) - 0.95).abs() < 1e-9);
    }

    #[test]
    fn nonsensical_floors_degrade_to_never_shrink_fonts() {
        for min_scale in [1.5, -0.5, f64::NAN] {
            let design = design_with(|d| {
                d.sizes.body = Length::new(9.5, LengthUnit::Pt);
                d.fit_floor.min_body_size = Length::new(9.5, LengthUnit::Pt);
                d.fit_floor.min_scale = min_scale;
            });
            assert!(
                font_floor_scale(&design) >= 1.0,
                "min_scale {min_scale} must not enable shrinking below the size floor"
            );
        }
    }

    #[test]
    fn tightness_maps_linearly_to_the_phase_a_floors() {
        let scales = |t: f64| {
            let p = scales_at(0.8, 0.9, t);
            (p.font_scale, p.spacing_scale, p.margin_scale)
        };
        assert_eq!(scales(0.0), (0.9, 1.0, 1.0));
        assert_eq!(scales(1.0), (0.9, 0.6, 0.8));
        assert_eq!(scales(0.5), (0.9, 0.8, 0.9));
    }

    // ---- loop guarantees (plan 4.2) ----

    #[test]
    fn never_shrinks_a_document_that_fits_first_try() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = fake_session(&config, |_| true, probes.clone())
            .run()
            .expect("fake compile never fails");
        assert_eq!(outcome.compiles, 1, "a single identity compile (spec §7)");
        assert_eq!(probes.borrow().len(), 1);
        assert_eq!(probes.borrow()[0], FitParams::default());
        assert!(!outcome.overflowed);
        assert_eq!(outcome.applied, FitParams::default(), "scale 1.00");
    }

    #[test]
    fn a_disabled_loop_compiles_once_without_shrinking() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(false);
        let outcome = fake_session(&config, |_| false, probes.clone())
            .run()
            .expect("compile");
        assert_eq!(outcome.compiles, 1);
        assert_eq!(outcome.applied, FitParams::default());
        assert!(
            !outcome.overflowed,
            "overflow reporting belongs to the loop only"
        );
    }

    #[test]
    fn phase_a_picks_the_least_aggressive_fitting_tightness() {
        // Fits iff spacing_scale ≤ 0.66, i.e. t ≥ 0.85.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = fake_session(&config, |p| p.spacing_scale <= 0.66, probes.clone())
            .run()
            .expect("compile");
        // Bisection grid: .5 F, .75 F, .875 T, .8125 F, .84375 F → best t = .875.
        assert!(
            (outcome.applied.spacing_scale - 0.65).abs() < 1e-9,
            "got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.applied.font_scale, 1.0, "Phase B was not needed");
        assert_eq!(
            outcome.compiles,
            1 + MAX_PHASE_PROBES,
            "identity + 5 probes"
        );
        // The one grid step below the winner was probed and infeasible: nothing
        // less aggressive on the grid fits.
        let spacing: Vec<f64> = probes.borrow().iter().map(|p| p.spacing_scale).collect();
        assert!(
            spacing.iter().any(|s| (s - 0.6625).abs() < 1e-9),
            "the grid point below the winner must have been probed: {spacing:?}"
        );
    }

    #[test]
    fn phase_b_picks_the_largest_fitting_font_scale_at_full_tightness() {
        // Phase A never fits (fonts stay at 1.0); at full tightness it fits iff
        // font_scale ≤ 0.9.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = fake_session(
            &config,
            |p| p.font_scale <= 0.9 && p.spacing_scale <= SPACING_FLOOR,
            probes.clone(),
        )
        .run()
        .expect("compile");
        // Phase B grid on [8.5/9.5, 1): .947 F, .921 F, .908 F, .901 F, .898 T.
        assert!(
            (outcome.applied.font_scale - 0.8980263157894737).abs() < 1e-12,
            "got {:?}",
            outcome.applied
        );
        assert!(
            outcome.applied.font_scale <= 0.9,
            "the returned setting does fit"
        );
        assert!(!outcome.overflowed);
        assert_eq!(
            outcome.compiles,
            1 + (MAX_PHASE_PROBES + 1) + MAX_PHASE_PROBES,
            "identity + A(5+1) + B(5 found) — no floor probe needed"
        );
    }

    #[test]
    fn impossible_content_probes_the_bound_and_flags_overflow() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = fake_session(&config, |_| false, probes.clone())
            .run()
            .expect("compile");
        assert_eq!(
            outcome.compiles,
            1 + (MAX_PHASE_PROBES + 1) * 2,
            "the documented worst case is exactly this count (spec §7 hard bound)"
        );
        assert!(outcome.overflowed);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(outcome.applied.font_scale, 8.5 / 9.5));
        assert!(near(outcome.applied.spacing_scale, SPACING_FLOOR));
        assert!(
            near(outcome.applied.margin_scale, config.margin_min_scale),
            "overflow reports the exact floors, not grid approximations"
        );
    }

    #[test]
    fn every_probe_stays_above_every_floor() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let _ = fake_session(&config, |_| false, probes.clone())
            .run()
            .expect("compile");
        let font_floor = 8.5 / 9.5;
        for p in probes.borrow().iter() {
            assert!(
                (font_floor - 1e-9..=1.0 + 1e-9).contains(&p.font_scale)
                    && (SPACING_FLOOR - 1e-9..=1.0 + 1e-9).contains(&p.spacing_scale)
                    && (config.margin_min_scale - 1e-9..=1.0 + 1e-9).contains(&p.margin_scale),
                "probe {p:?} crossed a floor"
            );
        }
    }

    #[test]
    fn font_phase_disabled_at_floor_one_skips_phase_b() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            enabled: true,
            margin_min_scale: 0.8,
            font_floor: 1.0,
        };
        let outcome = fake_session(&config, |_| false, probes.clone())
            .run()
            .expect("compile");
        assert!(outcome.overflowed);
        assert_eq!(
            outcome.compiles,
            1 + MAX_PHASE_PROBES + 1,
            "no font probes when the floor equals the original size"
        );
        assert!(
            probes.borrow().iter().all(|p| p.font_scale == 1.0),
            "fonts are never probed below 1.0"
        );
        assert_eq!(outcome.applied.font_scale, 1.0);
    }

    #[test]
    fn the_same_input_produces_the_same_probe_sequence_and_result() {
        let run_once = || {
            let probes = Rc::new(RefCell::new(Vec::new()));
            let config = default_config(true);
            let outcome = fake_session(&config, |p| p.spacing_scale <= 0.7, probes.clone())
                .run()
                .expect("compile");
            (outcome.applied, outcome.compiles, probes.borrow().clone())
        };
        let first = run_once();
        let second = run_once();
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "determinism (spec §7 hard requirement)"
        );
    }

    #[test]
    fn a_compile_error_stops_the_search_immediately_and_propagates() {
        let config = default_config(true);
        let mut session = Session::new(&config, |params| {
            if params.spacing_scale >= 1.0 {
                Ok(output(2)) // identity does not fit
            } else {
                Err(RenderError::Template {
                    message: "boom".into(),
                })
            }
        });
        let error = session
            .run()
            .expect_err("the failing probe must abort the loop");
        assert!(error.to_string().contains("boom"));
        assert_eq!(
            session.compiles, 2,
            "identity, then the first A probe fails"
        );
    }
}
