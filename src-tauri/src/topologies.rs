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
//! `id` doubles as the WAV file stem (`resources/samples/<id>.wav`) and as the
//! stable key a saved instrument profile references — keep them in lockstep
//! (the `every_topology_has_a_wav` test guards drift).

/// One pickup topology: display metadata + the deterministic synth recipe.
#[derive(Debug, Clone, Copy)]
pub struct Topology {
    /// Stable id == WAV file stem == profile reference key.
    pub id: &'static str,
    /// UI display label.
    pub label: &'static str,
    /// "guitar" | "bass" — groups the dropdown.
    pub instrument: &'static str,
    /// Deterministic PRNG seed → reproducible committed WAV.
    pub seed: u64,
    /// Resonant-peak frequency (Hz) of the pickup.
    pub freq: f32,
    /// Resonance Q — high = peaky (passive), low = flat/broad (active/acoustic).
    pub q: f32,
    /// Output level as **input drive**: stimulus peak amplitude (0..1).
    pub peak: f32,
    /// Mix of the resonant bandpass voice (passive pickups ≈ 0.8).
    pub bp_mix: f32,
    /// Mix of the low-passed "body" voice (active/acoustic lean higher → flatter).
    pub body_mix: f32,
    /// Pluck attack ramp (ms) — short = percussive (acoustic), long = round (bass).
    pub attack_ms: f32,
}

/// The shipped catalog — **one stimulus per pickup character**. The output-level
/// axis (low/med/high) was removed once Tier-2 calibration began measuring real
/// output directly: calibration sets *level*, so a topology only needs to set
/// *spectrum*. `peak` is now a single per-character default that drives the amp
/// for UNcalibrated profiles; a calibrated profile overrides it (see
/// `read_stimulus_calibrated`).
pub const TOPOLOGIES: &[Topology] = &[
    // ── Guitar ─────────────────────────────────────────────────────────────
    Topology { id: "guitar-singlecoil", label: "Single-coil", instrument: "guitar",
               seed: 0x1111_0002_0000_0002, freq: 5500.0, q: 2.0, peak: 0.50, bp_mix: 0.80, body_mix: 0.35, attack_ms: 4.0 },
    Topology { id: "guitar-humbucker", label: "Humbucker", instrument: "guitar",
               seed: 0x2222_0002_0000_0002, freq: 3000.0, q: 2.5, peak: 0.55, bp_mix: 0.80, body_mix: 0.40, attack_ms: 4.0 },
    Topology { id: "guitar-active", label: "Active (EMG/Fluence)", instrument: "guitar",
               seed: 0x3333_0001_0000_0001, freq: 3500.0, q: 1.2, peak: 0.85, bp_mix: 0.45, body_mix: 0.60, attack_ms: 3.0 },
    Topology { id: "guitar-acoustic", label: "Acoustic / piezo", instrument: "guitar",
               seed: 0x4444_0001_0000_0001, freq: 4000.0, q: 0.8, peak: 0.60, bp_mix: 0.50, body_mix: 0.50, attack_ms: 1.5 },
    // ── Bass ───────────────────────────────────────────────────────────────
    Topology { id: "bass-singlecoil", label: "Bass single-coil", instrument: "bass",
               seed: 0x5555_0002_0000_0002, freq: 700.0, q: 2.0, peak: 0.55, bp_mix: 0.80, body_mix: 0.40, attack_ms: 5.0 },
    Topology { id: "bass-humbucker", label: "Bass humbucker", instrument: "bass",
               seed: 0x6666_0002_0000_0002, freq: 550.0, q: 2.2, peak: 0.60, bp_mix: 0.80, body_mix: 0.45, attack_ms: 5.0 },
    Topology { id: "bass-active", label: "Bass active", instrument: "bass",
               seed: 0x7777_0001_0000_0001, freq: 800.0, q: 1.0, peak: 0.85, bp_mix: 0.45, body_mix: 0.60, attack_ms: 4.0 },
];

/// Fallback topology when a leveling job carries no instrument selection (and no
/// explicit stimulus / env override) — a neutral mid-output humbucker. Guarded by
/// `default_topology_exists` so a catalog rename can't silently break the default.
pub const DEFAULT_TOPOLOGY_ID: &str = "guitar-humbucker";

/// Look up a topology by its stable id (== WAV stem). Used to resolve a saved
/// profile's `topology_id` to its synth/WAV entry.
pub fn by_id(id: &str) -> Option<&'static Topology> {
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
        let mut ids: Vec<&str> = TOPOLOGIES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate topology id");
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
