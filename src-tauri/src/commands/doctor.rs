//! Doctor (tone diagnosis) commands — capture · diagnose. Doctor never writes to the
//! unit: each fix is a step the player makes by hand (the owner's decision), so there is
//! no apply, save or discard command.
use super::ProgressStatus;
use crate::error::{LevelError, Refusal};
use crate::{
    blockcaps,
    commands::level_preset::resolve_stimulus_with_capture,
    device_gate::with_released_seize,
    doctor, footswitch, leveller, session,
    slot_read::{read_slot_preset_complete, SlotReadError},
    stimulus::read_stimulus_calibrated,
    topologies, AppState, AtomicBool, ListIndex, SeqCst, Serialize, Session, State,
};

// ───────────────────────── Doctor (tone diagnosis) ─────────────────────────

/// One sound to check: a preset's base (`scene: None`) or one scene
/// (`scene: Some(wire index)` — 0-based `scenes[]`, base slot 8 excluded).
/// `nodes` is the preset's chain from the backup scan's graph (may be empty —
/// diagnosis still runs, graph-dependent prescriptions are absent).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorInput {
    pub key: String,
    pub list_index: ListIndex,
    pub scene: Option<u32>,
    /// Block-acting footswitch index (0-based `switch`) when this sound is a
    /// footswitch state; `None` for base/scene sounds.
    pub footswitch: Option<u32>,
    /// The wire label ("Tweed Warm · Lead"): the results carry it back and the frontend
    /// finds each sound by it. It also names the preset, so no sentence on screen uses it.
    pub label: String,
    /// The sound's name as the screen says it in a sentence: a scene's name ("Lead"),
    /// "footswitch 3" for a footswitch, "Main sound" for a main sound. Scene consistency
    /// writes it into its steps. Empty (a caller that sends none) falls back to `label`.
    #[serde(default)]
    pub sound_name: String,
    pub tag: Option<String>,
    pub topology_id: Option<String>,
    pub calibration_lufs: Option<f32>,
    /// Instrument profile id: when it has a stored Tier-2 DI capture, that WAV is
    /// the stimulus (read VERBATIM, calibration scaling off) and the sound is
    /// diagnosed as `StimulusKind::Capture` (one shared threshold table; the
    /// kind only gates fizzy flatness). `None`/capture-less → the synthetic
    /// topology sample (the pinned default).
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<doctor::DoctorNode>,
    /// The preset's block-acting footswitches, from the startup backup scan
    /// (`BackupPresetRow.footswitches`) — drives OFFLINE force-bypass isolation
    /// derivation ([`footswitch::derived_force_bypass`]) so base/footswitch sounds skip the
    /// live ~1.9 s field-8 isolation read whenever the frontend has this (which
    /// it always does once the backup scan has reached the preset). Empty when
    /// absent (pre-scan, or a preset the scan couldn't parse).
    #[serde(default)]
    pub footswitches: Vec<footswitch::FootswitchInfo>,
    /// A SCENE sound's saved overlay relative to `nodes` (`BackupPresetRow.scene_overrides`
    /// at this sound's scene index) — what the scene recall puts on the unit, so the
    /// diagnosis reads the graph that was captured. Empty for base/footswitch sounds and
    /// for a scene whose overlay the scan could not read (diagnosed on base, as before).
    #[serde(default)]
    pub scene_overrides: Vec<doctor::NodeOverride>,
}

/// `(group_id, node_id, bypass_to_write)` — the force-bypass isolation list
/// [`doctor_force_bypass`] and [`base_isolation_or_refuse`] share. Named purely to keep
/// their signatures under clippy's `type_complexity` threshold.
pub(crate) type ForceBypass = Vec<(String, String, bool)>;

impl DoctorInput {
    /// [`Self::sound_name`], or the label when the caller sent none.
    fn screen_name(&self) -> &str {
        if self.sound_name.is_empty() {
            &self.label
        } else {
            &self.sound_name
        }
    }
}

/// The force-bypass isolation list for capturing one sound cleanly (mirrors the
/// leveller's base/footswitch isolation, `footswitch.rs`): Base forces EVERY
/// footswitch on/off block OFF; a footswitch forces its OWN blocks into their
/// switch-active state (isActive-aware — a plain flip of the saved bypass inverts
/// on a preset saved with the switch engaged, HW preset 024's BD2) while forcing
/// every other switch's block off; a scene contributes NOTHING (its own bypass
/// overrides define it). `(group_id, node_id, bypass_to_write)`.
pub(crate) fn doctor_force_bypass(
    ftsw: &serde_json::Value,
    preset: &serde_json::Value,
    footswitch: Option<u32>,
) -> ForceBypass {
    match footswitch {
        Some(s) => {
            let mut out = footswitch::siblings_off_excluding(ftsw, s);
            out.extend(footswitch::engaged_bypass_for_switch(ftsw, preset, s));
            out
        }
        None => footswitch::all_onoff_blocks(ftsw)
            .into_iter()
            .map(|(g, n)| (g, n, true))
            .collect(),
    }
}

/// The shared "read → refuse-or-force" step for a BASE leveling run's isolation: given the
/// OUTCOME of a `read_slot_preset_complete(slot, &["ftsw"])`-shaped read (`Ok(preset)` or the
/// read's own typed `Err`), either derive the base force-bypass list (`doctor_force_bypass`'s
/// `None` arm) plus the preset's `restore_scene`, or refuse with the SAME text production's
/// base arm has always shown. ONE definition of "base isolation" for every arm
/// that levels base — the paths disagreeing about what "base" means cost a ~16 LU divergence
/// (2026-08-31 bisect: an empty `&[]` force list solved C=-11.95 where production's isolated
/// base measured C≈-28.2).
///
/// The refusal's kind is the read's CAUSE ([`SlotReadError::skip_kind`]): a cut or missing
/// `ftsw` is [`crate::error::SkipKind::FootswitchesUnreadable`], and a read that failed, came
/// back empty or did not parse is [`crate::error::SkipKind::PresetUnreadable`], as the scene
/// lane reports the same read.
///
/// Pure over the read's `Result` rather than doing the read itself, so the refusal is
/// testable at all: `SimDevice` always serves whole bodies, so a truncated field-8 read can
/// never be driven from an offline fixture — a test hands this fn a synthetic `Err` instead.
pub(crate) fn base_isolation_or_refuse(
    read: Result<serde_json::Value, SlotReadError>,
    slot: ListIndex,
) -> Result<(serde_json::Value, ForceBypass, Option<u32>), Refusal> {
    let preset = read.map_err(|e| {
        Refusal::new(
            e.skip_kind(),
            format!(
                "could not read preset {}'s footswitch assignments ({e}) — they define which \
                 blocks are switched off for the Base measurement, and leveling without them \
                 would save a level solved for the wrong sound",
                slot.device_slot()
            ),
        )
    })?;
    let force = doctor_force_bypass(&preset["ftsw"], &preset, None);
    let restore_scene = crate::scene_jobs::last_loaded_scene(&preset);
    Ok((preset, force, restore_scene))
}

