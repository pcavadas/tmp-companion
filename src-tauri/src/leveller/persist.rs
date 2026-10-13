//! Post-save persist verify: re-read the saved preset once and flag every write the save
//! did not keep, for the scene lane ([`persist_mismatches`]) and the footswitch lane
//! ([`verify_fs_persisted_writes`]).

use super::*;

/// One solved scene write, to be checked against what the batch-end save actually persisted.
#[derive(Debug, Clone)]
pub(crate) struct PersistedWrite {
    /// The outcome row the value was solved for; `session::BASE_SCENE_SLOT` = the base row.
    pub scene_slot: u32,
    /// Where the value LIVES in the saved document: `Some(s)` = scene `s`'s overlay, `None` =
    /// the base graph. Not always `scene_slot`'s own overlay — see [`write_overlay`].
    pub overlay: Option<u32>,
    pub node_id: String,
    pub parameter_id: String,
    /// The value the run SOLVED and reported.
    pub value: f32,
}

/// [`PersistedWrite::overlay`] for a write solved for row `scene_slot`: `None` (base) for the
/// base row AND for a scene write the landing policy sent to the shared base value
/// (`WriteDirect { lands_on_base: true }`, decided from the same pre-run `saved` doc `set_knobs`
/// wrote under).
pub(super) fn write_overlay(
    saved: Option<&serde_json::Value>,
    scene_slot: u32,
    node_id: &str,
    parameter_id: &str,
) -> Option<u32> {
    if scene_slot >= crate::session::BASE_SCENE_SLOT {
        return None;
    }
    match saved.map(|sv| scene_write_verdict_for_param(sv, scene_slot, node_id, parameter_id)) {
        Some(SceneWriteVerdict::WriteDirect {
            lands_on_base: true,
        }) => None,
        _ => Some(scene_slot),
    }
}

/// Agreement band between a solved `f32` and its round-tripped JSON value. Wide enough for
/// the float formatting, far below any real leveling step.
pub(super) const PERSIST_TOL: f64 = 1e-3;

/// What a post-save re-read can say about ONE solved write. The third state is the whole
/// point: "the document does not carry this value" and "the document cannot speak to this
/// location at all" are different facts, and only the first is evidence of a lost write.
pub(super) enum PersistedRead {
    /// The saved document answers with this value.
    Value(f64),
    /// The document COVERS the location and holds nothing there — a genuine miss.
    Absent,
    /// The document cannot answer: the section the value lives in never arrived.
    Unverifiable,
}

/// What the saved document holds for one solved write, read where it LANDED
/// ([`PersistedWrite::overlay`]): the scene overlay's value, or the base graph node's
/// (`scene_overlay` answers `Unknown` at/above `BASE_SCENE_SLOT`, so base must not go through
/// it).
pub(super) fn persisted_value(saved: &serde_json::Value, w: &PersistedWrite) -> PersistedRead {
    let Some(scene) = w.overlay else {
        // `audioGraph` heads the document, so a tail truncation never reaches it — but a
        // read that lost it lost everything, and must not be read as a wiped write.
        if !saved.get("audioGraph").is_some_and(|g| g.is_object()) {
            return PersistedRead::Unverifiable;
        }
        return match crate::audiograph::node_param_f64(saved, &w.node_id, &w.parameter_id) {
            Some(v) => PersistedRead::Value(v),
            None => PersistedRead::Absent,
        };
    };
    // CALL-SITE DECISION (three-state split): a value LOOKUP, so `Full` and `BypassOnly`
    // share one arm — read whatever the overlay holds. A `BypassOnly` overlay carries no
    // knob keys, so the lookup misses and the write counts as a MISS, which is right: the
    // only write the policy lets through there lands on BASE and arrives with `overlay:
    // None` (above), so a value reported as written INTO one is by definition not persisted.
    //
    // `Unknown` is the arm that must NOT collapse into that miss (HW, 2026-08-19): it means
    // the `scenes` section never arrived, and `scenes` sits at the document tail, so it is
    // exactly what a truncated field-8 read loses. "Friedman HBE" truncates at 21044 B before
    // its scenes, and the run duly warned that scene 3's `ACD_TwinReverb65NoFx/outputLevel`
    // "solved 0.7814 but the saved preset holds no such value" — while that scene's own
    // re-measure of the SAVED state read -22.99 LUFS against its -23 target, i.e. the write
    // had persisted perfectly. A false "did not persist" is worse than no check: it teaches
    // the user to distrust correct results, and the external judge SKIPS a good row.
    match overlay_param(saved, scene, &w.node_id, &w.parameter_id) {
        SceneParamRead::Value(v) => match v.as_f64() {
            Some(v) => PersistedRead::Value(v),
            None => PersistedRead::Absent,
        },
        SceneParamRead::Absent => PersistedRead::Absent,
        SceneParamRead::Unknown => PersistedRead::Unverifiable,
    }
}

