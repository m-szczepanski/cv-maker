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
//! No Typst types here: the loop only sees [`RenderOutput`] page counts, and the
//! scales reach the template through the `fit` inputs (`src/render/`).

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
    Session {
        engine,
        config,
        compiles: 0,
    }
    .run()
}

struct Session<'a> {
    engine: &'a Engine,
    config: &'a FitConfig,
    compiles: usize,
}

fn fits(output: &RenderOutput) -> bool {
    // A paged document always has ≥ 1 page; `<= 1` only guards the impossible 0.
    output.page_count <= 1
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

impl Session<'_> {
    fn compile(
        &mut self,
        font_scale: f64,
        t: f64,
    ) -> std::result::Result<RenderOutput, RenderError> {
        self.compiles += 1;
        self.engine
            .compile(scales_at(self.config.margin_min_scale, font_scale, t))
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
        let identity = self.compile(1.0, 0.0)?;
        if !self.config.enabled || fits(&identity) {
            // Hard requirement (spec §7): never shrink what already fits.
            return Ok(self.outcome(identity, FitParams::default(), false));
        }

        // Phase A: least aggressive tightness that fits, fonts at full size.
        let margin_min = self.config.margin_min_scale;
        if let Some((t, output)) = search_min_feasible(MAX_PHASE_PROBES, &mut |t| {
            self.compile(1.0, t).map(|o| (fits(&o), o))
        })? {
            return Ok(self.outcome(output, scales_at(margin_min, 1.0, t), false));
        }

        let at_floors = self.compile(1.0, 1.0)?;
        if fits(&at_floors) {
            return Ok(self.outcome(at_floors, scales_at(margin_min, 1.0, 1.0), false));
        }

        // Phase B: largest font scale that fits at full tightness.
        let floor = self.config.font_floor;
        if floor >= 1.0 {
            // Font shrinking is floored at the original size: the Phase A floor
            // probe is already the most aggressive compile.
            return Ok(self.outcome(at_floors, scales_at(margin_min, 1.0, 1.0), true));
        }
        if let Some((f, output)) = search_max_feasible(floor, MAX_PHASE_PROBES, &mut |f| {
            self.compile(f, 1.0).map(|o| (fits(&o), o))
        })? {
            return Ok(self.outcome(output, scales_at(margin_min, f, 1.0), false));
        }

        let at_floor = self.compile(floor, 1.0)?;
        let overflowed = !fits(&at_floor);
        Ok(self.outcome(at_floor, scales_at(margin_min, floor, 1.0), overflowed))
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

    fn design_with(f: impl FnOnce(&mut Design)) -> Design {
        let mut design = Design::default();
        f(&mut design);
        design
    }

    #[test]
    fn margin_floor_is_set_by_the_smaller_margin() {
        let margins = Margins {
            x: Length::new(1.5, LengthUnit::Cm),
            y: Length::new(1.2, LengthUnit::Cm),
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(margin_min_scale(&margins), 1.0 / 1.2));
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
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // 8.5/9.5 = 0.8947 is stricter than 0.85.
        assert!(near(font_floor_scale(&design), 8.5 / 9.5));

        let design = design_with(|d| {
            d.sizes.body = Length::new(9.5, LengthUnit::Pt);
            d.fit_floor.min_scale = 0.95;
        });
        assert!(near(font_floor_scale(&design), 0.95));
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
                font_floor_scale(&design) >= (9.5 / 9.5),
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
}