/// [`base_isolation_or_refuse`] with the read it needs — the whole "isolate base before
/// levelling" step, for every base leveling arm (production's `commands::level_preset` and both
/// probe arms). Returns the preset body, the read's own fail-safe `has_fs_scenes` flag
/// (`read_slot_preset_sections`' doc), the force-bypass list and `restore_scene`.
pub(crate) fn read_base_isolation(
    slot: ListIndex,
) -> Result<(serde_json::Value, bool, ForceBypass, Option<u32>), Refusal> {
    let read = crate::slot_read::read_slot_preset_complete(slot, &["ftsw"]);
    let has_fs_scenes = read
        .as_ref()
        .is_ok_and(|(_, has_fs_scenes, _)| *has_fs_scenes);
    let (preset, force, restore_scene) =
        base_isolation_or_refuse(read.map(|(preset, _, _)| preset), slot)?;
    Ok((preset, has_fs_scenes, force, restore_scene))
}

/// `DoctorNode`s → a `node_id → saved bypass` map, first-occurrence-wins
/// (mirrors the pre-extraction `nodes.iter().find(...)` semantics) — the shape
/// [`footswitch::derived_force_bypass`] needs, decoupled from the Doctor's own
/// node type.
fn saved_bypass_map(nodes: &[doctor::DoctorNode]) -> std::collections::HashMap<String, bool> {
    let mut map = std::collections::HashMap::new();
    for n in nodes {
        map.entry(n.node_id.clone()).or_insert(n.bypassed);
    }
    map
}

/// What a diagnosed sound's capture must WRITE before engaging — one policy for
/// every Doctor capture of that sound, so no capture can observe a different
/// state than the diagnosis. `bypass` is the force-bypass isolation; `params` is
/// the measured footswitch's `param`-function engaged `valueA` writes (a footswitch
/// SOUND is its on-off flips PLUS its param jumps — without the latter a param-only
/// switch would capture the base sound).
pub(crate) struct SoundIsolation {
    pub(crate) bypass: Vec<(String, String, bool)>,
    pub(crate) params: Vec<(String, String, String, f32)>,
    /// Set when this sound's state could not be resolved at all — the caller must
    /// report the sound as errored instead of capturing it. A FOOTSWITCH sound IS its
    /// isolation (`ftsw` decides which blocks are on and which params jump), so an
    /// unresolvable one captures the base sound and diagnoses the wrong thing under the
    /// switch's name. Base sounds stay best-effort: their isolation only silences other
    /// switches, so a missing one degrades the baseline rather than misnaming it.
    pub(crate) unresolved: Option<String>,
    /// The graph this sound is captured in (`doctor::effective_nodes`): the saved base
    /// graph plus these writes, or plus the scene's saved overlay for a scene sound. The
    /// capture tail, the diagnosis and the prescriptions all read it, never the base graph.
    pub(crate) graph: Vec<doctor::DoctorNode>,
}

impl SoundIsolation {
    /// These writes as graph overrides — built from the SAME lists the capture sends, so
    /// the diagnosed graph can never drift from the written one.
    fn overrides(&self) -> Vec<doctor::NodeOverride> {
        let bypass = self.bypass.iter().map(|(g, n, b)| doctor::NodeOverride {
            group_id: g.clone(),
            node_id: n.clone(),
            bypassed: Some(*b),
            params: std::collections::HashMap::new(),
        });
        let params = self.params.iter().map(|(g, n, p, v)| doctor::NodeOverride {
            group_id: g.clone(),
            node_id: n.clone(),
            bypassed: None,
            params: std::collections::HashMap::from([(p.clone(), f64::from(*v))]),
        });
        bypass.chain(params).collect()
    }
}

/// Resolve the pre-engage writes for a diagnosed sound — one policy for every Doctor
/// capture of that sound (the check and the probe arms that replay it), so none can
/// observe a different bypass/param state than the diagnosis. A SCENE sound always gets
/// NO write, graph present or not: it rides its own saved overlay/bypass state
/// (as-played — the scene recall itself asserts whatever the player hears), so forcing
/// every footswitch block off here would diagnose a baseline the player never actually
/// hears. Base/footswitch sounds keep the baseline isolation: graph present → offline
/// derivation (`derived_force_bypass` + `derived_param_writes`); graph absent → ONE
/// cached live field-8 read per preset (`doctor_force_bypass` + `param_fn_values`). The
/// sound's captured graph ([`SoundIsolation::graph`]) is the base `nodes` plus those
/// writes — or, for a scene sound, plus `scene_overrides` (its saved overlay, which its
/// recall puts on the unit).
fn resolve_sound_isolation(
    nodes: &[doctor::DoctorNode],
    footswitches: &[footswitch::FootswitchInfo],
    scene_overrides: &[doctor::NodeOverride],
    scene: Option<u32>,
    footswitch: Option<u32>,
    list_index: ListIndex,
    preset_cache: &mut std::collections::HashMap<ListIndex, serde_json::Value>,
) -> SoundIsolation {
    if scene.is_some() {
        return SoundIsolation {
            bypass: Vec::new(),
            params: Vec::new(),
            unresolved: None,
            graph: doctor::effective_nodes(nodes, scene_overrides),
        };
    }
    let mut iso = isolation_writes(nodes, footswitches, footswitch, list_index, preset_cache);
    iso.graph = doctor::effective_nodes(nodes, &iso.overrides());
    iso
}

/// [`resolve_sound_isolation`]'s base/footswitch writes (its `graph` left empty).
fn isolation_writes(
    nodes: &[doctor::DoctorNode],
    footswitches: &[footswitch::FootswitchInfo],
    footswitch: Option<u32>,
    list_index: ListIndex,
    preset_cache: &mut std::collections::HashMap<ListIndex, serde_json::Value>,
) -> SoundIsolation {
    if !nodes.is_empty() {
        SoundIsolation {
            bypass: footswitch::derived_force_bypass(
                footswitches,
                &saved_bypass_map(nodes),
                footswitch,
            ),
            params: footswitch::derived_param_writes(footswitches, footswitch),
            unresolved: None,
            graph: Vec::new(),
        }
    } else {
        // Base/footswitch sounds fall back to the legacy live field-8 read
        // (cached per list index across that preset's base + footswitch
        // sounds) only when the backup scan missed this preset's graph (rare
        // — its presetJson failed to parse). A read hiccup on that fallback
        // degrades to no isolation (best-effort, like `level_preset`), never
        // fails the run.
        if let std::collections::hash_map::Entry::Vacant(e) = preset_cache.entry(list_index) {
            // `ftsw` is required of this read: a large preset's field-8 body is cut
            // before it, and a partial one derives a partial isolation — which for a
            // footswitch sound means capturing something the switch does not make.
            let preset = match read_slot_preset_complete(list_index, &["ftsw"]) {
                Ok((p, _, _)) => p,
                Err(err) => {
                    log::warn!(
                        "doctor_check slot={list_index}: isolation preset read failed ({err}), capturing without isolation"
                    );
                    serde_json::Value::Null
                }
            };
            e.insert(preset);
        }
        let preset = &preset_cache[&list_index];
        let ftsw = preset.get("ftsw").unwrap_or(&serde_json::Value::Null);
        SoundIsolation {
            bypass: doctor_force_bypass(ftsw, preset, footswitch),
            params: footswitch
                .map(|sw| {
                    footswitch::param_fn_values(ftsw, sw)
                        .into_iter()
                        .map(|(g, n, p, a, _b)| (g, n, p, a))
                        .collect()
                })
                .unwrap_or_default(),
            // A footswitch SOUND is defined by `ftsw` — no readable `ftsw` means the
            // capture would engage nothing and diagnose the base sound under the
            // switch's name. Report the sound instead of inventing it. A base sound
            // keeps the documented best-effort degrade.
            unresolved: footswitch.filter(|_| ftsw.as_array().is_none()).map(|sw| {
                format!(
                    "footswitch {} on preset {}: the saved footswitch assignments could not \
                     be read (the preset is too large to read over USB), so this switch's \
                     sound cannot be isolated — skipped",
                    sw + 1,
                    list_index.device_slot()
                )
            }),
            graph: Vec::new(),
        }
    }
}