/// What a post-save re-read found, split by what it can honestly claim.
#[derive(Debug, Default)]
pub(crate) struct PersistCheck {
    /// Writes the save did NOT keep, as `(scene_slot, detail)` — the report must not show
    /// these numbers as persisted.
    pub(crate) missed: Vec<(u32, String)>,
    /// Writes the re-read cannot speak to, as `(scene_slot, detail)`. NOT a miss: the caller
    /// leaves the verdict unknown rather than stamping a mismatch it cannot support.
    pub(crate) unverifiable: Vec<(u32, String)>,
}

/// Grade every solved write against the re-read saved document. A value that diverges (or is
/// absent from a section that DID arrive) is a miss — the gate exists so a report can never
/// show numbers the save wiped. A value whose section never arrived is `unverifiable`, which
/// is not the same thing and must never be reported as a lost write.
pub(crate) fn persist_mismatches(
    saved: &serde_json::Value,
    writes: &[PersistedWrite],
) -> PersistCheck {
    let mut out = PersistCheck::default();
    for w in writes {
        match persisted_value(saved, w) {
            PersistedRead::Value(got) if (got - w.value as f64).abs() <= PERSIST_TOL => {}
            PersistedRead::Value(got) => out.missed.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the save holds {got:.4}",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
            PersistedRead::Absent => out.missed.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the saved preset holds no such value",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
            PersistedRead::Unverifiable => out.unverifiable.push((
                w.scene_slot,
                format!(
                    "{}/{} solved {:.4} but the re-read carries no section holding it",
                    w.node_id, w.parameter_id, w.value
                ),
            )),
        }
    }
    out
}

