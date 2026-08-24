//! Test-only fixtures and synthetic-signal helpers shared across module test
//! suites (`audio`'s onset tests, `leveller`'s Doctor onset-gate tests) —
//! ONE home so a shared fixture/generator can't drift between the two.
//! `#[cfg(test)]`-gated at the `mod test_support;` declaration in `lib.rs`;
//! nothing here is compiled into a release binary.

/// A pluck train with a distinctive envelope (like the shipped stimuli) — a
/// synthetic stand-in for the real guitar-humbucker Doctor stimulus, which
/// isn't a bundled test asset.
pub(crate) fn plucky(secs: f32) -> Vec<f32> {
    use std::f32::consts::PI;
    const SR: u32 = 48_000;
    let n = (secs * SR as f32) as usize;
    let note = SR as usize / 2; // 500 ms notes
    (0..n)
        .map(|i| {
            let t = (i % note) as f32 / SR as f32;
            let env = (-t / 0.12).exp();
            env * (2.0 * PI * 220.0 * i as f32 / SR as f32).sin() * 0.5
        })
        .collect()
}

/// A tiny deterministic LCG (no new dependency) — just needs to be
/// unpredictable enough that per-hop noise can't accidentally correlate with
/// a stimulus envelope.
pub(crate) struct Lcg(pub(crate) u64);
impl Lcg {
    pub(crate) fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f64 / (1u64 << 24) as f64 - 1.0) as f32
    }
}

/// 2 ms hop at 48 kHz — matches the `fs13_wash_envelope_2ms` fixture's
/// envelope resolution.
const HOP: usize = 96;

/// Reconstruct a capture whose 2 ms-hop RMS matches `envelope` exactly: each
/// hop is deterministic LCG noise normalised to unit RMS, then scaled by that
/// hop's envelope value — so `doctor::tail_energy_ratio` (which only ever
/// reads RMS over sample ranges) reproduces the real capture's numbers.
pub(crate) fn reconstruct_capture(envelope: &[f64]) -> Vec<f32> {
    let mut lcg = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::with_capacity(envelope.len() * HOP);
    for &e in envelope {
        let mut hop: Vec<f32> = (0..HOP).map(|_| lcg.next_f32()).collect();
        let rms = (hop
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / HOP as f64)
            .sqrt();
        let scale = if rms > 0.0 { e / rms } else { 0.0 };
        for v in &mut hop {
            *v = (f64::from(*v) * scale) as f32;
        }
        out.extend(hop);
    }
    out
}

/// 2 ms RMS envelope of a REAL fs13 (`ACD_TMLargePlate`, 65%-wet) Doctor
/// capture — see `fixtures/fs13_wash_envelope_2ms.txt`'s header for
/// provenance and the pinned ground-truth `tail_energy_ratio` table.
const FS13_ENVELOPE: &str = include_str!("fixtures/fs13_wash_envelope_2ms.txt");

pub(crate) fn fs13_envelope() -> Vec<f64> {
    FS13_ENVELOPE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.parse::<f64>().expect("fixture line parses as f64"))
        .collect()
}

pub(crate) fn fs13_capture() -> Vec<f32> {
    reconstruct_capture(&fs13_envelope())
}