/// The previous sound's run outcome, for the consecutive-scene load skip.
/// Only a SUCCESSFUL sound is recorded — after an error the unit may be on any
/// preset, so the loop resets this to `None` (same effect as the run's start).
struct PrevSound {
    list_index: ListIndex,
    /// It sent working-copy writes — force-bypass isolation OR a footswitch's
    /// param-function `valueA` jumps (`SoundIsolation`, either list non-empty).
    /// Only a reload clears the pollution (a scene recall re-asserts ONLY the
    /// scene's own overrides, not foreign bypasses or param values).
    wrote: bool,
}

/// Whether the current sound can skip the per-sound preset load connection: the
/// previous sound already loaded the SAME preset, made no working-copy writes, and
/// succeeded — and the current sound is a SCENE (its capture connection recalls the
/// scene anyway, the production scene-leveling mechanism; HW-validated 0.001 LU
/// identical to a fresh reload). Base and footswitch sounds always reload (base
/// needs the base-scene state; footswitch sounds write isolation either way).
fn doctor_skip_load(prev: Option<&PrevSound>, list_index: ListIndex, is_scene: bool) -> bool {
    is_scene
        && prev
            .map(|p| p.list_index == list_index && !p.wrote)
            .unwrap_or(false)
}

/// Why a just-captured Doctor sound can't be diagnosed as captured (#246). Each one
/// is retried once with a fresh preset recall, then surfaced as an error — never a
/// diagnosis of something that isn't the preset's sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSuspect {
    /// The capture reads as the device's stationary output floor — the leveller's
    /// `leveller::floor_suspect` guard on MOMENTARY spreads.
    Floor,
    /// Neither the energy step nor the correlator found the stimulus
    /// (`leveller::OnsetSource::UnAligned`). A stationary floor has no energy step,
    /// so this is the path a silent inject takes; diagnosing it would judge the
    /// device's idle noise over an un-aligned split.
    NoOnset,
    /// The capture ends at or before the stimulus body does, so there is no tail
    /// window — `doctor::tail_energy_ratio` would read its −80 dB "bone dry"
    /// sentinel and silently drop a wet preset's `washed` verdict.
    NoTail,
}

impl CaptureSuspect {
    /// The per-sound error a suspect that survives the retry becomes.
    fn into_error(self) -> LevelError {
        match self {
            // A missing onset is a silent inject seen from the other side: the
            // stimulus never showed up in the capture.
            CaptureSuspect::Floor | CaptureSuspect::NoOnset => LevelError::FloorRead,
            CaptureSuspect::NoTail => LevelError::Device(TRUNCATED_CAPTURE_ERR.to_string()),
        }
    }
}

/// The per-sound error when a capture still ends before the stimulus does after the
/// retry ([`CaptureSuspect::NoTail`]).
const TRUNCATED_CAPTURE_ERR: &str = "the capture stopped before the sound finished, so \
    its tail can't be judged — check the USB audio connection and try again";

/// Should a just-captured sound be refused rather than diagnosed? `onset_confident` is
/// `leveller::DoctorOnset::confident`, `has_tail` whether the capture runs past the
/// body's end, and both spreads are MOMENTARY spreads (the Doctor's 3 s stimulus is
/// exactly one short-term window, so its short-term spread reads ≈ 0 on a live capture
/// and a floor alike). `None` means the capture looks live and complete. The floor
/// check runs first: a floor read usually also has no onset, and its error names the
/// cause.
fn capture_suspect(
    onset_confident: bool,
    has_tail: bool,
    profile_momentary_lu: f64,
    stimulus_momentary_lu: f64,
) -> Option<CaptureSuspect> {
    if leveller::floor_suspect(profile_momentary_lu, stimulus_momentary_lu) {
        Some(CaptureSuspect::Floor)
    } else if !onset_confident {
        Some(CaptureSuspect::NoOnset)
    } else if !has_tail {
        Some(CaptureSuspect::NoTail)
    } else {
        None
    }
}

/// How many bands a coverage vector (`doctor::output_coverage_with_body`'s
/// output) marks NOT covered — the count `sound_result` stamps onto
/// `DoctorSoundResult.skippedBandCount` (D4, SNR-gate transparency; `> 0` IS
/// "gated", no separate bool). `None` (no coverage computed at all — an
/// errored or showcase sound) reads as 0, same as an all-covered vector: this
/// can under-report "gated" in that case, never over-report. Split out for a
/// unit test with no capture data (mirrors [`capture_suspect`]'s split).
fn skipped_band_count(coverage: Option<&[bool]>) -> u32 {
    coverage
        .map(|c| u32::try_from(c.iter().filter(|covered| !**covered).count()).unwrap_or(0))
        .unwrap_or(0)
}

/// The instrument a sound is judged as — from its topology, guitar by default.
fn instrument_of(item: &DoctorInput) -> doctor::Instrument {
    instrument_of_topology(item.topology_id.as_deref())
}

/// The instrument a topology id is judged as — guitar when unknown or absent.
fn instrument_of_topology(topology_id: Option<&str>) -> doctor::Instrument {
    doctor::Instrument::from_topology(
        topology_id
            .and_then(topologies::by_id)
            .map(|t| t.instrument)
            .unwrap_or("guitar"),
    )
}

/// Streamed per-sound progress row (`active` → `done`/`error`). Diagnoses ride
/// the command's RETURN value, not this channel — the structured per-preset
/// results (incl. scene consistency) are assembled once every sound is measured.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorProgressItem {
    pub key: String,
    pub status: ProgressStatus,
    pub message: Option<String>,
}

