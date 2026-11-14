//! Fitting loop to the target page count (spec §7, plan 4.1–4.2; search shape in
//! ADR-0024).
//!
//! Strategy, in spec order:
//!
//! 1. Compile as-is. If it fits (within `design.pages` pages), **never shrink** — the
//!    identity compile doubles as the "t = 0 is infeasible" knowledge for the search.
//! 2. Phase A — tighten `spacing.*` and margins together along one linear "tightness"
//!    `t`, floors: spacing scale 0.6, margins never below 1.0 cm.
//! 3. Phase B — only if A's floors still overflow: shrink `font_scale` toward its
//!    floor (`fit_floor.min_scale` and `min_body_size / sizes.body`, whichever is
//!    stricter, i.e. the higher lower bound) at `t = 1`.
//! 4. Phase F (fill, ADR-0044) — only when the identity compile already fits
//!    and the target is more than one page: grow the scales that make the
//!    pages fuller, one bounded search at a time, each entered only when the
//!    previous one ended at its cap with the document still fitting —
//!    `spacing_scale` (the vertical rhythm stretches), then `font_scale`
//!    (text and leading grow), then `margin_scale` (the text area shrinks so
//!    the same content fills it deeper, typography untouched). The caps
//!    ([`FILL_MAX_SCALE`], [`FILL_MAX_FONT_SCALE`], [`FILL_MAX_MARGIN_SCALE`])
//!    are sanity limits, not fill guarantees: content far below the target
//!    cannot fill it gracefully at any setting.
//! 5. The searches are bounded bisections probing deterministic midpoints
//!    (`MAX_PHASE_PROBES` each, plus one explicit boundary probe), so they
//!    terminate by construction, never cross a floor, and return the least
//!    aggressive *grid point* that fits (monotonicity of "shrinks more ⇒ fits"
//!    is assumed; where rounding breaks it locally the result still fits, see
//!    ADR-0024).
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
/// example step). Worst-case compile count: `1 + 2 * (5 + 1) = 13` on the
/// tighten path, `1 + 3 * (5 + 1) = 19` on the fill path (they are mutually
/// exclusive).
pub const MAX_PHASE_PROBES: usize = 5;

/// Fill-phase caps (ADR-0044): growth stops here even if the pages are not
/// full yet — stretched rhythm, oversized type, and shrunken text columns all
/// read badly past these limits, and content far below the target cannot fill
/// it gracefully at any setting.
pub const FILL_MAX_SCALE: f64 = 3.0;
pub const FILL_MAX_FONT_SCALE: f64 = 1.25;
pub const FILL_MAX_MARGIN_SCALE: f64 = 1.4;

/// Everything the loop needs from the (resolved) design block, precomputed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitConfig {
    /// Run the loop at all (`design.fit_one_page`, overridable by `--no-fit`).
    pub enabled: bool,
    /// Page count the document must fit within (`design.pages`).
    pub target_pages: usize,
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
            target_pages: design.pages.get() as usize,
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
    /// Compiles performed — bounded by `1 + 2 * (MAX_PHASE_PROBES + 1)` on the
    /// tighten path and `1 + MAX_PHASE_PROBES + 1` on the fill path; the two
    /// are mutually exclusive.
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
    let target_pages = config.target_pages;
    Session::new(config, |params| engine.compile(params, target_pages)).run()
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

struct Session<'a> {
    config: &'a FitConfig,
    compile: Box<dyn FnMut(FitParams) -> std::result::Result<RenderOutput, RenderError> + 'a>,
    compiles: usize,
}

impl<'a> Session<'a> {
    fn new(
        config: &'a FitConfig,
        compile: impl FnMut(FitParams) -> std::result::Result<RenderOutput, RenderError> + 'a,
    ) -> Self {
        Self {
            config,
            compile: Box::new(compile),
            compiles: 0,
        }
    }

    /// Does this compile stay within the target page count?
    fn fits(&self, output: &RenderOutput) -> bool {
        // A paged document always has ≥ 1 page; the comparison only also guards
        // the impossible 0 against a target of 1.
        output.page_count <= self.config.target_pages
    }

    /// One compile at the mapped scales, counted for telemetry.
    fn call(&mut self, font_scale: f64, t: f64) -> std::result::Result<RenderOutput, RenderError> {
        self.compiles += 1;
        (self.compile)(scales_at(self.config.margin_min_scale, font_scale, t))
    }

