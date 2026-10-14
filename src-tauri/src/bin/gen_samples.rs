//! Synthetic stimulus generator for the TMP Companion.
//!
//! Produces guitar/bass-like WAVs (48 kHz mono f32) into `resources/samples/`,
//! one per pickup topology in the shared catalog
//! (`tmp_companion_lib::topologies::TOPOLOGIES`). Each is a short musical
//! phrase — strummed chords and single notes for guitar, a bass line for bass —
//! played on Karplus–Strong plucked strings and passed through a resonant
//! 2nd-order low-pass that models the pickup's loaded resonance. Pitched
//! strings give the stimulus a real guitar's harmonic structure and note-to-note
//! dynamics, and the low-pass gives it a real pickup's spectral tilt: the old
//! band-passed-noise recipe carried several times a DI's energy above 2.5 kHz,
//! so bright and high-gain presets were mis-levelled against real playing.
//!
//! Each topology's `peak` is the output level expressed as **input drive** — the
//! stimulus is injected pre-amp, so peak amplitude sets how hard the amp model is
//! pushed (a hot pickup saturates a high-gain preset more than a weak one).
//!
//! Committed WAVs are reproducible: a fixed PRNG seed → byte-identical output.
//! Run after editing the catalog: `cargo run --bin gen_samples`.

use tmp_companion_lib::topologies::{Topology, TOPOLOGIES};

const SR: u32 = 48_000;
const SECS: f32 = 6.0;

/// Loop-filter brightness of the string model: the two-tap average weight.
/// 0.5 is the classic Karplus–Strong average (high partials decay fastest).
const LOOP_BRIGHTNESS: f32 = 0.5;

/// Deterministic xorshift PRNG → reproducible committed samples.
struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let u = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11; // 53 significant bits
        (u as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0 // [-1, 1)
    }
}