/// Post-save param-level verify: RE-READ the preset and confirm every solved write survived
/// the batch-end save, stamping `persist_mismatch` on each outcome. Cheap and audio-free —
/// one field-8 read per preset per run, after all capture work — and the one thing that stops
/// a summary from reporting pre-wipe numbers as persisted. `writes` pairs a scene slot with
/// the values solved for it; scenes that wrote nothing are not checked.
///
/// `force_bypass_restore` is A5/F2's DETECTION half: `(group_id, node_id, expected_bypass)` for
/// every node a trade's base isolation forced — this function otherwise reads only
/// `node_param_f64` numeric values (`persist_mismatches`), so a silently DROPPED inverse write
/// (the pedal saved forced OFF) would be invisible without it. A mismatch here is loud: it
/// means the run's own isolation cleanup did not land, so the saved preset carries a pedal in a
/// state the player never authored.
pub(super) fn verify_persisted_writes(
    slot: ListIndex,
    writes: &[PersistedWrite],
    force_bypass_restore: &[(String, String, bool)],
    outcomes: &mut [BatchedSceneOutcome],
) {
    if writes.is_empty() && force_bypass_restore.is_empty() {
        return;
    }
    // `save_deferred_scene_writes` has just closed its session and `read_saved_preset` sleeps
    // only AFTER itself, so the opening gap is the caller's to provide.
    // COMPLETE-OR-FAIL, not the plain read: this verifier compares SCENE OVERLAYS, which sit
    // at the tail of the document, and a field-8 stream that truncates before `scenes` makes
    // every checked scene look unwritten. HW, 2026-08-19: "Friedman HBE" truncates at 21044 B
    // before its scenes section, and the run duly warned that scene 3's
    // `ACD_TwinReverb65NoFx/outputLevel` "solved 0.7814 but the saved preset holds no such
    // value" — while that scene's own re-measure of the SAVED state read -22.99 LUFS against
    // its -23 target, i.e. the write had persisted perfectly. A false "did not persist" is
    // worse than no check: it teaches the user to distrust correct results, and the external
    // judge SKIPS the row rather than passing it.
    let saved = match read_saved_preset_complete(slot) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "slot {slot}: post-save verify skipped — the saved preset could not be re-read \
                 completely ({e}); the reported values are unconfirmed"
            );
            return;
        }
    };
    let check = persist_mismatches(&saved, writes);
    for o in outcomes.iter_mut() {
        // Only a scene we actually checked gets a verdict; the rest stay `None` (unknown).
        if !writes.iter().any(|w| w.scene_slot == o.scene_slot) {
            continue;
        }
        // A scene the re-read cannot speak to stays `None` too — an unanswerable document is
        // not evidence of a lost write (see `PersistedRead::Unverifiable`). Second line of
        // defence behind the complete-or-fail read above, and the one that survives a
        // document which arrives whole but still carries no `scenes`.
        if check.unverifiable.iter().any(|(s, _)| *s == o.scene_slot) {
            continue;
        }
        let mismatch = check.missed.iter().any(|(scene, _)| *scene == o.scene_slot);
        o.persist_mismatch = Some(mismatch);
    }
    for (scene, detail) in &check.missed {
        log::warn!(
            "slot {slot} scene {scene}: the save did not persist the leveled value — {detail}"
        );
    }
    for (scene, detail) in &check.unverifiable {
        log::warn!(
            "slot {slot} scene {scene}: the leveled value is UNCONFIRMED, not lost — {detail}"
        );
    }
    // A5/F2 DETECTION: confirm every isolated node's bypass came back to what the pre-run
    // saved document held. A mismatch here means the trade's inverse-write cleanup silently
    // dropped — the saved preset now carries this pedal forced OFF — so it is stamped on the
    // BASE outcome (the only row a base isolation fact can attach to) and logged loudly.
    let mut isolation_leaked = false;
    for (_g, node_id, expected) in force_bypass_restore {
        let got = crate::footswitch::block_bypassed_in_base(&saved, node_id);
        if got != *expected {
            isolation_leaked = true;
            log::error!(
                "slot {slot}: base isolation on {node_id} did NOT persist as restored — saved \
                 bypass is {got}, expected {expected}; the trade's inverse write was dropped"
            );
        }
    }
    if isolation_leaked {
        if let Some(base_outcome) = outcomes
            .iter_mut()
            .find(|o| o.scene_slot == crate::session::BASE_SCENE_SLOT)
        {
            base_outcome.persist_mismatch = Some(true);
        }
    }
}

/// The FS-lane mirror of [`verify_persisted_writes`]: re-read the saved preset ONCE after the
/// FS batch save and confirm every solved+saved switch's write survived, stamping
/// `persist_mismatch` on the result at each `(idx, node_id, param, value, is_assign)` entry
/// (`is_assign` picks the read: `dspUnitParameters` for a Bake, the `ftsw` table's `valueA`
/// for an Assign — the two writes land in different places on the device).
///
/// It reads the STORED row (field 8), which on fw 1.8.58 holds a save the moment its handler
/// returns, so it detects any write that didn't persist (a dropped chunked edit / lapse /
/// rejection / pre-save revert). `base_expect` is the `presetLevel` the preset held before
/// the batch, which writes none of its own. A mismatch there means the batch's save reverted
/// the base level — which no per-switch param check
/// would ever catch on its own — so every switch in this batch is stamped mismatched too
/// (the whole preset's base sound, not just one switch, is now suspect).
pub(crate) fn verify_fs_persisted_writes(
    slot: ListIndex,
    writes: &[(usize, String, String, f32, bool)],
    base_expect: Option<f32>,
    results: &mut [Option<FootswitchLevelResult>],
) {
    if writes.is_empty() {
        return;
    }
    // COMPLETE-OR-FAIL for the same reason as the scene twin above: a truncated document
    // reports a write that landed as a write that vanished.
    let saved = match read_saved_preset_complete(slot) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "slot {slot}: FS post-save verify skipped — the saved preset could not be \
                 re-read completely ({e}); the reported values are unconfirmed"
            );
            return;
        }
    };
    for (idx, verdict) in fs_persist_verdicts(&saved, writes, base_expect) {
        match verdict {
            Some(mismatch) => {
                if let Some(r) = results.get_mut(idx).and_then(|o| o.as_mut()) {
                    r.persist_mismatch = Some(mismatch);
                }
                if mismatch {
                    log::warn!(
                        "slot {slot}: FS result idx {idx} did not persist as solved (or the \
                         batch's save changed the stored presetLevel)"
                    );
                }
            }
            // Verdict left unknown on purpose — see `fs_persist_verdicts`.
            None => log::warn!(
                "slot {slot}: FS result idx {idx} is UNCONFIRMED, not lost — the re-read \
                 carries no section holding its value"
            ),
        }
    }
}

