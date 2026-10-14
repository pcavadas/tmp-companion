//! Pickup-topology catalog — the single source of truth shared by the synthetic
//! stimulus generator (`bin/gen_samples.rs`) and the runtime (`lib.rs`).
//!
//! Each entry maps a pickup *character* to the synth parameters that shape its
//! stimulus WAV. The crucial axis for leveling is `peak`: in re-amp mode the
//! stimulus is injected at the instrument input (pre-amp), so its peak amplitude
//! sets how hard the amp model is *driven*. A hot pickup into a high-gain preset
//! saturates more (and compresses) than a weak one — modeling output level as
//! drive is what lets the leveler match loudness across (preset, instrument)
//! pairs. Output level is therefore `peak`, NOT a post-hoc gain.
//!
//! A `Topology::id` doubles as the WAV file stem (`resources/samples/<id>.wav`)
//! and as the stable key a saved instrument profile references — keep them in
//! lockstep (the `every_topology_has_a_wav` test guards drift). Profiles may
//! instead reference an `ALIASES` id (a familiar pickup name mapped to a parent
//! topology) — alias ids are NOT WAV stems; `canonical_id` resolves them first.

/// One pickup topology: display metadata + the deterministic synth recipe.
///
/// The recipe is a plucked-string model (`bin/gen_samples.rs`): Karplus–Strong
/// strings play a fixed musical phrase for the instrument, and the summed
/// string signal goes through a resonant 2nd-order low-pass — the pickup's
/// loaded electrical resonance (coil inductance against cable + pot
/// capacitance). A real pickup is a low-pass with a resonant hump, not a
/// band-pass: the old band-passed-noise recipe put far too much energy above
/// 2.5 kHz and mis-levelled bright presets (hardware A/B against real DIs).
#[derive(Debug, Clone, Copy)]
pub struct Topology {
    /// Stable id == WAV file stem == profile reference key, for CANONICAL
    /// topologies only. A profile may instead hold an `ALIASES` id — not a WAV
    /// stem — which must be resolved via `canonical_id` before any WAV lookup.
    pub id: &'static str,
    /// UI display label.
    pub label: &'static str,
    /// "guitar" | "bass" | "bass-vi" — groups the dropdown and picks the
    /// phrase the generator plays.
    pub instrument: &'static str,
    /// Deterministic PRNG seed → reproducible committed WAV.
    pub seed: u64,
    /// Loaded resonance (Hz) of the pickup low-pass. Passive guitar values are
    /// typical measured loaded resonances (humbucker ~2.8 kHz, single-coil
    /// ~4.5 kHz); active and piezo pickups are low-impedance, so their corner
    /// sits high and flat.
    pub freq: f32,
    /// Resonance Q of the pickup low-pass — high = peaky (passive), ≤0.707 =
    /// no hump (active/piezo).
    pub q: f32,
    /// Output level as **input drive**: stimulus peak amplitude (0..1).
    pub peak: f32,
    /// Pick softness: the one-pole pole applied (twice) to each string's
    /// excitation. Higher = a rounder attack with less top end.
    pub pick_softness: f32,
}

