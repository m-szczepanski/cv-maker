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
//! 4. Phase F (fill, ADR-0044) — entered whenever a compile fits within a
//!    multi-page target (the identity or a tightened landing): the loop grows
//!    the rhythm while the document stays within the count, and ADR-0052
//!    extends that phase to reach the count exactly. Growth happens one
//!    bounded search at a time, each entered only when the previous one
//!    ended at its cap with the document still fitting — `spacing_scale`
//!    (the vertical rhythm stretches), then `font_scale` (text and leading
//!    grow), then `margin_scale` (the text area shrinks so the same content
//!    fills it deeper, typography untouched) — each to the largest value
//!    that still fits, up to the ordinary caps ([`FILL_MAX_SCALE`],
//!    [`FILL_MAX_FONT_SCALE`], [`FILL_MAX_MARGIN_SCALE`]).
//! 5. Exact-count stretch (ADR-0052) — when the capped growth still leaves
//!    fewer pages than the target, one final bounded search grows
//!    `spacing_scale` past the caps to the gentlest stretch that reaches the
//!    count, under a ceiling set by `design.fit_stretch_max` (default
//!    [`FILL_EXTENDED_MAX_SPACING`], never below the ordinary caps). A
//!    stretch that jumps over the target (kept-whole sections move several
//!    pages at once) or a ceiling that cannot reach it is reported
//!    `underfilled` — never a silently wrong page count.
//! 6. If the floors still overflow the target (spec §7 step 4, ADR-0052),
//!    the loop does not render the floors: it adopts the **least aggressive
//!    compile it already probed** among those reaching the **smallest page
//!    count it saw** (the identity itself if nothing moved the page count).
//!    Compression that buys no page is pure damage, and the choice costs no
//!    extra compiles: every candidate is in the probe history. The run is
//!    still reported as overflowed (the target was not met) with the warning
//!    naming the page count it landed on.
//! 7. The searches are bounded bisections probing deterministic midpoints
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
/// example step). Worst-case compile count (spec §7): 13 on a run that only
/// tightens (the overflow fallback adds none — it picks from the probe
/// history), 25 on a run that only fills (19 plus the ceiling probe and the
/// stretch bisection), and 36 when a tightened landing under the target
/// chains the fill and stretch searches after it.
pub const MAX_PHASE_PROBES: usize = 5;

/// Fill-phase caps (ADR-0044): ordinary growth stops here even if the pages
/// are not full yet — stretched rhythm, oversized type, and shrunken text
/// columns all read badly past these limits. They are preferences, not
/// ceilings: when the exact target page count is still not reached, the
/// spacing scale grows further (see [`FILL_EXTENDED_MAX_SPACING`], ADR-0052).
pub const FILL_MAX_SCALE: f64 = 3.0;
pub const FILL_MAX_FONT_SCALE: f64 = 1.25;
pub const FILL_MAX_MARGIN_SCALE: f64 = 1.4;

/// Hard ceiling of the exact-target stretch (ADR-0052): twelve times the
/// designed rhythm, and the default of `design.fit_stretch_max`. Content
/// sparser than that cannot span the target page count at any acceptable
/// setting, and the run reports `underfilled`.
pub const FILL_EXTENDED_MAX_SPACING: f64 = 12.0;

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
    /// Largest `spacing_scale` the exact-target stretch may use (ADR-0052),
    /// clamped into `[FILL_MAX_SCALE, FILL_EXTENDED_MAX_SPACING]`.
    pub max_stretch_spacing: f64,
}

impl FitConfig {
    /// Derive the loop parameters from a fully resolved design block.
    pub fn from_design(design: &Design) -> Self {
        Self {
            enabled: design.fit_one_page,
            target_pages: design.pages.get() as usize,
            margin_min_scale: margin_min_scale(&design.margins),
            font_floor: font_floor_scale(design),
            max_stretch_spacing: stretch_ceiling(design),
        }
    }
}