    /// One compile at explicit scales, counted for telemetry (the fill phase
    /// probes spacing scales above 1.0, which [`Self::call`]'s `t` mapping
    /// cannot express).
    fn call_params(&mut self, params: FitParams) -> std::result::Result<RenderOutput, RenderError> {
        self.compiles += 1;
        (self.compile)(params)
    }

    /// Compile split into `(fits, output)` for the searches.
    fn probe(
        &mut self,
        font_scale: f64,
        t: f64,
    ) -> std::result::Result<(bool, RenderOutput), RenderError> {
        let output = self.call(font_scale, t)?;
        Ok((self.fits(&output), output))
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
        if !self.config.enabled {
            return Ok(self.outcome(identity, FitParams::default(), false));
        }
        if self.fits(&identity) {
            // Hard requirement (spec §7): never shrink what already fits. A
            // multi-page target instead gets the fill phase (ADR-0044): grow
            // spacing so the pages do not underfill.
            if self.config.target_pages > 1 {
                return self.fill(identity);
            }
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
            search_max_feasible(floor, 1.0, MAX_PHASE_PROBES, &mut |f| self.probe(f, 1.0))?
        {
            return Ok(self.outcome(output, scales_at(margin_min, f, 1.0), false));
        }

        let (fits_floor, at_floor) = self.probe(floor, 1.0)?;
        Ok(self.outcome(at_floor, scales_at(margin_min, floor, 1.0), !fits_floor))
    }
    /// Phase F (ADR-0044): the content fits within the target page count but
    /// underfills it. Grow spacing, then fonts, then margins — each to the
    /// largest scale whose compile still fits, each stage entered only when the
    /// previous one ended at its cap (i.e. the pages can still take more). A
    /// one-page CV never fills: its layout stays the top-flow identity.
    fn fill(&mut self, identity: RenderOutput) -> std::result::Result<FitOutcome, RenderError> {
        let (spacing_params, spacing_output, spacing_capped) =
            self.grow_to_cap(FILL_MAX_SCALE, FitParams::default(), identity, &|p, s| {
                FitParams {
                    spacing_scale: s,
                    ..p
                }
            })?;
        if !spacing_capped {
            return Ok(self.outcome(spacing_output, spacing_params, false));
        }

        let (font_params, font_output, font_capped) = self.grow_to_cap(
            FILL_MAX_FONT_SCALE,
            spacing_params,
            spacing_output,
            &|p, f| FitParams { font_scale: f, ..p },
        )?;
        if !font_capped {
            return Ok(self.outcome(font_output, font_params, false));
        }

        let (margin_params, margin_output, _) =
            self.grow_to_cap(FILL_MAX_MARGIN_SCALE, font_params, font_output, &|p, m| {
                FitParams {
                    margin_scale: m,
                    ..p
                }
            })?;
        Ok(self.outcome(margin_output, margin_params, false))
    }

    /// One fill-stage search: grow the scale `set` writes from 1.0 toward `cap`,
    /// keeping the other params at `base`, to the largest value whose compile
    /// still fits. Returns the adopted params, the output they produced, and
    /// whether the cap itself fit — the caller's signal that slack remains for
    /// the next stage. When no probed point fits, `base` stays (its output is
    /// known-feasible: it either is the identity or just fit the previous cap).
    fn grow_to_cap(
        &mut self,
        cap: f64,
        base: FitParams,
        base_output: RenderOutput,
        set: &dyn Fn(FitParams, f64) -> FitParams,
    ) -> std::result::Result<(FitParams, RenderOutput, bool), RenderError> {
        let mut probe = |scale: f64| {
            let output = self.call_params(set(base, scale))?;
            Ok((self.fits(&output), output))
        };
        let grid_best = search_max_feasible(1.0, cap, MAX_PHASE_PROBES, &mut probe)?;
        // The cap is the most generous setting the stage has: adopt it whenever
        // it still fits, whether or not a grid point below it did.
        let (fits_cap, at_cap) = probe(cap)?;
        if fits_cap {
            return Ok((set(base, cap), at_cap, true));
        }
        if let Some((scale, output)) = grid_best {
            return Ok((set(base, scale), output, false));
        }
        Ok((base, base_output, false))
    }
}

/// Smallest `t` in `(0, 1]` that the probe accepts, probed at deterministic
/// bisection midpoints. Assumes `t = 0` is infeasible. Returns `None` when no
/// probed point fits.
fn search_min_feasible(
    max_probes: usize,
    probe: &mut dyn FnMut(f64) -> std::result::Result<(bool, RenderOutput), RenderError>,
) -> std::result::Result<Option<(f64, RenderOutput)>, RenderError> {
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let mut best: Option<(f64, RenderOutput)> = None;
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

/// Largest point in `(lo, hi)` that the probe accepts, at deterministic
/// bisection midpoints. The caller owns the boundary semantics: when this
/// returns `None` it probes the edge the grid could not reach (the font floor
/// in Phase B, the fill cap in Phase F).
fn search_max_feasible(
    lo: f64,
    hi: f64,
    max_probes: usize,
    probe: &mut dyn FnMut(f64) -> std::result::Result<(bool, RenderOutput), RenderError>,
) -> std::result::Result<Option<(f64, RenderOutput)>, RenderError> {
    let (mut lo, mut hi) = (lo, hi);
    let mut best: Option<(f64, RenderOutput)> = None;
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
    fn fake_session<'a>(
        config: &'a FitConfig,
        predicate: impl Fn(&FitParams) -> bool + 'a,
        probes: Rc<RefCell<Vec<FitParams>>>,
    ) -> Session<'a> {
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
    fn a_zero_body_size_floors_the_font_phase_at_the_original_scale() {
        // `sizes.body: 0pt` parses (the model allows any finite length); the
        // ratio would divide by zero, so the phase must be disabled instead.
        let design = design_with(|d| {
            d.sizes.body = Length::new(0.0, LengthUnit::Pt);
        });
        assert_eq!(font_floor_scale(&design), 1.0);
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
    fn a_two_page_target_fills_rather_than_shrinks_a_fitting_document() {
        // The fake always compiles to two pages: the identity fits, so the fill
        // phase runs every stage to its cap instead of ever shrinking.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = session_growing_with_spacing(&config, |_| 2, probes.clone())
            .run()
            .expect("fake compile never fails");
        assert_eq!(
            outcome.applied,
            FitParams {
                font_scale: FILL_MAX_FONT_SCALE,
                spacing_scale: FILL_MAX_SCALE,
                margin_scale: FILL_MAX_MARGIN_SCALE,
            },
            "all three fill caps are adopted"
        );
        assert!(!outcome.overflowed);
        assert_eq!(outcome.compiles, 1 + 3 * (MAX_PHASE_PROBES + 1));
    }

    #[test]
    fn a_two_page_target_still_tightens_content_beyond_two_pages() {
        // Fits (2 pages) only once spacing is tightened enough; otherwise 3 pages.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = Session::new(&config, |params| {
            Ok(output(if params.spacing_scale <= 0.7 { 2 } else { 3 }))
        })
        .run()
        .expect("fake compile never fails");
        assert!(
            outcome.applied.spacing_scale <= 0.7,
            "the returned setting fits within 2 pages: {:?}",
            outcome.applied
        );
        assert_eq!(outcome.compiles, 1 + MAX_PHASE_PROBES);
        assert!(!outcome.overflowed);
    }