/// A sound's run outcome on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SoundStatus {
    Measured,
    Error,
    /// The run was stopped before this sound was captured.
    Unrun,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSoundResult {
    pub key: String,
    pub list_index: ListIndex,
    pub scene: Option<u32>,
    pub footswitch: Option<u32>,
    pub label: String,
    pub tag: Option<String>,
    /// Findings for this sound, each tagged with the quietest playback level it
    /// fires at (`doctor::diagnose_levels` — one capture, diagnosed at all three
    /// levels). `fromLevel: "quiet"` = a problem at any volume; `rehearsal`/
    /// `stage` = only appears at that volume and louder.
    pub diags: Vec<doctor::LeveledDiag>,
    /// Whether this sound was captured, failed, or never reached (a Stop).
    pub status: SoundStatus,
    /// `None` unless `status` is `measured` — an unrun or failed sound has no loudness,
    /// and a placeholder number would enter scene consistency as a real reading.
    pub integrated_lufs: Option<f64>,
    pub tail_ratio_db: Option<f64>,
    pub balance_db: Vec<f64>,
    /// The display labels of THIS sound's family band layout, in lockstep with
    /// `balance_db` and the `Diag.bands` indices (6 for guitar/bass, 7 for
    /// Bass VI's Sub-first layout). The frontend renders bars/labels from this.
    pub band_labels: Vec<String>,
    /// The "does this cut through the mix?" ESTIMATE (`doctor::cut_through`) —
    /// a presence-contrast reading vs the measured factory-bank distribution,
    /// not a diagnosis: it fires no rule and carries no `Rx`. `None` when this
    /// sound's capture failed or the ratio was degenerate.
    pub cut_through: Option<doctor::CutThrough>,
    /// Set when this sound's capture failed (no diags then); the run continues. For the
    /// log and the bug report: the screen words the failure by [`Self::error_kind`].
    pub error: Option<String>,
    /// Why this sound's capture failed (`LevelError::skip_kind`), set exactly when
    /// `error` is — the typed kind the frontend picks its message by (a silent capture,
    /// `no_signal`, is refined by the preset's backup-scan silence hint).
    pub error_kind: Option<crate::error::SkipKind>,
    /// How many of this sound's bands the SNR gate (`doctor::output_coverage_with_body`)
    /// dropped as too quiet to trust against its own noise floor — some rules were
    /// silently skipped for lack of signal, not because the tone measured clean.
    /// 0 on the unconfident-onset fallback too (every band reads "covered" there,
    /// see that fn's doc) and on an errored/showcase sound (no coverage computed
    /// at all) — this can under-report in those cases, never over-report. No
    /// separate gated/not-gated bool rides alongside this — `> 0` IS "gated".
    pub skipped_band_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorPresetResult {
    pub list_index: ListIndex,
    pub sounds: Vec<DoctorSoundResult>,
    pub scene_consistency: Option<doctor::SceneConsistency>,
    /// Backup-scan-only advisories (zero device captures): footswitch `param`
    /// assignments whose shape matches the pre-`param_class` leveler's damage
    /// signatures. Computed from the SAME `footswitches` `doctor_check` was
    /// given, independent of whether any sound's capture succeeded.
    pub leveling_damage: Vec<doctor::LevelingDamageHint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheckResult {
    pub presets: Vec<DoctorPresetResult>,
    pub stopped: bool,
}

/// One sound's run outcome, before diagnosis. `nodes` is the graph the sound was
/// captured in ([`SoundIsolation::graph`]) — what its diagnosis must read.
#[derive(Clone)]
enum SoundOutcome {
    Unrun,
    /// The capture failed: its text (for the log) and its typed kind.
    Failed(String, crate::error::SkipKind),
    Measured {
        profile: doctor::SoundProfile,
        /// The captured output's band coverage; `None` on the showcase's curated profiles.
        coverage: Option<Vec<bool>>,
        nodes: Vec<doctor::DoctorNode>,
    },
}

/// Diagnose one sound on its OWN measurements (the deterministic target-deviation
/// metric) — no run-cohort, so a verdict never depends on which other sounds ran.
fn sound_result(
    item: &DoctorInput,
    kind: doctor::StimulusKind,
    outcome: &SoundOutcome,
) -> DoctorSoundResult {
    let instrument = instrument_of(item);
    let mut r = DoctorSoundResult {
        key: item.key.clone(),
        list_index: item.list_index,
        scene: item.scene,
        footswitch: item.footswitch,
        label: item.label.clone(),
        tag: item.tag.clone(),
        diags: Vec::new(),
        status: SoundStatus::Unrun,
        integrated_lufs: None,
        tail_ratio_db: None,
        balance_db: Vec::new(),
        band_labels: instrument.labels_owned(),
        cut_through: None,
        error: None,
        error_kind: None,
        skipped_band_count: 0,
    };
    match outcome {
        SoundOutcome::Unrun => {}
        SoundOutcome::Failed(e, kind) => {
            r.status = SoundStatus::Error;
            r.error = Some(e.clone());
            r.error_kind = Some(*kind);
        }
        SoundOutcome::Measured {
            profile: p,
            coverage,
            nodes,
        } => {
            r.status = SoundStatus::Measured;
            // Diagnosed at ALL three playback levels (each finding tagged with its
            // quietest firing level) — the capture is level-independent, so this is
            // three pure passes over one profile.
            r.diags = doctor::diagnose_levels(
                p,
                (!nodes.is_empty()).then_some(nodes.as_slice()),
                instrument,
                kind,
                coverage.as_deref(),
            );
            log_findings(item, &r.diags);
            r.integrated_lufs = Some(p.integrated_lufs);
            r.tail_ratio_db = Some(p.tail_ratio_db);
            r.balance_db = doctor::balance(&p.bands);
            r.cut_through = doctor::cut_through(p, instrument);
            // SNR-gate transparency (D4) — see `skipped_band_count`'s doc.
            r.skipped_band_count = skipped_band_count(coverage.as_deref());
        }
    }
    r
}

/// Log each finding's technical reading (`Diag::detail`, `Rx::detail`): the
/// Hz/dB behind the plain words, which the screen doesn't show.
fn log_findings(item: &DoctorInput, diags: &[doctor::LeveledDiag]) {
    for d in diags {
        let fixes: Vec<&str> = d
            .diag
            .rx
            .iter()
            .map(|r| r.detail.as_str())
            .filter(|t| !t.is_empty())
            .collect();
        log::info!(
            "doctor_check {}: {} (z {:.1}, from {:?}): {}; fix settings: [{}]",
            item.key,
            d.diag.key,
            d.diag.z,
            d.from_level,
            d.diag.detail,
            fixes.join(" | ")
        );
    }
}

/// Group the run's sounds per preset in first-seen item order, then add each preset's
/// backup-scan advisories and its scene consistency. Only MEASURED sounds enter
/// consistency — an unrun or failed sound has no loudness to compare.
fn assemble_results(
    resolved: &[(DoctorInput, String, doctor::StimulusKind)],
    outcomes: &[SoundOutcome],
) -> Vec<DoctorPresetResult> {
    let mut presets: Vec<DoctorPresetResult> = Vec::new();
    // Each preset's first measured base sound — scene consistency's reference.
    let mut bases: std::collections::HashMap<ListIndex, (String, f64, doctor::Instrument)> =
        std::collections::HashMap::new();
    for ((item, _, kind), outcome) in resolved.iter().zip(outcomes) {
        let sound = sound_result(item, *kind, outcome);
        if let (None, None, Some(lufs)) = (sound.scene, sound.footswitch, sound.integrated_lufs) {
            bases
                .entry(item.list_index)
                .or_insert_with(|| (sound.label.clone(), lufs, instrument_of(item)));
        }
        match presets.iter_mut().find(|p| p.list_index == item.list_index) {
            Some(p) => p.sounds.push(sound),
            None => presets.push(DoctorPresetResult {
                list_index: item.list_index,
                sounds: vec![sound],
                scene_consistency: None,
                // Leveling-damage advisories (fix P3-5): pure backup-scan data, so this
                // runs regardless of whether any sound's capture succeeded. Every item of
                // a preset carries the SAME `footswitches`, so its first item's serve.
                leveling_damage: doctor::leveling_damage_hints(&item.footswitches),
            }),
        }
    }
    // Each sound's on-screen name, for the steps scene consistency writes.
    let screen_names: std::collections::HashMap<&str, &str> = resolved
        .iter()
        .map(|(item, _, _)| (item.key.as_str(), item.screen_name()))
        .collect();
    for p in &mut presets {
        let Some((label, lufs, instrument)) = bases.get(&p.list_index) else {
            continue;
        };
        // Scene sounds carry Some(wire index); footswitch sounds carry None — both are
        // stomp destinations. Only a MEASURED sound has a loudness, so the `?` admits
        // exactly those.
        let others: Vec<doctor::OtherSound> = p
            .sounds
            .iter()
            .filter(|s| s.scene.is_some() || s.footswitch.is_some())
            .filter_map(|s| {
                Some(doctor::OtherSound {
                    label: s.label.clone(),
                    name: screen_names
                        .get(s.key.as_str())
                        .map_or_else(|| s.label.clone(), |n| (*n).to_string()),
                    tag: s.tag.clone(),
                    lufs: s.integrated_lufs?,
                    scene: s.scene,
                })
            })
            .collect();
        p.scene_consistency = doctor::scene_consistency(label, *lufs, &others, *instrument);
    }
    presets
}

/// Cooperative cancel for [`doctor_check`] — stops before the next sound;
/// already-measured sounds keep their results (they're diagnosed and returned).
static DOCTOR_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_doctor_check() {
    DOCTOR_CANCEL.store(true, SeqCst);
    // Also wake the in-flight capture/settle waits (see `device_gate::OP_ABORT`).
    crate::device_gate::request_op_abort();
}

/// The Doctor RUN: capture every selected sound (Doctor tail), then diagnose
/// each sound on its OWN measurements (the deterministic target-deviation metric)
/// and derive per-preset scene consistency from the same captures. READ-ONLY on
/// the unit: loads + captures, never a save; every capture ends re-amp OFF.
/// One command per run (the `copy_apply`/`level_scenes_apply_batched` shape):
/// per-sound progress streams over `on_result`, structured results return.
/// `restore_list_index` is the pre-run ACTIVE preset (the frontend's
/// `activeListIndex`): the run ends by reloading it — falling back to the
/// last-scanned slot — so the unit is back where the player left it and the
/// 0.5 reference `presetLevel` never lingers in the edit buffer.
#[tauri::command]
pub(crate) async fn doctor_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    items: Vec<DoctorInput>,
    restore_list_index: Option<ListIndex>,
    on_result: tauri::ipc::Channel<DoctorProgressItem>,
) -> Result<DoctorCheckResult, String> {
    if items.is_empty() {
        return Err("no sounds selected".to_string());
    }
    DOCTOR_CANCEL.store(false, SeqCst);
    // Resolve stimulus paths up front (needs the app handle; the closure below
    // runs on the blocking pool). `from_capture` → `StimulusKind::Capture`:
    // verbatim injection (no calibration scaling, below) + the capture-only
    // fizzy flatness gate; the threshold table is shared either way.
    let resolved: Vec<(DoctorInput, String, doctor::StimulusKind)> = items
        .into_iter()
        .map(|it| {
            let (path, from_capture) = resolve_stimulus_with_capture(
                &app,
                None,
                it.topology_id.clone(),
                it.profile_id.as_deref(),
            )?;
            let kind = if from_capture {
                doctor::StimulusKind::Capture
            } else {
                doctor::StimulusKind::Synthetic
            };
            Ok((it, path, kind))
        })
        .collect::<Result<_, String>>()?;
    with_released_seize(state.session.clone(), move || {
        run_doctor_check(&resolved, restore_list_index, &mut |item| {
            let _ = on_result.send(item);
        })
    })
    .await
}

/// The device-thread body of [`doctor_check`]: every sound in `resolved` order, then the
/// run-end re-amp OFF and the active-preset restore. `progress` streams each sound's item.
fn run_doctor_check(
    resolved: &[(DoctorInput, String, doctor::StimulusKind)],
    restore_list_index: Option<ListIndex>,
    progress: &mut dyn FnMut(DoctorProgressItem),
) -> Result<DoctorCheckResult, String> {
    let mut run = DoctorRun {
        stims: std::collections::HashMap::new(),
        preset_cache: std::collections::HashMap::new(),
        prev: None,
    };
    // One outcome per item, `Unrun` until the loop reaches it — a Stop leaves the
    // rest `Unrun`, never a measured-looking placeholder.
    let mut outcomes: Vec<SoundOutcome> = vec![SoundOutcome::Unrun; resolved.len()];
    let mut stopped = false;
    let mut last_scanned: Option<ListIndex> = None;
    for (i, (item, path, kind)) in resolved.iter().enumerate() {
        if DOCTOR_CANCEL.load(SeqCst) {
            stopped = true;
            break;
        }
        // Marketing-screenshot showcase: the offline fake re-amp is a stimulus
        // passthrough (every sound measures identically, the same flat-chain cards
        // on each, #247), so inject
        // curated profiles instead and let the real `diagnose` engine render cards.
        // Skips the device reads + reconnect sleeps for this item entirely.
        #[cfg(feature = "e2e")]
        if crate::e2e_showcase() {
            outcomes[i] = run.showcase_sound(item);
            progress(DoctorProgressItem {
                key: item.key.clone(),
                status: ProgressStatus::Done,
                message: None,
            });
            continue;
        }
        last_scanned = Some(item.list_index);
        progress(DoctorProgressItem {
            key: item.key.clone(),
            status: ProgressStatus::Active,
            message: None,
        });
        match run.measure_sound(item, path, *kind) {
            Ok(outcome) => {
                outcomes[i] = outcome;
                progress(DoctorProgressItem {
                    key: item.key.clone(),
                    status: ProgressStatus::Done,
                    message: None,
                });
            }
            // A Stop that landed mid-capture surfaces as a cancel, not a failure: end the
            // run like the top-of-loop check would have, leaving this sound un-run rather
            // than reporting it as an errored sound.
            Err(e) if e.is_cancelled() => {
                stopped = true;
                break;
            }
            Err(e) => {
                let kind = e.skip_kind();
                let e = e.to_string();
                // The screen words the failure by its kind; its text lives here, for the log
                // and the bug report.
                log::warn!(
                    "doctor_check {}: couldn't check this sound ({}): {e}",
                    item.key,
                    kind.as_str()
                );
                outcomes[i] = SoundOutcome::Failed(e.clone(), kind);
                progress(DoctorProgressItem {
                    key: item.key.clone(),
                    status: ProgressStatus::Error,
                    message: Some(e),
                });
            }
        }
        // A Stop landing after the last item's own success, with no further loop-header
        // check ahead of it, would otherwise finish the run silently instead of
        // reporting it stopped.
        if crate::device_gate::op_aborted() {
            stopped = true;
            break;
        }
    }

    let presets = assemble_results(resolved, &outcomes);
    // Never leave the unit in re-amp after a run: the verified run-end OFF ladder, on
    // its own fresh connection (#261). Then put it back on the pre-run active preset
    // (fallback: the last-scanned slot) — the reload also clears the 0.5 reference
    // presetLevel from the edit buffer.
    let _ = leveller::reamp_off_guaranteed("doctor_check");
    if let Some(slot) = restore_list_index.or(last_scanned) {
        if let Err(e) = Session::connect_lean().and_then(|mut s| s.load_preset(slot)) {
            log::warn!("doctor_check: failed to restore active preset after run: {e}");
        }
    }
    Ok(DoctorCheckResult { presets, stopped })
}

/// A decoded Doctor stimulus, sliced to the Doctor window, with what every capture of
/// it reads: its own momentary spread (the floor guard's reference) and its Welch PSD
/// at the device clock (the transfer reference for the localized rules and the
/// stimulus-transfer anchor). Both are pure functions of the samples, so one
/// computation per (path, calibration) cache entry serves every sound and every
/// floor-guard retry (#197).
struct DoctorStim {
    samples: Vec<f32>,
    spread: f64,
    /// `leveller::stim_body_psd` at [`leveller::RATE`]; a capture at another rate
    /// recomputes its own (see [`DoctorStim::psd_at`]).
    psd: crate::psd::Psd,
}

impl DoctorStim {
    fn new(samples: Vec<f32>) -> Self {
        let spread = leveller::stimulus_momentary_spread(&samples);
        let psd = leveller::stim_body_psd(&samples, leveller::RATE);
        DoctorStim {
            samples,
            spread,
            psd,
        }
    }

    /// The stimulus PSD on `rate`'s frequency grid: the cached one at the device
    /// clock, else a fresh computation (the cache key carries no rate, so a
    /// differently clocked capture must never be handed the 48 kHz grid).
    fn psd_at(&self, rate: u32) -> std::borrow::Cow<'_, crate::psd::Psd> {
        if rate == leveller::RATE {
            std::borrow::Cow::Borrowed(&self.psd)
        } else {
            std::borrow::Cow::Owned(leveller::stim_body_psd(&self.samples, rate))
        }
    }
}