/// The shipped catalog — **one stimulus per pickup character**. The output-level
/// axis (low/med/high) was removed once Tier-2 calibration began measuring real
/// output directly: calibration sets *level*, so a topology only needs to set
/// *spectrum*. `peak` is a single per-character default that drives the amp
/// for UNcalibrated profiles; a calibrated profile overrides it (see
/// `read_stimulus_calibrated`).
///
/// The passive guitar rows and the bass humbucker are the hardware-checked
/// recipes; the active, acoustic, other bass and Bass VI rows extrapolate the
/// same model from each pickup's electrical character.
pub const TOPOLOGIES: &[Topology] = &[
    // ── Guitar ─────────────────────────────────────────────────────────────
    Topology {
        id: "guitar-singlecoil",
        label: "Single-coil",
        instrument: "guitar",
        seed: 0x1111_0003_0000_0003,
        freq: 4500.0,
        q: 2.8,
        peak: 0.50,
        pick_softness: 0.93,
    },
    Topology {
        id: "guitar-humbucker",
        label: "Humbucker",
        instrument: "guitar",
        seed: 0x2222_0003_0000_0003,
        freq: 2800.0,
        q: 2.0,
        peak: 0.55,
        pick_softness: 0.93,
    },
    Topology {
        id: "guitar-active",
        label: "Active (EMG/Fluence)",
        instrument: "guitar",
        seed: 0x3333_0002_0000_0002,
        freq: 6000.0,
        q: 0.9,
        peak: 0.85,
        pick_softness: 0.93,
    },
    Topology {
        id: "guitar-acoustic",
        label: "Acoustic / piezo",
        instrument: "guitar",
        seed: 0x4444_0002_0000_0002,
        freq: 9000.0,
        q: 0.7,
        peak: 0.60,
        pick_softness: 0.88,
    },
    // ── Bass ───────────────────────────────────────────────────────────────
    Topology {
        id: "bass-singlecoil",
        label: "Bass single-coil",
        instrument: "bass",
        seed: 0x5555_0003_0000_0003,
        freq: 3200.0,
        q: 2.2,
        peak: 0.55,
        pick_softness: 0.93,
    },
    Topology {
        id: "bass-humbucker",
        label: "Bass humbucker",
        instrument: "bass",
        seed: 0x6666_0003_0000_0003,
        freq: 2500.0,
        q: 1.8,
        peak: 0.60,
        pick_softness: 0.93,
    },
    Topology {
        id: "bass-active",
        label: "Bass active",
        instrument: "bass",
        seed: 0x7777_0002_0000_0002,
        freq: 5000.0,
        q: 0.8,
        peak: 0.85,
        pick_softness: 0.93,
    },
    // ── Bass VI ──────────────────────────────────────────────────────────────
    // A baritone six-string (E1..E4) with bright Jaguar-style single coils: the
    // guitar phrase an octave down through a single-coil-like pickup.
    // PROVISIONAL — a calibrated profile overrides it; the doctor side gives it
    // its own 7-band layout (a Sub band) for the sub-60 Hz octave.
    Topology {
        id: "bass-vi",
        label: "Bass VI",
        instrument: "bass-vi",
        seed: 0x8888_0003_0000_0003,
        freq: 4000.0,
        q: 2.4,
        peak: 0.55,
        pick_softness: 0.93,
    },
];

/// Fallback topology when a leveling job carries no instrument selection (and no
/// explicit stimulus / env override) — a neutral mid-output humbucker. Guarded by
/// `default_topology_exists` so a catalog rename can't silently break the default.
pub const DEFAULT_TOPOLOGY_ID: &str = "guitar-humbucker";

/// A pickup *alias*: a familiar pickup name musicians look for in the picker,
/// mapped to the shipped topology whose stimulus it uses. Aliases are
/// first-class picker rows (a profile may store an alias id as its
/// `topology_id`), but carry NO synth params or WAV of their own —
/// `canonical_id` resolves them to the parent before any stimulus lookup.
pub struct Alias {
    /// Stable picker/profile id (never a WAV stem — see `canonical_id`).
    pub id: &'static str,
    pub label: &'static str,
    /// The `TOPOLOGIES` entry whose stimulus this alias uses.
    pub topology_id: &'static str,
}

pub const ALIASES: &[Alias] = &[
    // Low-wind, bright Gretsch humbucker — resonance sits at the single-coil template.
    Alias {
        id: "guitar-filtertron",
        label: "Filter'Tron",
        topology_id: "guitar-singlecoil",
    },
    Alias {
        id: "guitar-dynasonic",
        label: "DynaSonic",
        topology_id: "guitar-singlecoil",
    },
    // Fat, hot single-coil — inductance/drive land on the humbucker template.
    Alias {
        id: "guitar-p90",
        label: "P90",
        topology_id: "guitar-humbucker",
    },
    Alias {
        id: "guitar-goldfoil",
        label: "Gold foil",
        topology_id: "guitar-singlecoil",
    },
];