    // ---- fill phase (ADR-0044): a fitting multi-page document fills its pages ----

    /// A compiler whose page count only grows with the spacing scale — the shape
    /// of a sparse document the fill phase stretches into the target.
    fn session_growing_with_spacing<'a>(
        config: &'a FitConfig,
        pages_at: impl Fn(f64) -> usize + 'a,
        probes: Rc<RefCell<Vec<FitParams>>>,
    ) -> Session<'a> {
        Session::new(config, move |params| {
            probes.borrow_mut().push(params);
            Ok(output(pages_at(params.spacing_scale)))
        })
    }

    #[test]
    fn fill_grows_spacing_to_the_largest_fitting_scale() {
        // Content that is two pages at scale 1.0, three pages past spacing 2.5:
        // the largest fitting grid point is 2.5, and the cap (3.0) does not fit.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome =
            session_growing_with_spacing(&config, |s| if s <= 2.5 { 2 } else { 3 }, probes.clone())
                .run()
                .expect("fake compile never fails");
        assert!(
            (outcome.applied.spacing_scale - 2.5).abs() < 1e-9,
            "got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.applied.font_scale, 1.0, "fonts never grow");
        assert_eq!(outcome.applied.margin_scale, 1.0, "margins never grow");
        assert!(!outcome.overflowed);
        assert_eq!(
            outcome.compiles,
            1 + MAX_PHASE_PROBES + 1,
            "identity + fill grid + the cap probe"
        );
        assert!(
            probes
                .borrow()
                .iter()
                .all(|p| p.font_scale == 1.0 && p.margin_scale == 1.0),
            "only spacing moves: {:?}",
            probes.borrow()
        );
    }

    #[test]
    fn fill_adopts_the_cap_when_it_still_fits() {
        // Content so sparse that even the spacing cap fits: the cap is adopted,
        // and the still-slack pages pull in the font and margin stages too.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = session_growing_with_spacing(
            &config,
            |s| if s <= FILL_MAX_SCALE { 1 } else { 3 },
            probes.clone(),
        )
        .run()
        .expect("fake compile never fails");
        assert_eq!(
            outcome.applied,
            FitParams {
                font_scale: FILL_MAX_FONT_SCALE,
                spacing_scale: FILL_MAX_SCALE,
                margin_scale: FILL_MAX_MARGIN_SCALE,
            },
            "got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.compiles, 1 + 3 * (MAX_PHASE_PROBES + 1));
    }

    #[test]
    fn the_font_stage_grows_only_until_the_target_is_reached() {
        // The spacing cap fits, but font growth past ~1.1 (with spacing at the
        // cap) spills to a third page: the font stage stops at its boundary and
        // the margin stage never runs.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = Session::new(&config, |p| {
            let fits =
                p.spacing_scale <= FILL_MAX_SCALE && p.font_scale <= 1.1 && p.margin_scale <= 1.0;
            Ok(output(if fits { 2 } else { 3 }))
        })
        .run()
        .expect("fake compile never fails");
        assert!(
            outcome.applied.font_scale > 1.0 && outcome.applied.font_scale < FILL_MAX_FONT_SCALE,
            "font growth stops below its cap, got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.applied.spacing_scale, FILL_MAX_SCALE);
        assert_eq!(outcome.applied.margin_scale, 1.0, "margins never move");
        assert_eq!(outcome.compiles, 1 + 2 * (MAX_PHASE_PROBES + 1));
    }

    #[test]
    fn the_margin_stage_fills_what_the_font_stage_left() {
        // Everything fits until margin growth past ~1.2: the margin stage is
        // the last one entered and stops at its own boundary.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = Session::new(&config, |p| {
            let fits = p.spacing_scale <= FILL_MAX_SCALE
                && p.font_scale <= FILL_MAX_FONT_SCALE
                && p.margin_scale <= 1.2;
            Ok(output(if fits { 2 } else { 3 }))
        })
        .run()
        .expect("fake compile never fails");
        assert!(
            outcome.applied.margin_scale > 1.0
                && outcome.applied.margin_scale < FILL_MAX_MARGIN_SCALE,
            "margin growth stops below its cap, got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.applied.spacing_scale, FILL_MAX_SCALE);
        assert_eq!(outcome.applied.font_scale, FILL_MAX_FONT_SCALE);
        assert_eq!(outcome.compiles, 1 + 3 * (MAX_PHASE_PROBES + 1));
    }

    #[test]
    fn fill_keeps_the_identity_when_no_growth_still_fits() {
        // Content that fills the target exactly at the identity: any growth
        // overflows, so the identity compile is the fill result.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome =
            session_growing_with_spacing(&config, |s| if s <= 1.0 { 2 } else { 3 }, probes.clone())
                .run()
                .expect("fake compile never fails");
        assert_eq!(outcome.applied, FitParams::default(), "no growth fits");
        assert!(!outcome.overflowed);
        assert_eq!(outcome.compiles, 1 + MAX_PHASE_PROBES + 1);
    }

    #[test]
    fn a_disabled_loop_never_fills_even_for_a_multi_page_target() {
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            enabled: false,
            target_pages: 2,
            ..default_config(false)
        };
        let outcome = session_growing_with_spacing(&config, |_| 2, probes.clone())
            .run()
            .expect("fake compile never fails");
        assert_eq!(outcome.compiles, 1, "identity only");
        assert_eq!(outcome.applied, FitParams::default());
        assert!(!outcome.overflowed);
    }

    #[test]
    fn the_fill_phase_is_deterministic() {
        let run_once = || {
            let probes = Rc::new(RefCell::new(Vec::new()));
            let config = FitConfig {
                target_pages: 2,
                ..default_config(true)
            };
            let outcome = session_growing_with_spacing(
                &config,
                |s| if s <= 2.25 { 2 } else { 3 },
                probes.clone(),
            )
            .run()
            .expect("fake compile never fails");
            (outcome.applied, outcome.compiles, probes.borrow().clone())
        };
        assert_eq!(
            format!("{:?}", run_once()),
            format!("{:?}", run_once()),
            "determinism (spec §7 hard requirement)"
        );
    }

    #[test]
    fn a_compile_error_inside_the_fill_search_propagates() {
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let mut session = Session::new(&config, |params| {
            if params.spacing_scale > 1.0 {
                Err(RenderError::Template {
                    message: format!("boom at spacing {}", params.spacing_scale),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session
            .run()
            .expect_err("the first fill probe failure must escape");
        assert!(error.to_string().contains("spacing 2"), "got: {error}");
        assert_eq!(session.compiles, 2, "identity, then the first fill probe");
    }

    #[test]
    fn a_compile_error_in_a_later_fill_stage_propagates() {
        // Spacing grows fine; the font stage is where the template breaks.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let mut session = Session::new(&config, |params| {
            if params.font_scale > 1.0 {
                Err(RenderError::Template {
                    message: format!("boom at font {}", params.font_scale),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session
            .run()
            .expect_err("the font-stage failure must escape");
        assert!(error.to_string().contains("font 1.125"), "got: {error}");
    }

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
            target_pages: 1,
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
    fn a_non_monotonic_predicate_still_returns_a_fitting_result_within_budget() {
        // Adversarial layout: only `spacing_scale ∈ (0.74, 0.86)` fits —
        // tightening past the window breaks the page again (the ADR-0024
        // monotonicity caveat made visible). No grid probe lands on a window
        // boundary, so the fake is float-stable. Guarantee under test: the
        // returned setting fits, whatever the predicate shape, within budget.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let in_window = |s: f64| s > 0.74 && s < 0.86;
        let outcome = fake_session(&config, |p| in_window(p.spacing_scale), probes.clone())
            .run()
            .expect("compile");
        assert!(
            in_window(outcome.applied.spacing_scale),
            "the returned setting must fit: {:?}",
            outcome.applied
        );
        assert!(!outcome.overflowed);
        assert_eq!(outcome.compiles, 1 + MAX_PHASE_PROBES);
    }

    #[test]
    fn phase_a_can_land_exactly_on_its_floor_probe() {
        // Only `t = 1` fits (spacing at the exact floor): the grid misses it, so
        // the explicit floor probe is what returns the fitting result.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = fake_session(
            &config,
            |p| p.spacing_scale <= SPACING_FLOOR,
            probes.clone(),
        )
        .run()
        .expect("compile");
        assert!(
            (outcome.applied.spacing_scale - SPACING_FLOOR).abs() < 1e-9,
            "the floor probe result must be adopted: {:?}",
            outcome.applied
        );
        assert_eq!(outcome.applied.font_scale, 1.0, "Phase B was not needed");
        assert_eq!(outcome.compiles, 1 + MAX_PHASE_PROBES + 1);
        assert!(!outcome.overflowed);
    }

    /// A compile that breaks at a chosen setting: the loop must surface that
    /// compiler's error from whichever stage probed it (the searches each return
    /// through a `?`, so a late failure must not be swallowed or retried).
    fn session_failing_when<'a>(
        config: &'a FitConfig,
        fails: impl Fn(&FitParams) -> bool + 'a,
    ) -> Session<'a> {
        Session::new(config, move |params| {
            if fails(&params) {
                Err(RenderError::Template {
                    message: format!(
                        "boom at font {} spacing {}",
                        params.font_scale, params.spacing_scale
                    ),
                })
            } else {
                Ok(output(2)) // every other setting overflows, but compiles
            }
        })
    }

    fn never_shrinkable() -> FitConfig {
        FitConfig {
            enabled: true,
            target_pages: 1,
            margin_min_scale: 0.8,
            font_floor: 0.5,
        }
    }

    #[test]
    fn a_compile_error_at_the_phase_a_floor_probe_propagates() {
        let config = default_config(true);
        let mut session = session_failing_when(&config, |p| {
            (p.spacing_scale - SPACING_FLOOR).abs() < 1e-12 && p.font_scale == 1.0
        });
        let error = session
            .run()
            .expect_err("the floor probe failure must escape");
        assert!(error.to_string().contains("spacing 0.6"), "got: {error}");
        assert_eq!(
            session.compiles,
            1 + MAX_PHASE_PROBES + 1,
            "identity + the A grid + the floor probe that broke"
        );
    }

    #[test]
    fn a_compile_error_inside_the_phase_b_search_propagates() {
        let config = never_shrinkable();
        let mut session = session_failing_when(&config, |p| {
            (p.spacing_scale - SPACING_FLOOR).abs() < 1e-12 && p.font_scale < 1.0
        });
        let error = session
            .run()
            .expect_err("the first Phase B probe failure must escape");
        assert!(error.to_string().contains("font 0.75"), "got: {error}");
        assert_eq!(
            session.compiles,
            1 + (MAX_PHASE_PROBES + 1) + 1,
            "identity + A(5+1) + the B probe that broke"
        );
    }

    #[test]
    fn a_compile_error_at_the_font_floor_probe_propagates() {
        let config = never_shrinkable();
        let mut session = session_failing_when(&config, |p| (p.font_scale - 0.5).abs() < 1e-12);
        let error = session
            .run()
            .expect_err("the font floor probe failure must escape");
        assert!(error.to_string().contains("font 0.5"), "got: {error}");
        assert_eq!(
            session.compiles,
            1 + (MAX_PHASE_PROBES + 1) * 2,
            "the whole budget, the last compile breaking"
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

    /// A template that breaks only at one setting still aborts the loop with that
    /// compiler's error wherever the search hits it: the Phase A floor probe, a Phase
    /// B midpoint, and the final font-floor probe (the three `?` sites left after the
    /// search itself).
    #[test]
    fn a_compile_error_at_the_phase_a_floor_probe_aborts_the_loop() {
        let config = tight_config();
        let mut session = Session::new(&config, |params| {
            if params.font_scale == 1.0 && params.spacing_scale == SPACING_FLOOR {
                Err(RenderError::Template {
                    message: "broken at the spacing floor".into(),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session.run().expect_err("the floor probe must abort");
        assert!(
            error.to_string().contains("broken at the spacing floor"),
            "got: {error}"
        );
        assert_eq!(session.compiles, 1 + MAX_PHASE_PROBES + 1);
    }

    #[test]
    fn a_compile_error_during_the_phase_b_search_aborts_the_loop() {
        let config = tight_config();
        let mut session = Session::new(&config, |params| {
            if params.spacing_scale == SPACING_FLOOR && (params.font_scale - 0.75).abs() < 1e-9 {
                Err(RenderError::Template {
                    message: "broken at font scale 0.75".into(),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session
            .run()
            .expect_err("the first Phase B probe must abort");
        assert!(
            error.to_string().contains("broken at font scale 0.75"),
            "got: {error}"
        );
        // identity + A(5+1) + the first B midpoint.
        assert_eq!(session.compiles, 1 + MAX_PHASE_PROBES + 1 + 1);
    }

    #[test]
    fn a_compile_error_at_the_font_floor_probe_aborts_the_loop() {
        let config = tight_config();
        let mut session = Session::new(&config, |params| {
            if params.font_scale == config.font_floor {
                Err(RenderError::Template {
                    message: "broken at the font floor".into(),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session.run().expect_err("the font floor probe must abort");
        assert!(
            error.to_string().contains("broken at the font floor"),
            "got: {error}"
        );
        assert_eq!(
            session.compiles,
            1 + (MAX_PHASE_PROBES + 1) + MAX_PHASE_PROBES + 1,
            "the worst case, cut short at the last probe"
        );
    }

    /// A config with both phase floors below 1.0, so every stage of the search runs.
    fn tight_config() -> FitConfig {
        FitConfig {
            enabled: true,
            target_pages: 1,
            margin_min_scale: 0.8,
            font_floor: 0.5,
        }
    }
}