/// The state one [`doctor_check`] run carries from sound to sound.
struct DoctorRun {
    /// One stimulus decode per (path, calibration) pair — items share them, along
    /// with the decoded stimulus's OWN dynamics spread (arms the floor guard below;
    /// computed once here rather than per capture).
    stims: std::collections::HashMap<(String, Option<u32>), DoctorStim>,
    /// One field-8 preset read per list index, reused across that preset's base
    /// + footswitch sounds — the source for each sound's force-bypass isolation.
    preset_cache: std::collections::HashMap<ListIndex, serde_json::Value>,
    /// The previous sound's outcome — drives the consecutive-scene load skip
    /// (`doctor_skip_load`).
    prev: Option<PrevSound>,
}

impl DoctorRun {
    /// The showcase stand-in for one sound: a curated profile, no device work.
    #[cfg(feature = "e2e")]
    fn showcase_sound(&mut self, item: &DoctorInput) -> SoundOutcome {
        // The captured graph is derived offline when the scan has the preset's
        // graph; an empty one would need the live fallback read, so it stays empty
        // (diagnosed as an unknown graph, as production does).
        let nodes = if item.nodes.is_empty() {
            Vec::new()
        } else {
            resolve_sound_isolation(
                &item.nodes,
                &item.footswitches,
                &item.scene_overrides,
                item.scene,
                item.footswitch,
                item.list_index,
                &mut self.preset_cache,
            )
            .graph
        };
        // No device load happened — the skip decision must not chain off this.
        self.prev = None;
        SoundOutcome::Measured {
            profile: doctor::showcase_profile(item.list_index),
            coverage: None,
            nodes,
        }
    }

