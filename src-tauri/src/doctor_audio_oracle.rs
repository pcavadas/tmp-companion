//! Captured-audio verdict coverage for the Doctor rules the online oracle preset
//! (407) cannot fire on the real device (#199).
//!
//! Each test runs the SHIPPED guitar stimulus through a real filter (an RBJ
//! biquad, the textbook defect shape) and then through the PRODUCTION analysis
//! path ([`crate::leveller::analyze_capture`] → [`crate::doctor::diagnose_kind`])
//! — the same onset, body window, transfer and rule code a device capture
//! takes. Nothing here forces a verdict: the band powers, tilt and peaks all
//! come out of the audio.
//!
//! Every rule gets a pair bracketing its gate: a positive whose margin over the
//! gate is bounded (so a gate that loosens or tightens materially breaks it)
//! and a near-threshold negative control the same filter shape at a lower gain.

#![cfg(test)]

use crate::doctor::{self, Family, PlaybackOffsets, StimulusKind};
use crate::leveller::{
    analyze_capture, doctor_stim_slice, stim_body_psd, DoctorSignal, DOCTOR_TAIL_MS,
};

const SR: u32 = 48_000;

fn stimulus() -> Vec<f32> {
    let path = format!(
        "{}/resources/samples/guitar-humbucker.wav",
        env!("CARGO_MANIFEST_DIR")
    );
    doctor_stim_slice(crate::stimulus::read_stimulus_calibrated(&path, None).unwrap())
}

#[derive(Clone, Copy)]
enum Shape {
    Peak,
    HighShelf,
    LowPass,
}

/// RBJ cookbook biquad of `shape` at `f0` (Hz), quality `q`, gain `gain_db`.
fn biquad(x: &[f32], shape: Shape, f0: f64, q: f64, gain_db: f64) -> Vec<f32> {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / f64::from(SR);
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q);
    let (b, f) = match shape {
        Shape::Peak => (
            [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        ),
        Shape::LowPass => (
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        ),
        Shape::HighShelf => {
            let k = 2.0 * a.sqrt() * alpha;
            (
                [
                    a * ((a + 1.0) + (a - 1.0) * cos + k),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - k),
                ],
                [
                    (a + 1.0) - (a - 1.0) * cos + k,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - k,
                ],
            )
        }
    };
    let (b0, b1, b2) = (b[0] / f[0], b[1] / f[0], b[2] / f[0]);
    let (a1, a2) = (f[1] / f[0], f[2] / f[0]);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    x.iter()
        .map(|&v| {
            let v = f64::from(v);
            let y = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, v, y1, y);
            y as f32
        })
        .collect()
}

/// The production verdicts (key, severity) for `stim` played through `chain`,
/// captured stereo at the HW inject latency with a dry tail.
fn verdicts(stim: &[f32], chain: &dyn Fn(&[f32]) -> Vec<f32>) -> Vec<(&'static str, f64)> {
    let mut cap = vec![0.0f32; SR as usize * 32 / 1000];
    cap.extend(chain(stim));
    cap.extend(std::iter::repeat_n(0.0, SR as usize * 3 / 10));
    let sig = DoctorSignal {
        channels: vec![cap.clone(), cap.clone()],
        mix: cap,
    };
    let pc = analyze_capture(
        &sig,
        SR,
        stim,
        &stim_body_psd(stim, SR),
        Family::Guitar,
        DOCTOR_TAIL_MS,
        None,
    )
    .unwrap();
    doctor::diagnose_kind(
        &pc.profile,
        None,
        Family::Guitar,
        StimulusKind::Synthetic,
        Some(&pc.coverage),
        PlaybackOffsets::NONE,
    )
    .iter()
    .map(|d| (d.key, d.severity))
    .collect()
}

/// The healthy chain every band-rule pair starts from: the pitched pluck sits
/// several dB under the guitar target above 3 kHz, so a +12 dB high shelf at
/// 2.5 kHz voices it onto the target (every local within 2 dB of it) without
/// firing anything.
fn healthy(x: &[f32]) -> Vec<f32> {
    biquad(x, Shape::HighShelf, 2500.0, 0.7, 12.0)
}