/// The pure grading behind [`verify_fs_persisted_writes`]: `(result index, verdict)` per
/// write, where `Some(true)` is a lost write, `Some(false)` a kept one, and `None` means the
/// re-read cannot speak to it.
///
/// That third state is the gate (HW, 2026-08-19). A field-8 read of a large preset is
/// tail-truncated, and the sections these writes live in — `ftsw` for an Assign, `audioGraph`
/// for a Bake — can simply be missing. Grading a missing SECTION as a missing VALUE reports a
/// write that landed as a write that vanished, which is worse than no check at all: it
/// teaches the user to distrust correct results. Second line of defence behind the
/// complete-or-fail read; it holds even for a document that arrives whole but short.
pub(super) fn fs_persist_verdicts(
    saved: &serde_json::Value,
    writes: &[(usize, String, String, f32, bool)],
    base_expect: Option<f32>,
) -> Vec<(usize, Option<bool>)> {
    let ftsw = saved.get("ftsw").filter(|f| f.is_array());
    let has_graph = saved.get("audioGraph").is_some_and(|g| g.is_object());
    // The base-revert arm reads `audioGraph.presetLevel`, so without the graph its `None`
    // would read as "reverted" and condemn every switch in the batch on a read defect.
    if base_expect.is_some() && !has_graph {
        return writes.iter().map(|w| (w.0, None)).collect();
    }
    let base_reverted =
        base_expect.is_some_and(|pl| match crate::audiograph::preset_level(saved) {
            Some(got) => (got - pl as f64).abs() > PERSIST_TOL,
            None => true,
        });
    writes
        .iter()
        .map(|(idx, node, param, value, is_assign)| {
            if !if *is_assign {
                ftsw.is_some()
            } else {
                has_graph
            } {
                return (*idx, None);
            }
            let got = if *is_assign {
                ftsw.and_then(|f| ftsw_value_a(f, node, param))
            } else {
                crate::audiograph::node_param_f64(saved, node, param)
            };
            let mismatch = base_reverted
                || match got {
                    Some(v) => (v - *value as f64).abs() > PERSIST_TOL,
                    None => true,
                };
            (*idx, Some(mismatch))
        })
        .collect()
}

#[cfg(test)]
mod persist_verify_tests {
    use super::*;