    /// The decoded stimulus for `path` at calibration `cal`, from the run's cache.
    fn stimulus<'a>(
        stims: &'a mut std::collections::HashMap<(String, Option<u32>), DoctorStim>,
        path: &str,
        cal: Option<f32>,
    ) -> Result<&'a DoctorStim, String> {
        let stim_key = (path.to_string(), cal.map(f32::to_bits));
        match stims.entry(stim_key) {
            std::collections::hash_map::Entry::Occupied(e) => Ok(&*e.into_mut()),
            std::collections::hash_map::Entry::Vacant(e) => {
                // Sliced to the Doctor window ONCE at cache fill so the
                // capture, onset alignment, floor-guard spread, and
                // `stimulus_samples` all see the same 3 s stimulus.
                read_stimulus_calibrated(path, cal)
                    .map(|s| &*e.insert(DoctorStim::new(leveller::doctor_stim_slice(s))))
            }
        }
    }

    /// Measure one sound: isolate it, capture + profile it (one floor-suspect retry), and
    /// record whether the next sound may skip its load.
    fn measure_sound(
        &mut self,
        item: &DoctorInput,
        path: &str,
        kind: doctor::StimulusKind,
    ) -> Result<SoundOutcome, LevelError> {
        // A CAPTURE stimulus (real DI) is injected VERBATIM — calibration
        // scaling is None-by-definition (mirrors the leveling seam), so its
        // amplitude drives the chain as recorded. Synthetic keeps its scalar.
        let cal = match kind {
            doctor::StimulusKind::Capture => None,
            doctor::StimulusKind::Synthetic => item.calibration_lufs,
        };
        let stim = Self::stimulus(&mut self.stims, path, cal);
        // Pre-engage writes for this sound (isolation bypasses + the footswitch's
        // param-function valueA jumps) — the shared `resolve_sound_isolation`
        // policy (see its doc).
        let iso = resolve_sound_isolation(
            &item.nodes,
            &item.footswitches,
            &item.scene_overrides,
            item.scene,
            item.footswitch,
            item.list_index,
            &mut self.preset_cache,
        );
        let skip_load = doctor_skip_load(self.prev.as_ref(), item.list_index, item.scene.is_some());
        let result = stim.and_then(|stim| {
            // An unresolvable footswitch sound never reaches a capture: folding it into
            // `stim` reports it through the loop's own per-sound error path (a skipped
            // sound, never an aborted run) instead of measuring the wrong sound.
            match &iso.unresolved {
                Some(e) => Err(e.clone()),
                None => Ok(stim),
            }
        });
        let result = result
            .map_err(LevelError::from)
            .and_then(|stim| capture_profile_with_floor_retry(item, &iso, stim, skip_load));
        match result {
            Ok((profile, cov)) => {
                self.prev = Some(PrevSound {
                    list_index: item.list_index,
                    wrote: !iso.bypass.is_empty() || !iso.params.is_empty(),
                });
                Ok(SoundOutcome::Measured {
                    profile,
                    coverage: Some(cov),
                    nodes: iso.graph,
                })
            }
            Err(e) => {
                // A failed sound may have left the unit on ANY preset (e.g. its
                // load connection never opened) — the next sound must reload. (A
                // cancel ends the run, so the reset is moot there.)
                self.prev = None;
                Err(e)
            }
        }
    }
}