/// A hard-driven amp with no cabinet: `tanh` at 64x the stimulus peak, whose
/// harmonics run past 10 kHz the way real fizz does (an EQ boost cannot make
/// it: the pluck carries almost nothing up there to lift).
fn driven(x: &[f32]) -> Vec<f32> {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    x.iter().map(|v| (64.0 * v / peak).tanh() * 0.3).collect()
}

/// A cabinet's high cut: two cascaded 2nd-order Butterworth low-passes at `fc`.
fn cab(x: &[f32], fc: f64) -> Vec<f32> {
    let once = biquad(x, Shape::LowPass, fc, std::f64::consts::FRAC_1_SQRT_2, 0.0);
    biquad(
        &once,
        Shape::LowPass,
        fc,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
    )
}

fn keys(v: &[(&'static str, f64)]) -> Vec<&'static str> {
    v.iter().map(|(k, _)| *k).collect()
}

/// `positive` must fire exactly `key`, with a margin over its gate inside
/// `margin` (dB, or dB/oct for bright) — so a gate that tightens past the low
/// end, or a rule that is disabled, fails here — and `negative` (the same
/// shape, weaker) must fire nothing, so a gate that loosens past it fails too.
fn assert_bracket(
    key: &str,
    positive: &[(&'static str, f64)],
    negative: &[(&'static str, f64)],
    margin: std::ops::Range<f64>,
) {
    assert_eq!(keys(positive), vec![key], "positive {key}: {positive:?}");
    let m = positive[0].1;
    assert!(
        margin.contains(&m),
        "{key} margin {m:.2} outside {margin:?}"
    );
    assert!(
        negative.is_empty(),
        "negative {key} control fired {negative:?}"
    );
}

#[test]
fn the_healthy_chain_fires_nothing() {
    let stim = stimulus();
    assert!(verdicts(&stim, &healthy).is_empty());
}

// Measured: +18 dB reads boomy at margin 1.8 (tilt-split lows local 5.9 vs
// the 4.1 gate, centered 8.9 vs 4.7); +12 dB reads a lows local of 1.9.
#[test]
fn a_narrow_low_bump_fires_boomy_and_a_smaller_one_does_not() {
    let stim = stimulus();
    let bump = |g: f64| move |x: &[f32]| biquad(&healthy(x), Shape::Peak, 80.0, 1.4, g);
    assert_bracket(
        "boomy",
        &verdicts(&stim, &bump(18.0)),
        &verdicts(&stim, &bump(12.0)),
        1.0..2.5,
    );
}

// Measured: +18 dB reads harsh at margin 2.9 (high-mids local 6.8 vs 3.9);
// +9 dB reads a local of 2.0. Q 2 keeps it a band defect, not a ring.
#[test]
fn a_high_mid_push_fires_harsh_and_a_smaller_one_does_not() {
    let stim = stimulus();
    let push = |g: f64| move |x: &[f32]| biquad(&healthy(x), Shape::Peak, 2000.0, 2.0, g);
    assert_bracket(
        "harsh",
        &verdicts(&stim, &push(18.0)),
        &verdicts(&stim, &push(9.0)),
        2.0..3.5,
    );
}

// Measured: a +18 dB shelf tilts the whole tone 1.2 dB/oct past the 1.8
// dB/oct gate; +12 dB sits 0.2 over it and +9 dB under it.
#[test]
fn a_broad_treble_lift_fires_bright_and_a_smaller_one_does_not() {
    let stim = stimulus();
    let lift = |g: f64| move |x: &[f32]| biquad(&healthy(x), Shape::HighShelf, 500.0, 0.4, g);
    assert_bracket(
        "bright",
        &verdicts(&stim, &lift(18.0)),
        &verdicts(&stim, &lift(9.0)),
        0.7..1.7,
    );
}

// Measured: the driven amp through a 10 kHz cab reads Air 7.3 dB under the
// Highs band, 2.0 past the -9.3 dB gate; through a 6 kHz cab, 13.7 under.
#[test]
fn fizz_past_a_high_cab_cut_fires_fizzy_and_a_lower_cut_tames_it() {
    let stim = stimulus();
    assert_bracket(
        "fizzy",
        &verdicts(&stim, &|x| cab(&driven(x), 10_000.0)),
        &verdicts(&stim, &|x| cab(&driven(x), 6_000.0)),
        1.0..3.0,
    );
}