/// Resolve an alias id to its parent topology id; a non-alias id passes through.
/// Every id→WAV/params seam must go through this (an alias id is not a WAV stem).
pub fn canonical_id(id: &str) -> &str {
    ALIASES
        .iter()
        .find(|a| a.id == id)
        .map_or(id, |a| a.topology_id)
}

/// Look up a topology by its stable id (== WAV stem) or an alias id. Used to
/// resolve a saved profile's `topology_id` to its synth/WAV entry.
pub fn by_id(id: &str) -> Option<&'static Topology> {
    let id = canonical_id(id);
    TOPOLOGIES.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_topology_exists() {
        assert!(
            by_id(DEFAULT_TOPOLOGY_ID).is_some(),
            "DEFAULT_TOPOLOGY_ID '{DEFAULT_TOPOLOGY_ID}' is not in the catalog"
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = TOPOLOGIES
            .iter()
            .map(|t| t.id)
            .chain(ALIASES.iter().map(|a| a.id))
            .collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate topology/alias id");
    }

    /// Every alias must point at a real topology, and resolve through `by_id`.
    #[test]
    fn aliases_resolve() {
        for a in ALIASES {
            let topo = by_id(a.id).unwrap_or_else(|| {
                panic!("alias '{}' → unknown topology '{}'", a.id, a.topology_id)
            });
            assert_eq!(topo.id, a.topology_id);
            assert_eq!(canonical_id(a.id), a.topology_id);
        }
        // A non-alias id passes through canonical_id untouched.
        assert_eq!(canonical_id(DEFAULT_TOPOLOGY_ID), DEFAULT_TOPOLOGY_ID);
    }

    /// Long-term band shares (% of power) of a committed stimulus WAV.
    fn band_shares(id: &str) -> [f64; 4] {
        let path = format!("{}/resources/samples/{id}.wav", env!("CARGO_MANIFEST_DIR"));
        let s = crate::stimulus::read_stimulus_48k(&path).expect("read stimulus");
        let p = crate::psd::welch_psd(&s, 48_000.0).band_powers(&[
            (20.0, 320.0),
            (320.0, 1250.0),
            (1250.0, 2500.0),
            (2500.0, 20_000.0),
        ]);
        let tot: f64 = p.iter().sum();
        [0, 1, 2, 3].map(|i| p[i] / tot * 100.0)
    }

    /// The committed stimuli must keep a real pickup's spectral balance: most
    /// power below 1.25 kHz and only a trace above 2.5 kHz. Real humbucker and
    /// single-coil DIs carry ≲1 % above 2.5 kHz; the old band-passed-noise WAVs
    /// carried 50–90 % there, which mis-levelled bright presets on hardware.
    #[test]
    fn every_stimulus_has_a_pickup_like_spectrum() {
        for t in TOPOLOGIES {
            let [low, low_mid, _, high] = band_shares(t.id);
            assert!(high < 3.0, "{}: {high:.2}% above 2.5 kHz", t.id);
            assert!(
                low + low_mid > 85.0,
                "{}: {:.1}% below 1.25 kHz",
                t.id,
                low + low_mid
            );
            let low_range = if t.instrument == "guitar" {
                10.0..50.0
            } else {
                35.0..80.0
            };
            assert!(low_range.contains(&low), "{}: {low:.1}% below 320 Hz", t.id);
        }
    }

    /// Each catalog id must have a committed WAV (run `cargo run --bin gen_samples`
    /// after adding/renaming entries). Guards id↔filename drift.
    #[test]
    fn every_topology_has_a_wav() {
        let dir = format!("{}/resources/samples", env!("CARGO_MANIFEST_DIR"));
        for t in TOPOLOGIES {
            let path = format!("{dir}/{}.wav", t.id);
            assert!(
                std::path::Path::new(&path).exists(),
                "missing stimulus WAV for topology '{}' — run `cargo run --bin gen_samples`: {path}",
                t.id
            );
        }
    }
}
