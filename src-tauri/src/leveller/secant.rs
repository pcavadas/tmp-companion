//! The one secant step every leveling solve takes ([`secant_step`]), with a named
//! [`Flatness`] rule per unit and the scene lane's trust region ([`BATCH_TRUST_DB`]).

/// What a FLAT local response is — "this move did not change the loudness, stop" — in
/// each secant solve's own x unit. One named rule per unit, so no solve carries a bare
/// cutoff of its own. Every rule also rejects a non-finite slope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flatness {
    /// The footswitch/handle solve's COORD space ([`FsParamTarget::to_coord`]): flat below
    /// `|slope| < 1e-3`. For a `LevelLinear` param the coordinate is log-knob
    /// (`|Δcoord| ≤ ~60 dB` on `[`[`KNOB_LOG_FLOOR`]`, 1]`, so any pair with ≥ 0.3 LU
    /// separation has `|slope| ≥ 0.005`); `LevelDb`/`WetMix` map identically. Two probed
    /// values that both fall below [`KNOB_LOG_FLOOR`] collapse to the SAME coordinate `-60`
    /// — a degenerate pair with a non-finite slope, which turns into a graceful
    /// `unconverged` rather than a divide-by-zero.
    SolveCoord,
    /// A knob-search coordinate ([`knob_search_space`]), LUFS per knob unit: a pair less
    /// than `1e-9` apart has no slope, and `|slope| < 1e-4` is flat.
    Knob,
    /// The scene lane's applied gain in dB: the response must RISE, by more than 0.05 LU per
    /// dB — a falling or ~zero slope is no usable response.
    AppliedDb,
}

impl Flatness {
    /// Whether a pair `dx` apart is too close to have a slope at all.
    pub(super) fn degenerate_dx(self, dx: f64) -> bool {
        match self {
            Flatness::Knob => dx.abs() < 1e-9,
            Flatness::SolveCoord | Flatness::AppliedDb => false,
        }
    }

    /// Whether a finite `slope` is flat.
    pub(super) fn is_flat(self, slope: f64) -> bool {
        match self {
            Flatness::SolveCoord => slope.abs() < 1e-3,
            Flatness::Knob => slope.abs() < 1e-4,
            Flatness::AppliedDb => slope <= 0.05,
        }
    }
}

/// A secant x coordinate: `f64` for the solve and applied-dB spaces, `f32` for the knob
/// search, whose difference and step are taken exactly as its knob values are stored.
pub(crate) trait SecantCoord: Copy {
    /// `b - a`, as the slope's denominator.
    fn diff(a: Self, b: Self) -> f64;
    fn to_f64(self) -> f64;
    /// The next coordinate, or `None` when it is not representable.
    fn from_f64(x: f64) -> Option<Self>;
}

impl SecantCoord for f64 {
    fn diff(a: f64, b: f64) -> f64 {
        b - a
    }
    fn to_f64(self) -> f64 {
        self
    }
    fn from_f64(x: f64) -> Option<f64> {
        Some(x)
    }
}

impl SecantCoord for f32 {
    fn diff(a: f32, b: f32) -> f64 {
        (b - a) as f64
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(x: f64) -> Option<f32> {
        x.is_finite().then_some(x as f32)
    }
}

/// THE secant step every leveling solve takes: from two `(x, loudness)` points, the `x`
/// that should hit `target` on the line through them. `None` when the local response is
/// flat by `flat`'s rule for that unit (the control doesn't move loudness here, so the
/// caller stops). With `trust`, the step is clamped to `p1.x ± trust` so a noisy near-zero
/// slope can't explode the jump; without it the result is UNCLAMPED and the caller clamps
/// to its own bounds.
pub(crate) fn secant_step<X: SecantCoord>(
    p0: (X, f64),
    p1: (X, f64),
    target: f64,
    flat: Flatness,
    trust: Option<f64>,
) -> Option<X> {
    let dx = X::diff(p0.0, p1.0);
    if flat.degenerate_dx(dx) {
        return None;
    }
    let slope = (p1.1 - p0.1) / dx;
    if !slope.is_finite() || flat.is_flat(slope) {
        return None;
    }
    let x1 = p1.0.to_f64();
    let next = x1 + (target - p1.1) / slope;
    X::from_f64(match trust {
        Some(t) => next.clamp(x1 - t, x1 + t),
        None => next,
    })
}

/// Trust region for the slope-jump controller (max dB the knob moves per trim):
/// full computed jumps overshot steep nonlinear knobs by ~6 LU on HW.
pub(crate) const BATCH_TRUST_DB: f32 = 6.0;