/// One sound's capture + profile, retried ONCE on a suspect read ([`capture_suspect`]).
fn capture_profile_with_floor_retry(
    item: &DoctorInput,
    iso: &SoundIsolation,
    stim: &DoctorStim,
    skip_load: bool,
) -> Result<(doctor::SoundProfile, Vec<bool>), LevelError> {
    let stim_spread = stim.spread;
    let first = doctor_capture_profile(item, iso, stim, skip_load)?;
    let Some(suspect) = suspect_of(&first, stim_spread) else {
        return Ok((first.profile, first.coverage));
    };
    // A floor read, a missing onset or a truncated capture: the reading can't be
    // diagnosed as captured. Wait out the quiet gap and force a fresh preset recall
    // (skip_load=false) — a bad capture means the inject failed, not that the cached
    // working copy is dirty.
    log::warn!(
        "doctor_check: {suspect:?} capture for {} (momentary spread {:.2} LU, stimulus {:.2} LU, onset {:?}) — retrying once",
        item.key, first.profile.momentary_spread_lu, stim_spread, first.onset.source
    );
    crate::device_gate::settle_or_cancel(leveller::FLOOR_RETRY_GAP_MS)?;
    let second = doctor_capture_profile(item, iso, stim, false)?;
    match suspect_of(&second, stim_spread) {
        None => Ok((second.profile, second.coverage)),
        Some(suspect) => {
            log::warn!(
                "doctor_check: still {suspect:?} after the retry for {} — reporting an error",
                item.key
            );
            Err(suspect.into_error())
        }
    }
}

/// One analysed Doctor capture (see [`leveller::ProfiledCapture`]).
type DoctorCaptureProfile = leveller::ProfiledCapture;

/// [`capture_suspect`] on one analysed capture.
fn suspect_of(p: &DoctorCaptureProfile, stim_spread: f64) -> Option<CaptureSuspect> {
    capture_suspect(
        p.onset.confident(),
        p.has_tail,
        p.profile.momentary_spread_lu,
        stim_spread,
    )
}

/// One capture + profile attempt, `skip_load` threaded through (the retry
/// forces a fresh preset recall — a floor read means the inject
/// failed, not that the working copy is stale).
/// Typed, so a Stop mid-capture reaches the caller as a cancel.
fn doctor_capture_profile(
    item: &DoctorInput,
    iso: &SoundIsolation,
    stim_cache: &DoctorStim,
    skip_load: bool,
) -> Result<DoctorCaptureProfile, LevelError> {
    let stim = stim_cache.samples.as_slice();
    let family = instrument_of(item);
    let tail_ms_u32 = doctor::doctor_tail_ms(&iso.graph);
    let tail_ms = u64::from(tail_ms_u32);
    // `_with_loudness`: the production Doctor path REPORTS `integrated_lufs`
    // to the user (D4), so it needs the un-mixed capture's stereo-measured
    // Loudness alongside the average-mixdown `samples` bands/PSD run on —
    // see `doctor_capture_with_loudness`'s doc.
    // ⟦P1/P4⟧ PREMISE-CHECK SEAM — see `validate_log::emit_doctor`'s doc.
    let validate = crate::validate_log::doctor_log_path().map(|_| {
        crate::validate_log::ValidationRow::doctor(
            item.list_index.get(),
            item.scene,
            item.footswitch,
        )
    });
    let (samples, rate, stereo_loudness) = leveller::doctor_capture_with_loudness(
        item.list_index,
        item.scene,
        &iso.bypass,
        &iso.params,
        stim,
        Some(0.5),
        tail_ms,
        skip_load,
        validate.as_ref(),
    )?;
    audible_or_no_signal(&stereo_loudness)?;
    Ok(profile_capture(
        &samples,
        rate,
        stereo_loudness,
        stim_cache,
        family,
        tail_ms_u32,
    )?)
}

/// A silent capture (integrated loudness −inf) is the typed no-signal case, not an
/// analysis failure: Results word it from the preset's silence hint (the amp at zero, a
/// muting pedal, nothing on USB 1/2) by its kind, never by its text.
/// `SoundProfile::from_channels` keeps its own guard for the other callers.
fn audible_or_no_signal(loudness: &crate::lufs::Loudness) -> Result<(), LevelError> {
    if loudness.integrated_lufs.is_finite() {
        Ok(())
    } else {
        Err(LevelError::NoSignal)
    }
}

/// Profile one Doctor capture: [`leveller::analyze_capture`] (the one
/// measurement space, shared with the calibration sweeps) with `stereo_loudness`
/// as its loudness and the cached stimulus PSD. An un-aligned onset is refused by
/// the caller ([`capture_suspect`]), never diagnosed — `doctor_onset` already
/// WARNs naming the fallback.
fn profile_capture(
    sig: &leveller::DoctorSignal,
    rate: u32,
    stereo_loudness: crate::lufs::Loudness,
    stim_cache: &DoctorStim,
    family: doctor::Instrument,
    tail_ms_u32: u32,
) -> Result<DoctorCaptureProfile, String> {
    let stim_psd = stim_cache.psd_at(rate);
    leveller::analyze_capture(
        sig,
        rate,
        &stim_cache.samples,
        &stim_psd,
        family,
        tail_ms_u32,
        Some(stereo_loudness),
    )
}