/// The chosen scales, the PDF they produced, and loop telemetry.
#[derive(Debug)]
pub struct FitOutcome {
    pub output: RenderOutput,
    pub applied: FitParams,
    /// Compiles performed — bounded by `1 + 2 * (MAX_PHASE_PROBES + 1) = 13`
    /// when only tightening runs, `1 + 3 * (MAX_PHASE_PROBES + 1) +
    /// (MAX_PHASE_PROBES + 1) = 25` when only the fill and its stretch run,
    /// and 36 when a tightened landing under the target chains them
    /// (spec §7 hard bound).
    pub compiles: usize,
    /// Content still overflowed at every floor (spec §7 step 4; exit stays 0,
    /// ADR-0026). The delivered PDF has MORE pages than the target.
    pub overflowed: bool,
    /// The target page count could not be reached from below either: the
    /// content stayed under it even stretched to [`FILL_EXTENDED_MAX_SPACING`],
    /// or the stretch jumped over it (whole sections moving pages at once).
    /// The delivered PDF has FEWER pages than the target (ADR-0052).
    pub underfilled: bool,
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

/// Stretch ceiling (ADR-0052): `design.fit_stretch_max` clamped into
/// `[FILL_MAX_SCALE, FILL_EXTENDED_MAX_SPACING]` — it can lower the search
/// range below the default but never disable the ordinary fill stages or
/// raise the sanity ceiling; a non-finite value degrades to the default,
/// mirroring how [`font_floor_scale`] contains a nonsense `min_scale`.
fn stretch_ceiling(design: &Design) -> f64 {
    let requested = design.fit_stretch_max;
    if requested.is_finite() {
        requested.clamp(FILL_MAX_SCALE, FILL_EXTENDED_MAX_SPACING)
    } else {
        FILL_EXTENDED_MAX_SPACING
    }
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

/// Ordering for the overflow fallback (ADR-0052): bigger fonts, then a wider
/// rhythm, then wider margins is gentler. Scale triples are finite by
/// construction (fixed floors, deterministic bisection midpoints), so
/// `total_cmp` compares them for a strict weak order.
fn gentler_than(a: &FitParams, b: &FitParams) -> bool {
    a.font_scale
        .total_cmp(&b.font_scale)
        .then(a.spacing_scale.total_cmp(&b.spacing_scale))
        .then(a.margin_scale.total_cmp(&b.margin_scale))
        .is_gt()
}

struct Session<'a> {
    config: &'a FitConfig,
    compile: Box<dyn FnMut(FitParams) -> std::result::Result<RenderOutput, RenderError> + 'a>,
    compiles: usize,
    /// Every compile of the tighten path, in probe order — the overflow
    /// fallback (ADR-0052) picks the least aggressive among them without
    /// re-probing, so the spec §7 compile bound holds unchanged. At the
    /// worst-case 36 probes with full PDF bytes this holds a few MB, which
    /// the build process lifetime makes acceptable.
    history: Vec<(FitParams, RenderOutput)>,
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
            history: Vec::new(),
        }
    }

    /// Does this compile stay within the target page count?
    fn fits(&self, output: &RenderOutput) -> bool {
        // A paged document always has ≥ 1 page; the comparison only also guards
        // the impossible 0 against a target of 1.
        output.page_count <= self.config.target_pages
    }

    /// One compile at the mapped scales, counted and recorded for telemetry.
    fn call(&mut self, font_scale: f64, t: f64) -> std::result::Result<RenderOutput, RenderError> {
        let params = scales_at(self.config.margin_min_scale, font_scale, t);
        self.compiles += 1;
        let output = (self.compile)(params)?;
        self.history.push((params, output.clone()));
        Ok(output)
    }

    /// One compile at explicit scales, counted and recorded for telemetry (the fill
    /// phase probes spacing scales above 1.0, which [`Self::call`]'s `t` mapping
    /// cannot express).
    fn call_params(&mut self, params: FitParams) -> std::result::Result<RenderOutput, RenderError> {
        self.compiles += 1;
        let output = (self.compile)(params)?;
        self.history.push((params, output.clone()));
        Ok(output)
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
            underfilled: false,
        }
    }

    /// The exact-target rule (ADR-0052): a compile that fits within the page
    /// count but lands under it grows through the fill phase instead.
    fn settle(
        &mut self,
        params: FitParams,
        output: RenderOutput,
    ) -> Result<FitOutcome, RenderError> {
        if output.page_count < self.config.target_pages {
            return self.fill((params, output));
        }
        Ok(self.outcome(output, params, false))
    }

    fn run(&mut self) -> std::result::Result<FitOutcome, RenderError> {
        let identity = self.call(1.0, 0.0)?;
        if !self.config.enabled {
            return Ok(self.outcome(identity, FitParams::default(), false));
        }
        if self.fits(&identity) {
            // Hard requirement (spec §7): never shrink what already fits. A
            // multi-page target gets the fill phase even at the right page
            // count (ADR-0044: grow the rhythm while it stays within target);
            // ADR-0052 extends that phase to reach the count exactly.
            if self.config.target_pages > 1 {
                return self.fill((FitParams::default(), identity));
            }
            return Ok(self.outcome(identity, FitParams::default(), false));
        }

        let margin_min = self.config.margin_min_scale;

        // Phase A: least aggressive tightness that fits, fonts at full size.
        if let Some((t, output)) =
            search_min_feasible(MAX_PHASE_PROBES, &mut |t| self.probe(1.0, t))?
        {
            return self.settle(scales_at(margin_min, 1.0, t), output);
        }

        let (floors_fit, at_floors) = self.probe(1.0, 1.0)?;
        if floors_fit {
            return self.settle(scales_at(margin_min, 1.0, 1.0), at_floors);
        }

        // Phase B: largest font scale that fits at full tightness.
        let floor = self.config.font_floor;
        if floor >= 1.0 {
            // Font shrinking is floored at the original size: the Phase A floor
            // probe is the most aggressive compile that exists.
            return Ok(self.overflow_fallback(scales_at(margin_min, 1.0, 1.0), at_floors));
        }
        if let Some((f, output)) =
            search_max_feasible(floor, 1.0, MAX_PHASE_PROBES, &mut |f| self.probe(f, 1.0))?
        {
            return self.settle(scales_at(margin_min, f, 1.0), output);
        }

        let (fits_floor, at_floor) = self.probe(floor, 1.0)?;
        if fits_floor {
            return self.settle(scales_at(margin_min, floor, 1.0), at_floor);
        }
        Ok(self.overflow_fallback(scales_at(margin_min, floor, 1.0), at_floor))
    }

    /// Spec §7 step 4 with ADR-0052: the target page count is unreachable, so
    /// among the compiles already probed, adopt the least aggressive one that
    /// reaches the smallest page count seen — compressing past that buys no
    /// page and only damages typography. Seeded with the floor compile (also
    /// in the history), the choice needs no probes and no unreachable
    /// branches: a candidate always exists.
    fn overflow_fallback(&self, floor_params: FitParams, floor_output: RenderOutput) -> FitOutcome {
        let mut best = (floor_params, floor_output);
        let mut min_pages = best.1.page_count;
        for (params, output) in &self.history {
            if output.page_count < min_pages {
                min_pages = output.page_count;
                best = (*params, output.clone());
            }
        }
        for (params, output) in &self.history {
            if output.page_count == min_pages && gentler_than(params, &best.0) {
                best = (*params, output.clone());
            }
        }
        FitOutcome {
            output: best.1,
            applied: best.0,
            compiles: self.compiles,
            overflowed: true,
            underfilled: false,
        }
    }
    /// Phase F (ADR-0044 + ADR-0052): the compile lands under the exact target
    /// page count. Grow spacing, then fonts, then margins — each to the
    /// largest scale whose compile still fits within the target, each stage
    /// entered only when the previous one ended at its cap. If even the caps
    /// leave the pages under target, [`Self::finish_fill`] stretches the
    /// rhythm further to land exactly on it.
    fn fill(
        &mut self,
        base: (FitParams, RenderOutput),
    ) -> std::result::Result<FitOutcome, RenderError> {
        let (spacing_params, spacing_output, spacing_capped) = self.grow_to_cap(
            FILL_MAX_SCALE,
            base.0,
            base.1,
            &|p, s| FitParams {
                spacing_scale: s,
                ..p
            },
            &|p| p.spacing_scale,
        )?;
        if !spacing_capped {
            return self.finish_fill((spacing_params, spacing_output));
        }

        let (font_params, font_output, font_capped) = self.grow_to_cap(
            FILL_MAX_FONT_SCALE,
            spacing_params,
            spacing_output,
            &|p, f| FitParams { font_scale: f, ..p },
            &|p| p.font_scale,
        )?;
        if !font_capped {
            return self.finish_fill((font_params, font_output));
        }

        let (margin_params, margin_output, _) = self.grow_to_cap(
            FILL_MAX_MARGIN_SCALE,
            font_params,
            font_output,
            &|p, m| FitParams {
                margin_scale: m,
                ..p
            },
            &|p| p.margin_scale,
        )?;
        self.finish_fill((margin_params, margin_output))
    }

    /// One fill-stage search: grow the scale `set` writes, from its value in
    /// `base` toward `cap`, to the largest value whose compile still fits
    /// within the target. Returns the adopted params, the output they
    /// produced, and whether the cap itself fit — the caller's signal that
    /// slack remains for the next stage. When no probed point fits, `base`
    /// stays (its output is known-feasible: it either is the identity or just
    /// fit the previous cap).
    fn grow_to_cap(
        &mut self,
        cap: f64,
        base: FitParams,
        base_output: RenderOutput,
        set: &dyn Fn(FitParams, f64) -> FitParams,
        value: &dyn Fn(&FitParams) -> f64,
    ) -> std::result::Result<(FitParams, RenderOutput, bool), RenderError> {
        // The stage's low bound is the inherited value; base scales always
        // sit at or below a cap here (tightening only ever moved them down).
        let lo = value(&base).max(1.0);
        let mut probe = |scale: f64| {
            let output = self.call_params(set(base, scale))?;
            Ok((self.fits(&output), output))
        };
        let grid_best = search_max_feasible(lo, cap, MAX_PHASE_PROBES, &mut probe)?;
        // The cap is the most generous setting the stage has: adopt it
        // whenever it still fits, whether or not a grid point below it did.
        let (fits_cap, at_cap) = probe(cap)?;
        if fits_cap {
            return Ok((set(base, cap), at_cap, true));
        }
        if let Some((scale, output)) = grid_best {
            return Ok((set(base, scale), output, false));
        }
        Ok((base, base_output, false))
    }

    /// ADR-0052: land the result exactly on the target page count. A compile
    /// at or past the target is final. Under it, one bounded search grows the
    /// spacing past the ordinary caps to the gentlest stretch reaching the
    /// target; reaching it exactly adopts that stretch, jumping past it or
    /// ceiling below it reports `underfilled` with the last under-target
    /// compile — a wrong page count is never delivered silently.
    fn finish_fill(
        &mut self,
        (params, output): (FitParams, RenderOutput),
    ) -> std::result::Result<FitOutcome, RenderError> {
        let target = self.config.target_pages;
        if output.page_count >= target {
            return Ok(self.outcome(output, params, false));
        }
        let with_spacing = |p: FitParams, s: f64| FitParams {
            spacing_scale: s,
            ..p
        };
        let (mut a, mut b) = (params.spacing_scale, self.config.max_stretch_spacing);
        let at_ceiling = self.call_params(with_spacing(params, b))?;
        if at_ceiling.page_count < target {
            return Ok(self.underfilled(output, params));
        }
        let mut best = (b, at_ceiling);
        for _ in 0..MAX_PHASE_PROBES {
            let mid = (a + b) / 2.0;
            let landing = self.call_params(with_spacing(params, mid))?;
            if landing.page_count >= target {
                b = mid;
                best = (mid, landing);
            } else {
                a = mid;
            }
        }
        let (scale, landing) = best;
        if landing.page_count == target {
            return Ok(self.outcome(landing, with_spacing(params, scale), false));
        }
        // The page count jumped over the target: no setting spans it exactly.
        Ok(self.underfilled(output, params))
    }

    fn underfilled(&self, output: RenderOutput, applied: FitParams) -> FitOutcome {
        FitOutcome {
            output,
            applied,
            compiles: self.compiles,
            overflowed: false,
            underfilled: true,
        }
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
        // The scale snapshot is taken before the assert, and the failure message is
        // written out to a local: a capture inside the assert macro's panic branch
        // would create an always-zero line record of its own and pull the file's
        // llvm-cov line metric below 100% for an artefact, not untested behaviour.
        let sc: Vec<(f64, f64)> = probes
            .borrow()
            .iter()
            .map(|p| (p.font_scale, p.margin_scale))
            .collect();
        assert!(sc.iter().all(|&(f, _)| f == 1.0));
        assert!(
            sc.iter().all(|&(_, m)| m == 1.0),
            "only spacing moves: {sc:?}"
        );
    }

    #[test]
    fn fill_adopts_the_cap_when_it_still_fits_and_reports_the_pages_underfilled() {
        // Content so sparse that even the spacing ceiling stays one page:
        // the caps are adopted (nothing else was seen), and the exact target
        // is honestly reported as unreachable from below (ADR-0052).
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        // The margin test at the end: every page count fits, every compile succeeds;
        // the oracle is constant by design (a branching one left a dead arm here).
        let outcome = session_growing_with_spacing(&config, |_s| 1, probes.clone())
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
        assert!(outcome.underfilled, "one page cannot span two (ADR-0052)");
        assert!(!outcome.overflowed);
        assert_eq!(
            outcome.compiles,
            1 + 3 * (MAX_PHASE_PROBES + 1) + 1,
            "caps plus the single ceiling probe that settles it"
        );
    }

    #[test]
    fn fill_stretches_past_the_caps_to_land_exactly_on_the_page_count() {
        // Content that stays one page inside the caps but crosses to two at
        // spacing 4: the exact-target search lands the gentlest stretch that
        // reaches two — and adopts it (ADR-0052).
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome =
            session_growing_with_spacing(&config, |s| if s < 4.0 { 1 } else { 2 }, probes)
                .run()
                .expect("fake compile never fails");
        assert!(!outcome.underfilled);
        assert!(!outcome.overflowed);
        assert!(
            (outcome.applied.spacing_scale - 4.125).abs() < 1e-9,
            "the gentlest probed two-page stretch: {:?}",
            outcome.applied
        );
        assert_eq!(outcome.output.page_count, 2);
        assert_eq!(
            outcome.compiles,
            1 + 3 * (MAX_PHASE_PROBES + 1) + 1 + MAX_PHASE_PROBES,
            "caps, then the ceiling probe and the bisection"
        );
    }

    #[test]
    fn a_capped_stretch_lands_on_the_gentlest_stretch_the_ceiling_allows() {
        // `design.fit_stretch_max` = 4.5 with the same shape as the default
        // ceiling test: the bisection range narrows to (3.0, 4.5] and the
        // adopted landing tightens from 4.125 to 4.03125 — still exact.
        let config = FitConfig {
            target_pages: 2,
            max_stretch_spacing: 4.5,
            ..default_config(true)
        };
        let outcome = session_growing_with_spacing(
            &config,
            |s| if s < 4.0 { 1 } else { 2 },
            Rc::new(RefCell::new(Vec::new())),
        )
        .run()
        .expect("fake compile never fails");
        assert!(!outcome.underfilled);
        assert!(!outcome.overflowed);
        assert!(
            (outcome.applied.spacing_scale - 4.03125).abs() < 1e-9,
            "the ceiling reshapes the grid: {:?}",
            outcome.applied
        );
        assert_eq!(outcome.output.page_count, 2);
        assert_eq!(
            outcome.compiles,
            1 + 3 * (MAX_PHASE_PROBES + 1) + 1 + MAX_PHASE_PROBES
        );
    }

    #[test]
    fn a_ceiling_below_the_reachable_stretch_reports_underfilled_without_bisecting() {
        // With `fit_stretch_max` = 3.5 the ceiling probe itself stays at one
        // page: underfilled immediately, and the bisection never runs.
        let config = FitConfig {
            target_pages: 2,
            max_stretch_spacing: 3.5,
            ..default_config(true)
        };
        let outcome = session_growing_with_spacing(
            &config,
            |s| if s < 4.0 { 1 } else { 2 },
            Rc::new(RefCell::new(Vec::new())),
        )
        .run()
        .expect("fake compile never fails");
        assert!(outcome.underfilled);
        assert_eq!(
            outcome.compiles,
            1 + 3 * (MAX_PHASE_PROBES + 1) + 1,
            "caps plus the single ceiling probe"
        );
        assert_eq!(outcome.applied.spacing_scale, FILL_MAX_SCALE);
    }

    #[test]
    fn the_stretch_ceiling_clamps_the_design_knob() {
        let ceiling = |max: f64| {
            stretch_ceiling(&design_with(|d| {
                d.fit_stretch_max = max;
            }))
        };
        assert!((ceiling(5.0) - 5.0).abs() < 1e-12, "in-range values pass");
        assert!(
            (ceiling(0.5) - FILL_MAX_SCALE).abs() < 1e-12,
            "the knob never disables the ordinary fill stages"
        );
        assert!(
            (ceiling(99.0) - FILL_EXTENDED_MAX_SPACING).abs() < 1e-12,
            "and never raises the sanity ceiling"
        );
        assert!(
            (ceiling(f64::NAN) - FILL_EXTENDED_MAX_SPACING).abs() < 1e-12,
            "nonsense degrades to the default"
        );
        assert!(
            (stretch_ceiling(&Design::default()) - FILL_EXTENDED_MAX_SPACING).abs() < 1e-12,
            "the default design uses the documented ceiling"
        );
    }

    #[test]
    fn a_stretch_that_jumps_over_the_target_reports_underfilled_instead() {
        // The page count goes 1 → 3 across the whole spacing range: no
        // setting lands on exactly two, so the last one-page compile stays
        // and the run reports underfilled rather than silently shipping 3.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = session_growing_with_spacing(
            &config,
            |s| if s < 5.0 { 1 } else { 3 },
            Rc::new(RefCell::new(Vec::new())),
        )
        .run()
        .expect("fake compile never fails");
        assert!(outcome.underfilled, "the target was jumped over");
        assert!(!outcome.overflowed);
        assert_eq!(outcome.output.page_count, 1);
        assert_eq!(
            outcome.applied.spacing_scale, FILL_MAX_SCALE,
            "the caps stay, not the jumping stretch"
        );
    }

    #[test]
    fn a_tightened_landing_under_the_target_grows_back_to_it() {
        // Phase A overshoots to one page for a two-page target (counts jump
        // 3 → 1): the settle step chains the fill phase, whose stretch then
        // jumps past two again — honestly underfilled, never silently short.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let outcome = Session::new(&config, |p| {
            Ok(output(if p.spacing_scale <= 0.66 { 1 } else { 3 }))
        })
        .run()
        .expect("compile");
        assert!(outcome.underfilled);
        assert!(!outcome.overflowed);
        assert!(
            (outcome.applied.spacing_scale - 0.65).abs() < 1e-9,
            "the Phase A landing stays when nothing spans two: {:?}",
            outcome.applied
        );
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
    fn a_compile_error_at_the_fill_cap_probe_propagates() {
        // The grid fits everywhere; only the stage's own cap probe breaks — the
        // dedicated `probe(cap)?` after the search (the search never probes the cap,
        // so nothing else reaches it).
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let mut session = Session::new(&config, |params| {
            if (params.spacing_scale - FILL_MAX_SCALE).abs() < 1e-12 {
                Err(RenderError::Template {
                    message: "boom at the spacing cap".into(),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session
            .run()
            .expect_err("the cap-probe failure must escape");
        assert!(error.to_string().contains("spacing cap"), "got: {error}");
        assert_eq!(
            session.compiles,
            1 + MAX_PHASE_PROBES + 1,
            "identity + the grid + the cap probe that broke"
        );
    }

    #[test]
    fn a_compile_error_in_the_margin_fill_stage_propagates() {
        // Both earlier stages cap out fine; the template breaks as soon as the
        // margin stage moves margins.
        let config = FitConfig {
            target_pages: 2,
            ..default_config(true)
        };
        let mut session = Session::new(&config, |params| {
            if params.margin_scale > 1.0
                && params.spacing_scale >= FILL_MAX_SCALE
                && params.font_scale >= FILL_MAX_FONT_SCALE
            {
                Err(RenderError::Template {
                    message: format!("boom at margin {}", params.margin_scale),
                })
            } else {
                Ok(output(2))
            }
        });
        let error = session
            .run()
            .expect_err("the margin-stage failure must escape");
        assert!(error.to_string().contains("boom at margin"), "got: {error}");
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
    fn an_unreachable_target_with_one_page_count_everywhere_keeps_the_identity() {
        // Every compile lands on two pages, so the identity is already the
        // least aggressive compile reaching the smallest page count: the
        // overflow outcome must stop reporting floor scales (ADR-0052) —
        // compressing to the floors bought no page and only damaged typography.
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
        assert_eq!(
            outcome.applied,
            FitParams::default(),
            "identity settings, not the floors: probes: {probes:?}",
        );
    }

    #[test]
    fn an_unreachable_target_adopts_the_gentlest_compile_that_reaches_the_fewest_pages() {
        // Three pages unless the spacing scale drops to 0.66; the target (one
        // page) is unreachable, but two pages is reached by a Phase A midpoint
        // — far gentler than the floors the old behaviour returned.
        let probes = Rc::new(RefCell::new(Vec::new()));
        let config = default_config(true);
        let outcome = Session::new(&config, move |p| {
            probes.borrow_mut().push(p);
            Ok(output(if p.spacing_scale <= 0.66 { 2 } else { 3 }))
        })
        .run()
        .expect("compile");
        assert!(outcome.overflowed);
        assert_eq!(
            outcome.applied.font_scale, 1.0,
            "the found setting keeps full fonts"
        );
        assert!(
            (outcome.applied.spacing_scale - 0.65).abs() < 1e-9,
            "the gentlest probed two-page compile is adopted: {:?}",
            outcome.applied
        );
        assert!(
            outcome.applied.spacing_scale > SPACING_FLOOR,
            "not the floor"
        );
        assert_eq!(
            outcome.compiles,
            1 + (MAX_PHASE_PROBES + 1) * 2,
            "the fallback adds no compiles of its own"
        );
    }

    #[test]
    fn the_overflow_fallback_adopts_the_floors_when_only_the_floors_reach_the_fewest_pages() {
        // Only the exact both-floors compile (font at the floor) gets down to
        // two pages — nothing gentler exists, so the floors are still reported.
        let config = default_config(true);
        let font_floor = 8.5 / 9.5;
        let outcome = Session::new(&config, move |p| {
            Ok(output(if p.font_scale == font_floor { 2 } else { 3 }))
        })
        .run()
        .expect("compile");
        assert!(outcome.overflowed);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(outcome.applied.font_scale, font_floor));
        assert!(near(outcome.applied.spacing_scale, SPACING_FLOOR));
        assert!(
            near(outcome.applied.margin_scale, config.margin_min_scale),
            "got {:?}",
            outcome.applied
        );
        assert_eq!(outcome.compiles, 1 + (MAX_PHASE_PROBES + 1) * 2);
    }

    #[test]
    fn a_target_reached_only_at_the_both_floors_compile_settles_without_overflow() {
        // Phase A's t=1 floor probe (full fonts) still overflows and Phase B's
        // grid misses, but the exact both-floors compile fits the target:
        // the settle path adopts it with `overflowed == false` (spec §7 step
        // 4 reached its happy end, not the ADR-0052 fallback).
        let config = default_config(true);
        let font_floor = 8.5 / 9.5;
        let outcome = Session::new(&config, move |p| {
            let at_floors = p.font_scale == font_floor && p.spacing_scale <= SPACING_FLOOR;
            Ok(output(if at_floors { 1 } else { 3 }))
        })
        .run()
        .expect("compile");
        assert!(!outcome.overflowed, "the target was met, just barely");
        assert!(!outcome.underfilled);
        assert!(
            (outcome.applied.font_scale - font_floor).abs() < 1e-9
                && (outcome.applied.spacing_scale - SPACING_FLOOR).abs() < 1e-9,
            "the both-floors compile is adopted: {:?}",
            outcome.applied
        );
        assert_eq!(outcome.compiles, 1 + (MAX_PHASE_PROBES + 1) * 2);
    }

    #[test]
    fn the_overflow_fallback_repairs_a_floor_compile_that_overshot_its_own_count() {
        // Adversarial layout (the ADR-0024 monotonicity caveat made visible):
        // the both-floors compile lands on THREE pages while gentler probed
        // settings reach two. The fallback must not trust the floors as the
        // minimum — it rescans the history and adopts the smallest count and
        // the gentlest compile that reached it.
        let config = default_config(true);
        let font_floor = 8.5 / 9.5;
        let outcome = Session::new(&config, move |p| {
            let two = p.spacing_scale <= 0.66 && p.font_scale > font_floor;
            Ok(output(if two { 2 } else { 3 }))
        })
        .run()
        .expect("compile");
        assert!(outcome.overflowed, "one page was never reached");
        assert_eq!(
            outcome.output.page_count, 2,
            "two pages is the reachable floor"
        );
        assert!(
            (outcome.applied.spacing_scale - 0.65).abs() < 1e-9
                && outcome.applied.font_scale == 1.0,
            "the gentlest two-page compile: {:?}",
            outcome.applied
        );
    }

    #[test]
    fn the_overflow_fallback_is_deterministic() {
        let run_once = || {
            let config = default_config(true);
            let outcome = Session::new(&config, move |p| {
                Ok(output(if p.spacing_scale <= 0.66 { 2 } else { 3 }))
            })
            .run()
            .expect("compile");
            (outcome.applied, outcome.compiles, outcome.overflowed)
        };
        assert_eq!(
            format!("{:?}", run_once()),
            format!("{:?}", run_once()),
            "determinism (spec §7 hard requirement)"
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
            max_stretch_spacing: FILL_EXTENDED_MAX_SPACING,
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
            max_stretch_spacing: FILL_EXTENDED_MAX_SPACING,
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
            max_stretch_spacing: FILL_EXTENDED_MAX_SPACING,
        }
    }
}
