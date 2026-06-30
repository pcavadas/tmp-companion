//! Synthetic stimulus generator for the TMP Companion.
//!
//! Produces guitar/bass-like shaped-noise WAVs (48 kHz mono f32) into
//! `resources/samples/`, one per pickup topology in the shared catalog
//! (`tmp_companion_lib::topologies::TOPOLOGIES`). These are NOT real guitar
//! DIs — they are pink-ish noise band-passed to each pickup type's resonant peak
//! and shaped with a sequence of plucked exponential-decay envelopes, so the
//! crest factor lands near a real guitar's (~15 dB) rather than flat noise's
//! ~3 dB. This makes the re-amp loudness reading representative of musical
//! playing for CLEAN presets; high-gain amp distortion is crest-factor/spectrum-
//! and drive-dependent, so for those the synthetic reading is a first guess that
//! Tier-2 calibration refines.
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

/// RBJ-cookbook bandpass biquad (constant 0 dB peak gain), processed in place.
struct Biquad {
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
impl Biquad {
    fn bandpass(freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / SR as f32;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Biquad {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
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

/// One pluck envelope sample at time `t` within a note (fast attack, exp decay).
/// `attack_s` sets percussiveness — short = piezo/acoustic, long = bass.
fn pluck_env(t: f32, decay_tau: f32, attack_s: f32) -> f32 {
    let attack = (t / attack_s).min(1.0);
    attack * (-t / decay_tau).exp()
}

/// Synthesize one stimulus from a topology: resonant-bandpassed noise mixed with
/// a low-passed "body" voice, gated by a sequence of plucked notes, normalized to
/// the topology's `peak` drive level. Deterministic in `seed`. The `bp_mix` /
/// `body_mix` split sets character: passive pickups lean on the resonant peak;
/// active/acoustic lean on the flatter body voice.
fn synth(top: &Topology) -> Vec<f32> {
    let n = (SECS * SR as f32) as usize;
    let mut rng = Rng(top.seed);
    let mut bp = Biquad::bandpass(top.freq, top.q);
    let mut lp_state = 0.0f32; // one-pole lowpass for pink-ish body
    let attack_s = top.attack_ms / 1000.0;

    // 10 plucked notes across the 6 s span, each with its own decay.
    let note_len = SECS / 10.0;
    let note_n = (note_len * SR as f32) as usize;

    let mut out = vec![0.0f32; n];
    for (i, sample) in out.iter_mut().enumerate() {
        let white = rng.next_f32();
        let resonant = bp.process(white);
        // pink-ish body: leaky integrator of white noise
        lp_state = 0.98 * lp_state + 0.02 * white;
        let voice = top.bp_mix * resonant + top.body_mix * lp_state;
        // pluck gate
        let t_in_note = (i % note_n) as f32 / SR as f32;
        // alternate decay times so notes vary (rhythm + sustained)
        let decay = if (i / note_n).is_multiple_of(2) {
            0.22
        } else {
            0.4
        };
        *sample = voice * pluck_env(t_in_note, decay, attack_s);
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