    // Base graph carrying one amp at outputLevel 0.40, plus a scene 0 overlay that holds
    // 0.72 for the same node — the shape a batch-end save leaves behind.
    fn saved_preset() -> serde_json::Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": {
                "G1": [ { "nodeId": "amp", "FenderId": "amp",
                          "dspUnitParameters": { "bypass": false, "outputLevel": 0.40 } } ]
            } },
            "scenes": [
                { "guitarNodes": { "G1": { "amp": { "dspUnitParameters": { "outputLevel": 0.72 } } } } },
                { "guitarNodes": { "G1": {} } }
            ]
        })
    }

    fn write(scene_slot: u32, value: f32) -> PersistedWrite {
        PersistedWrite {
            scene_slot,
            overlay: write_overlay(None, scene_slot, "amp", "outputLevel"),
            node_id: "amp".to_string(),
            parameter_id: "outputLevel".to_string(),
            value,
        }
    }

    // The gate this exists for: a report must never show numbers the save did not persist.
    #[test]
    fn persist_mismatches_flags_only_the_scenes_the_save_did_not_keep() {
        let saved = saved_preset();

        // Solved == persisted, in the scene overlay and at base: nothing to flag.
        assert!(persist_mismatches(&saved, &[write(0, 0.72)])
            .missed
            .is_empty());
        assert!(
            persist_mismatches(&saved, &[write(crate::session::BASE_SCENE_SLOT, 0.40)])
                .missed
                .is_empty()
        );

        // The wipe case: the overlay holds a DIFFERENT value than the run solved.
        let miss = persist_mismatches(&saved, &[write(0, 0.55)]).missed;
        assert_eq!(
            miss.len(),
            1,
            "the divergent scene must be flagged: {miss:?}"
        );
        assert_eq!(miss[0].0, 0);

        // A base write compared against the base graph, same divergence.
        assert_eq!(
            persist_mismatches(&saved, &[write(crate::session::BASE_SCENE_SLOT, 0.55)])
                .missed
                .len(),
            1
        );

        // Scene 1 has no overlay for the node at all — the write is simply not there, which
        // is a miss, not an "unknown" to be waved through.
        let miss = persist_mismatches(&saved, &[write(1, 0.61)]).missed;
        assert_eq!(miss.len(), 1, "an absent overlay is a miss: {miss:?}");
        assert_eq!(miss[0].0, 1);

        // Only the divergent write is reported when a batch mixes both.
        let mixed = persist_mismatches(&saved, &[write(0, 0.72), write(1, 0.61)]).missed;
        assert_eq!(mixed.len(), 1);
        assert_eq!(mixed[0].0, 1);
    }

    /// GATE for the false "did not persist" incident (HW, 2026-08-19, "Friedman HBE"): the
    /// field-8 read of a large preset truncates at ~21044 B, BEFORE `scenes`. The run then
    /// warned that scene 3's `ACD_TwinReverb65NoFx/outputLevel` "solved 0.7814 but the saved
    /// preset holds no such value" — while that same scene's re-measure of the SAVED device
    /// state read -22.99 LUFS against its -23 target, i.e. the write had persisted perfectly.
    /// The external judge duly SKIPped a row that should have passed.
    ///
    /// A document that cannot answer must be reported as UNCONFIRMED, never as a lost write.
    /// This is the second line of defence behind `read_saved_preset_complete`: it holds even
    /// for a document that arrives whole and still carries no `scenes`.
    #[test]
    fn a_truncated_reread_is_unconfirmed_not_a_lost_write() {
        // Exactly what the HBE re-read returned: everything up to the tail, `scenes` gone.
        let mut truncated = saved_preset();
        truncated.as_object_mut().expect("object").remove("scenes");

        let check = persist_mismatches(&truncated, &[write(0, 0.72)]);
        assert!(
            check.missed.is_empty(),
            "a scene the re-read cannot see must NOT be reported as a lost write: {:?}",
            check.missed
        );
        assert_eq!(
            check.unverifiable.len(),
            1,
            "it must be reported as unconfirmed instead: {check:?}"
        );
        assert_eq!(check.unverifiable[0].0, 0);

        // The base write in the SAME document is still fully checkable — `audioGraph` heads
        // the document, so a tail truncation never costs it. Grading it as unverifiable too
        // would throw away a real check.
        let base = persist_mismatches(&truncated, &[write(crate::session::BASE_SCENE_SLOT, 0.55)]);
        assert!(base.unverifiable.is_empty(), "{base:?}");
        assert_eq!(
            base.missed.len(),
            1,
            "base divergence still caught: {base:?}"
        );

        // A scenes array that ARRIVED but is cut short of the slot is the same class: the
        // HBE read returned 3 of 4 scenes, the third cut mid-record.
        let short = persist_mismatches(&saved_preset(), &[write(7, 0.61)]);
        assert!(
            short.missed.is_empty(),
            "a scene slot past the end of a truncated scenes array is unconfirmed: {short:?}"
        );
        assert_eq!(short.unverifiable.len(), 1, "{short:?}");

        // And the guarantee that keeps this from becoming a blanket amnesty: when `scenes`
        // IS present and covers the slot, a genuinely-absent overlay stays a MISS.
        let real = persist_mismatches(&saved_preset(), &[write(1, 0.61)]);
        assert_eq!(
            real.missed.len(),
            1,
            "a covered-but-empty overlay is still a lost write: {real:?}"
        );
        assert!(real.unverifiable.is_empty(), "{real:?}");
    }

    /// GATE for the shared-base scene write (offline profiling, 2026-10-10, slot 402 "Solo"):
    /// a scene whose overlay is bypass-only (Scene Edit off) has no knob to land in, so the
    /// write-landing policy sends it to the SHARED base value. Graded against the scene's
    /// overlay, the run warned "solved 3.0000 but the saved preset holds no such value" for a
    /// write that had persisted. The value must be read where it LANDED.
    #[test]
    fn a_shared_base_scene_write_is_graded_against_base_not_its_overlay() {
        // Base amp bypassed; scene 0 pins its own `outputLevel` (Full overlay); scene 1's
        // bypass-only overlay un-bypasses it — `shared_write_is_scene_local`'s anatomy.
        let pre = serde_json::json!({
            "audioGraph": { "guitarNodes": {
                "G1": [ { "nodeId": "amp", "FenderId": "amp",
                          "dspUnitParameters": { "bypass": true, "outputLevel": 0.40 } } ]
            } },
            "scenes": [
                { "guitarNodes": { "G1": { "amp": { "dspUnitParameters": { "outputLevel": 0.72 } } } } },
                { "guitarNodes": { "G1": { "amp": { "dspUnitParameters": { "bypass": false } } } } }
            ]
        });
        assert_eq!(write_overlay(Some(&pre), 1, "amp", "outputLevel"), None);
        // Every other landing keeps its own address.
        assert_eq!(write_overlay(Some(&pre), 0, "amp", "outputLevel"), Some(0));
        assert_eq!(write_overlay(None, 1, "amp", "outputLevel"), Some(1));
        assert_eq!(
            write_overlay(
                Some(&pre),
                crate::session::BASE_SCENE_SLOT,
                "amp",
                "outputLevel"
            ),
            None
        );

        // After the save: base moved to the solved value, scene 1's overlay is unchanged.
        let mut post = pre.clone();
        post["audioGraph"]["guitarNodes"]["G1"][0]["dspUnitParameters"]["outputLevel"] =
            serde_json::json!(0.55);
        let landed = PersistedWrite {
            overlay: write_overlay(Some(&pre), 1, "amp", "outputLevel"),
            ..write(1, 0.55)
        };
        let check = persist_mismatches(&post, std::slice::from_ref(&landed));
        assert!(
            check.missed.is_empty() && check.unverifiable.is_empty(),
            "a persisted shared-base write must grade clean: {check:?}"
        );
        // …and a base that did NOT move is still caught, under the scene's own row.
        let miss = persist_mismatches(&pre, &[landed]).missed;
        assert_eq!(miss.len(), 1, "{miss:?}");
        assert_eq!(
            miss[0].0, 1,
            "the miss is reported on the row it was solved for"
        );
    }

    /// GATE for the scene half of the capture-level re-assert: with no trade, a scene batch's
    /// captures assert the preset's own saved level rather than `None`. On fw 1.8.58 the
    /// recall serves that same value, so this pins a defensive choice (the fw 1.8.45 recall
    /// served a pre-save level for a while after a save). The mechanism it feeds is proven by
    /// the FS twin (`a_capture_renders_at_the_run_level_not_the_recall_reverted_one`).
    #[test]
    fn a_scene_batch_captures_at_the_saved_level_when_no_trade_holds_one() {
        let saved = serde_json::json!({ "audioGraph": { "presetLevel": 0.51009 } });

        // No trade: the preset's OWN saved level, never `None`.
        assert_eq!(
            scene_capture_level(None, Some(&saved)),
            Some(0.51009),
            "with no trade the captures must render at the preset's saved level"
        );

        // A landed trade holds a RAISED level unsaved in the working copy until the batch's
        // one save, so it outranks the saved doc — which still shows the pre-raise value.
        let hold = TradeHold {
            preset_level: 0.72,
            writes: Vec::new(),
            force_bypass_restore: Vec::new(),
        };
        assert_eq!(
            scene_capture_level(Some(&hold), Some(&saved)),
            Some(0.72),
            "a trade's unsaved raise wins over the saved doc"
        );

        // Nothing to assert only when there is genuinely nothing to assert.
        assert_eq!(scene_capture_level(None, None), None);
        assert_eq!(
            scene_capture_level(None, Some(&serde_json::json!({"audioGraph": {}}))),
            None,
            "a doc with no presetLevel asserts nothing rather than inventing a level"
        );
    }

    /// The FS-lane half of the same gate. An Assign write is read out of `ftsw` and a Bake
    /// write out of `audioGraph`; either section can be missing from a truncated re-read, and
    /// a missing SECTION must never be graded as a missing VALUE.
    #[test]
    fn a_truncated_reread_leaves_fs_verdicts_unknown_rather_than_lost() {
        // Base graph + an ftsw table whose switch 0 assigns the amp's outputLevel to 0.72.
        let full = serde_json::json!({
            "audioGraph": { "presetLevel": 0.5, "guitarNodes": {
                "G1": [ { "nodeId": "amp", "FenderId": "amp",
                          "dspUnitParameters": { "bypass": false, "outputLevel": 0.40 } } ]
            } },
            "ftsw": [ [ { "func": "param", "groupId": "G1", "nodeId": "amp",
                          "parameterId": "outputLevel", "valueA": 0.72, "valueType": 1 } ] ]
        });
        let bake = (
            0usize,
            "amp".to_string(),
            "outputLevel".to_string(),
            0.40,
            false,
        );
        let assign = (
            1usize,
            "amp".to_string(),
            "outputLevel".to_string(),
            0.72,
            true,
        );
        let writes = [bake.clone(), assign.clone()];

        // Control: the whole document is there, so both writes are gradable and both kept.
        assert_eq!(
            fs_persist_verdicts(&full, &writes, Some(0.5)),
            vec![(0, Some(false)), (1, Some(false))],
        );

        // `ftsw` gone (the tail a truncated read loses): the ASSIGN becomes unknown, while
        // the bake — read from the head of the document — is still fully checked.
        let mut no_ftsw = full.clone();
        no_ftsw.as_object_mut().expect("object").remove("ftsw");
        assert_eq!(
            fs_persist_verdicts(&no_ftsw, &writes, Some(0.5)),
            vec![(0, Some(false)), (1, None)],
            "an unreadable ftsw must not condemn the assign, nor cost the bake its check"
        );

        // `audioGraph` gone: the base-revert arm cannot run either, so the whole batch is
        // unknown rather than every switch being condemned by a `presetLevel` that is merely
        // unreadable.
        let mut no_graph = full.clone();
        no_graph
            .as_object_mut()
            .expect("object")
            .remove("audioGraph");
        assert_eq!(
            fs_persist_verdicts(&no_graph, &writes, Some(0.5)),
            vec![(0, None), (1, None)],
        );

        // Not an amnesty: a document that DOES carry both sections still reports real losses.
        let lost = [
            (
                0usize,
                "amp".to_string(),
                "outputLevel".to_string(),
                0.90,
                false,
            ),
            (
                1usize,
                "amp".to_string(),
                "outputLevel".to_string(),
                0.90,
                true,
            ),
        ];
        assert_eq!(
            fs_persist_verdicts(&full, &lost, Some(0.5)),
            vec![(0, Some(true)), (1, Some(true))],
        );
        // …and a base save that reverted still condemns the batch.
        assert_eq!(
            fs_persist_verdicts(&full, &writes, Some(0.9)),
            vec![(0, Some(true)), (1, Some(true))],
        );
    }
}