/// RBJ-cookbook low-pass biquad, resonant at `q` > 0.707 — the pickup model.
struct LowPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}
impl LowPass {
    fn new(freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / SR as f32;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        LowPass {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// One plucked note, added into `out` from sample `start`. A Karplus–Strong
/// string: a noise burst one period long, notched by the pick position (a comb)
/// and softened by the pick (a one-pole low-pass, twice), recirculates through a
/// lossy two-tap average tuned so the note falls 60 dB in `ring_s`.
#[allow(clippy::too_many_arguments)]
fn pluck(
    out: &mut [f32],
    start: usize,
    freq: f32,
    amp: f32,
    ring_s: f32,
    pick_pos: f32,
    softness: f32,
    rng: &mut Rng,
) {
    let n = (SR as f32 / freq).round() as usize;
    let noise: Vec<f32> = (0..n).map(|_| rng.next_f32()).collect();
    // Pick-position comb: plucking at fraction `pick_pos` of the string cancels
    // the harmonics with a node there.
    let d = ((pick_pos * n as f32) as usize).max(1);
    let mut line: Vec<f32> = (0..n)
        .map(|i| noise[i] - if i >= d { noise[i - d] } else { 0.0 })
        .collect();
    let mut state = 0.0f32;
    for _ in 0..2 {
        for v in line.iter_mut() {
            state = softness * state + (1.0 - softness) * *v;
            *v = state;
        }
    }
    let periods = ring_s * freq;
    let rho = 10f32.powf(-3.0 / periods).min(0.9999);
    let len = (ring_s * 1.2 * SR as f32) as usize;
    let mut idx = 0;
    let mut prev = 0.0f32;
    for k in 0..len {
        let Some(o) = out.get_mut(start + k) else {
            break;
        };
        let cur = line[idx];
        line[idx] = rho * ((1.0 - LOOP_BRIGHTNESS) * cur + LOOP_BRIGHTNESS * prev);
        prev = cur;
        idx = (idx + 1) % n;
        *o += amp * cur;
    }
}

fn midi_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

/// One phrase event: start time (s), the notes as MIDI numbers, and whether
/// they are strummed (a chord, 12 ms apart, ringing longer) or a single note.
type Event = (f32, &'static [f32], bool);

/// Open chords (E, Am, G, D) with single-note runs between them, across the
/// neck — the guitar phrase.
const GUITAR_PHRASE: &[Event] = &[
    (0.00, &[40.0, 47.0, 52.0, 56.0, 59.0, 64.0], true),
    (0.75, &[52.0], false),
    (1.05, &[55.0], false),
    (1.35, &[57.0], false),
    (1.65, &[45.0, 52.0, 57.0, 60.0, 64.0], true),
    (2.40, &[64.0], false),
    (2.62, &[67.0], false),
    (2.84, &[69.0], false),
    (3.10, &[43.0, 47.0, 50.0, 55.0, 59.0, 67.0], true),
    (3.85, &[71.0], false),
    (4.10, &[74.0], false),
    (4.35, &[76.0], false),
    (4.60, &[50.0, 57.0, 62.0, 66.0], true),
    (5.30, &[45.0], false),
    (5.55, &[40.0], false),
];

/// An eighth-note bass line from low E down to low D — the bass phrase.
const BASS_PHRASE: &[Event] = &[
    (0.00, &[28.0], false),
    (0.50, &[28.0], false),
    (1.00, &[40.0], false),
    (1.50, &[35.0], false),
    (2.00, &[33.0], false),
    (2.50, &[33.0], false),
    (3.00, &[45.0], false),
    (3.50, &[40.0], false),
    (4.00, &[31.0], false),
    (4.50, &[38.0], false),
    (5.00, &[36.0], false),
    (5.50, &[26.0], false),
];

/// The phrase and transposition (semitones) an instrument plays. Bass VI is
/// tuned an octave below a guitar, so it plays the guitar phrase an octave down.
fn phrase_for(instrument: &str) -> (&'static [Event], f32) {
    match instrument {
        "bass" => (BASS_PHRASE, 0.0),
        "bass-vi" => (GUITAR_PHRASE, -12.0),
        _ => (GUITAR_PHRASE, 0.0),
    }
}

/// Synthesize one stimulus from a topology: the instrument's phrase on plucked
/// strings, through the pickup low-pass, normalized to the topology's `peak`
/// drive level. Deterministic in `seed`.
fn synth(top: &Topology) -> Vec<f32> {
    let n = (SECS * SR as f32) as usize;
    let mut out = vec![0.0f32; n];
    let mut rng = Rng(top.seed);
    let (phrase, transpose) = phrase_for(top.instrument);
    for &(t, notes, strum) in phrase {
        let gap = if strum { 0.012 } else { 0.0 };
        let ring = if strum { 1.6 } else { 0.9 };
        for (i, &m) in notes.iter().enumerate() {
            let start = ((t + gap * i as f32) * SR as f32) as usize;
            let amp = if strum { 0.55 } else { 0.9 } * (0.85 + 0.15 * rng.next_f32());
            let pick_pos = 0.13 + 0.04 * rng.next_f32();
            pluck(
                &mut out,
                start,
                midi_hz(m + transpose),
                amp,
                ring,
                pick_pos,
                top.pick_softness,
                &mut rng,
            );
        }
    }

    let mut pickup = LowPass::new(top.freq, top.q);
    for s in &mut out {
        *s = pickup.process(*s);
    }

    // Normalize to the topology's peak (its output level as amp-input drive).
    let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs())).max(1e-9);
    let g = top.peak / peak;
    for s in &mut out {
        *s *= g;
    }
    out
}

fn write_wav(path: &str, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for &s in samples {
        w.write_sample(s).expect("write");
    }
    w.finalize().expect("finalize");
    eprintln!("wrote {path} ({} samples, {:.1}s)", samples.len(), SECS);
}

fn main() {
    // Resolve resources/samples relative to the crate dir so it works from
    // any cwd (`cargo run` sets CARGO_MANIFEST_DIR).
    let dir = format!("{}/resources/samples", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).expect("mkdir samples");

    for top in TOPOLOGIES {
        let samples = synth(top);
        write_wav(&format!("{dir}/{}.wav", top.id), &samples);
    }
    eprintln!("done — {} stimulus samples in {dir}", TOPOLOGIES.len());
}