/// Apply every op in `ops` on `s`'s live edit buffer (the caller must already
/// have `confirm_active` + `begin_live_edit`'d the session) — the ONE home of a
/// prescription op's wire semantics (`Param` → `change_parameter`; `InsertNode` →
/// `insert_node` + its param follow-ups on the id the device's confirm gave the
/// fresh node). Inserts are first checked against the live roster
/// ([`check_doctor_inserts`]). Returns the first failure's detail string; the
/// caller decides how to recover. The app never writes a prescription (Doctor only
/// describes its fixes); `probe --doctor-inject` is the one caller, through
/// [`ops_session`].
fn apply_doctor_ops(s: &mut Session, ops: &[doctor::DoctorOp]) -> Result<(), String> {
    // The working copy's (group, FenderId, node id)s, kept current as inserts land: the
    // read-back count and the known ids an unconfirmed insert's id is told apart from.
    let mut placed: Vec<(String, String, String)> = Vec::new();
    if ops
        .iter()
        .any(|op| matches!(op, doctor::DoctorOp::InsertNode { .. }))
    {
        let live = s.live_complete_value()?;
        let roster = blockcaps::roster_from_preset(&live);
        check_doctor_inserts(&roster, ops)?;
        placed = roster
            .into_iter()
            .map(|e| (e.group, e.fender_id, e.node_id))
            .collect();
    }
    for op in ops {
        let outcome: Result<bool, String> = match op {
            doctor::DoctorOp::Param {
                group_id,
                node_id,
                param,
                value,
            } => s
                .change_parameter(group_id, node_id, param, *value as f32)
                .map(|_| true),
            doctor::DoctorOp::InsertNode {
                group_id,
                before_node_id,
                fender_id,
                params,
            } => {
                let was = placed
                    .iter()
                    .filter(|(g, f, _)| g == group_id && f == fender_id)
                    .count();
                match s.insert_node_once(group_id, before_node_id.as_deref(), fender_id, Some(was))
                {
                    Ok(true) => {
                        let known: Vec<String> =
                            placed.iter().map(|(_, _, id)| id.clone()).collect();
                        let node = s.inserted_node_id(group_id, &known);
                        // Counted even unidentified: the next insert's read-back needs it.
                        placed.push((
                            group_id.clone(),
                            fender_id.clone(),
                            node.clone().unwrap_or_default(),
                        ));
                        let node = match node {
                            Some(n) => n,
                            None if params.is_empty() => continue,
                            None => {
                                return Err(format!("could not identify the inserted {fender_id}"))
                            }
                        };
                        let mut r = Ok(true);
                        for (p, v) in params {
                            if let Err(e) = s.change_parameter(group_id, &node, p, *v as f32) {
                                r = Err(e);
                                break;
                            }
                        }
                        r
                    }
                    other => other,
                }
            }
        };
        match outcome {
            Ok(true) => continue,
            Ok(false) => return Err("the device rejected the edit".to_string()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// [`blockcaps::check_insert`] for every Doctor insert, against the live pre-edit
/// `roster` (the probe's apply path has no other cap or anchor check).
fn check_doctor_inserts(
    roster: &[blockcaps::RosterEntry],
    ops: &[doctor::DoctorOp],
) -> Result<(), String> {
    let mut counts = blockcaps::counts(roster);
    ops.iter().try_for_each(|op| match op {
        doctor::DoctorOp::InsertNode {
            group_id,
            before_node_id,
            fender_id,
            ..
        } => blockcaps::check_insert(
            roster,
            &mut counts,
            group_id,
            before_node_id.as_deref(),
            fender_id,
        ),
        doctor::DoctorOp::Param { .. } => Ok(()),
    })
}

/// Open a live-edit session on `list_index`, confirm identity, recall `scene`,
/// and apply `ops` — the ONE home of the connect → `confirm_active` →
/// `begin_live_edit` → `load_scene` → `apply_doctor_ops` sequence. On success
/// returns the still-open session, so the caller can capture the edit buffer on it
/// without a recall reverting the ops; it never saves. On ANY failure the live
/// session is dropped (freeing the seize) BEFORE `restore_saved_preset` reconnects
/// to discard the partial edit, and an error naming `verb` is returned.
///
/// `scene`: see `apply_ops_under_scene`'s doc.
///
/// `pub(crate)`: used by `probe --doctor-inject` only (touches only
/// `Session`/`leveller`, no Tauri state) — the app has no prescription write path
/// (Doctor describes its fixes, the player makes them on the unit).
///
/// Every failure past this point — including `Session::connect`/`confirm_active`/
/// `begin_live_edit`/`load_scene`, not just `apply_doctor_ops` — routes through
/// the same restore: the probe calls this right after a BEFORE capture that may
/// already have written unsaved isolation onto the live edit buffer
/// (`leveller::doctor_capture`/`capture_full_at`, which never reloads before
/// returning), so the device can be dirty even before this function's own
/// `Session::connect` runs — a setup failure here must still discard that.
pub(crate) fn ops_session(
    list_index: ListIndex,
    expect_name: &str,
    scene: Option<u32>,
    ops: &[doctor::DoctorOp],
    verb: &str,
) -> Result<Session, String> {
    fn restored_err(list_index: ListIndex, verb: &str, detail: String) -> String {
        match leveller::restore_saved_preset(list_index) {
            Ok(()) => format!("couldn't {verb} — the preset was restored unchanged: {detail}"),
            Err(restore_err) => format!(
                "couldn't {verb} ({detail}) AND the restore also failed ({restore_err}) — verify the preset on the unit"
            ),
        }
    }
    // `s` is local to this closure, so an early `?` return drops it (freeing
    // the seize) as the closure's scope unwinds — BEFORE `.map_err` below
    // calls `restored_err`, which opens its own connection to restore.
    let setup = || -> Result<Session, String> {
        // The job's scene comes from the frontend's diagnosis, not a fresh read: prove it
        // against the stored document before recalling it (`doctor_scene_refusal`).
        if let Some(n) = scene.filter(|&n| n != session::BASE_SCENE_SLOT) {
            let (doc, _, _, cut) =
                crate::slot_read::read_slot_preset_sections(list_index, &["scenes"])?;
            if let Some(why) = doctor_scene_refusal(&doc, n, cut.truncated.is_empty()) {
                return Err(why);
            }
        }
        let mut s = Session::connect()?;
        s.confirm_active(list_index, Some(expect_name))?;
        apply_ops_under_scene(&mut s, scene, ops)?;
        Ok(s)
    };
    setup().map_err(|detail| restored_err(list_index, verb, detail))
}

/// Why a Doctor job may not recall `scene` on the preset `doc` describes, or `None`.
/// `scenes_whole` says the read delivered the `scenes` section uncut; a cut one only
/// proves a scene-less preset (an empty list), never a short one.
pub(crate) fn doctor_scene_refusal(
    doc: &serde_json::Value,
    scene: u32,
    scenes_whole: bool,
) -> Option<String> {
    let scene_less = doc
        .get("scenes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty);
    let refuse = scene_less || (scenes_whole && !session::scene_index_ok(doc, scene));
    refuse.then(|| {
        format!(
            "scene {} isn't on this preset any more; diagnose it again",
            scene + 1
        )
    })
}

/// `begin_live_edit` → `load_scene` → `apply_doctor_ops` on an already-confirmed
/// session — split out of `ops_session` so the op ORDER (scene recall before any
/// write) is unit-testable against a `SimDevice` without a real HID connection
/// (`ops_session` itself needs `Session::connect()` + `confirm_active`'s "My
/// Presets" list echo, which the fake doesn't model).
///
/// `scene`: the diagnosed sound's 0-based `scenes[]` wire index, `None` for
/// base. Recalled explicitly (base via `session::BASE_SCENE_SLOT`, never omitted)
/// because a bare `changeParameter` with no preceding `loadScene` lands in
/// whatever scene the CONNECTION happens to default to (the preset's saved
/// `lastLoadedScene`), not necessarily the diagnosed scene — omitting the recall
/// let a scene-2 prescription silently write scene-0's overlay (or base) instead.
///
/// KNOWN LIMITATION: when the diagnosed node has no overlay of its own in `scene`
/// (`scene_jobs::SceneWriteVerdict` is not `WriteDirect`), a bare `changeParameter`
/// here leaks to base, and enabling the overlay would reseed the node's other
/// scene-edited params from base with no repair pass. The one caller
/// (`probe --doctor-inject`) applies on the base sound only.
fn apply_ops_under_scene(
    s: &mut Session,
    scene: Option<u32>,
    ops: &[doctor::DoctorOp],
) -> Result<(), String> {
    s.begin_live_edit()?;
    s.load_scene(scene.unwrap_or(session::BASE_SCENE_SLOT))?;
    crate::device_gate::settle(std::time::Duration::from_millis(
        leveller::SETTLE_AFTER_SET_MS,
    ));
    apply_doctor_ops(s, ops)
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
