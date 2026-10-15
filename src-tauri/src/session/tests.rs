//! Session unit tests: the decoders, the handshake, keepalive and reopen behaviour, the
//! list and slot reads, and the structural-edit confirm gate, over scripted transports.

/// Device-shaped re-amp replies: proto3 omits a false flag, so a present reply without it
/// is OFF, and a missing reply is no reading at all.
#[test]
fn reamp_replies_decode_an_omitted_flag_as_off() {
    // TMS 5 { reAmpModeResponse(9) {} } and { 9 { isActive: true } }.
    assert_eq!(
        super::decode_mixer_reamp(&[0x2a, 0x02, 0x4a, 0x00]),
        Some(false)
    );
    assert_eq!(
        super::decode_mixer_reamp(&[0x2a, 0x04, 0x4a, 0x02, 0x08, 0x01]),
        Some(true)
    );
    // TMS 3 { usbSettingsResponse(56) { reampModeActive(1) {} } } and the ON shape.
    assert_eq!(
        super::decode_usb_reamp(&[0x1a, 0x05, 0xc2, 0x03, 0x02, 0x0a, 0x00]),
        Some(false)
    );
    assert_eq!(
        super::decode_usb_reamp(&[0x1a, 0x07, 0xc2, 0x03, 0x04, 0x0a, 0x02, 0x08, 0x01]),
        Some(true)
    );
    // A different message carries no reading.
    assert_eq!(super::decode_mixer_reamp(&proto_heartbeat()), None);
    assert_eq!(super::decode_usb_reamp(&proto_heartbeat()), None);
}

fn proto_heartbeat() -> Vec<u8> {
    crate::proto::heartbeat()
}

use super::lists::*;
use super::*;

/// The model ids of a series stage, in order. Panics on a non-series stage
/// (a test asserting the wrong `Stage` variant should fail loudly, not
/// silently mismatch).
fn stage_series(s: &Stage) -> Vec<String> {
    match s {
        Stage::Series { blocks } => blocks.iter().map(|b| b.model.clone()).collect(),
        _ => panic!("expected series, got {s:?}"),
    }
}

/// The model ids of each split lane, in order.
fn stage_split(s: &Stage) -> (Vec<String>, Vec<String>) {
    match s {
        Stage::Split { a, b } => (
            a.iter().map(|x| x.model.clone()).collect(),
            b.iter().map(|x| x.model.clone()).collect(),
        ),
        _ => panic!("expected split, got {s:?}"),
    }
}

/// Streams `chunks` backup chunks, one per pump, `gap` apart, then BACKUP_COMPLETE.
/// `chunks == 0` never answers at all.
struct PacedBackupTransport {
    chunks: u32,
    gap: std::time::Duration,
    sent: std::sync::Mutex<u32>,
}
impl PacedBackupTransport {
    fn report(body: &[u8]) -> Vec<u8> {
        let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
        r.extend_from_slice(body);
        r
    }
    fn chunk(&self, n: u32) -> Vec<u8> {
        let mut data = Vec::new();
        proto::field_varint(&mut data, 3, u64::from(self.chunks) * 4);
        proto::field_varint(&mut data, 4, u64::from(self.chunks));
        proto::field_varint(&mut data, 5, u64::from(n));
        data.extend(proto::len_delimited(6, &[n as u8; 4]));
        let mut state = Vec::new();
        proto::field_varint(&mut state, 1, 2);
        let mut bm = proto::len_delimited(2, &data);
        bm.extend(proto::len_delimited(3, &state));
        Self::report(&proto::len_delimited(8, &bm))
    }
    fn complete() -> Vec<u8> {
        let mut state = Vec::new();
        proto::field_varint(&mut state, 1, 4);
        Self::report(&proto::len_delimited(8, &proto::len_delimited(3, &state)))
    }
}
impl crate::hid::HidTransport for PacedBackupTransport {
    fn send(&self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn transact(&self, _: &[u8], _: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(Vec::new())
    }
    fn transact_chunked(&self, b: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.transact(b, ms)
    }
    fn transact_eager(&self, b: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.transact(b, ms)
    }
    fn pump(&self, _: u64) -> Result<Vec<Vec<u8>>, String> {
        let mut sent = self.sent.lock().unwrap();
        if self.chunks == 0 || *sent > self.chunks {
            return Ok(Vec::new());
        }
        std::thread::sleep(self.gap);
        *sent += 1;
        Ok(vec![if *sent > self.chunks {
            Self::complete()
        } else {
            self.chunk(*sent - 1)
        }])
    }
}
fn backup_session(t: PacedBackupTransport) -> Session {
    Session {
        hid: TrackedHid::new(Box::new(t)),
        raw: Vec::new(),
        fw_version: None,
        handshake_done: false,
    }
}

/// `max_secs` is a stall budget, not a transfer cap: a stream slower than the budget
/// in total still completes while each chunk lands inside it (HW: a 230-chunk library
/// needs ~85 s against the callers' 60).
#[test]
fn device_backup_completes_a_transfer_longer_than_the_stall_budget() {
    let t = PacedBackupTransport {
        chunks: 5,
        gap: std::time::Duration::from_millis(300),
        sent: std::sync::Mutex::new(0),
    };
    let (blob, stats) = backup_session(t).device_backup(1, |_| {}).unwrap();
    assert_eq!(stats.chunks_received, 5);
    assert_eq!(blob.len(), 20);
    assert!(stats.elapsed_secs > 1.0, "{}", stats.elapsed_secs);
}

#[test]
fn device_backup_errors_when_the_stream_never_starts() {
    let t = PacedBackupTransport {
        chunks: 0,
        gap: std::time::Duration::ZERO,
        sent: std::sync::Mutex::new(0),
    };
    let err = backup_session(t).device_backup(1, |_| {}).unwrap_err();
    assert!(err.contains("stalled for 1s"), "{err}");
}

/// Every body the handshake sends (the scripted device answers nothing — the
/// handshake never parses replies inline, it only accumulates).
fn handshake_sends(lean: bool) -> Vec<Vec<u8>> {
    let t = crate::test_support::ScriptedTransport::default();
    let mut s = crate::test_support::session_over(&t, Vec::new());
    s.handshake(false, None, false, lean).unwrap();
    // A binding, not a tail expression: the guard temporary must drop before `t`
    // (E0597 otherwise).
    let sends = t.sent.lock().unwrap().clone();
    sends
}

/// Every request the handshake and the keepalive send that the fake leaves unanswered
/// is a census kind, so a handshake change that adds an unmodelled request shows up
/// here rather than only in an offline run's `/sim/state`.
#[test]
fn every_unanswered_handshake_request_is_in_the_sim_census() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.handshake(true, None, true, true).expect("handshake");
    s.keepalive_pump().expect("keepalive");
    let outside: Vec<_> = sim
        .unhandled()
        .into_iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::Unhandled { tms, inner, .. }
                if !crate::sim_device::in_census(tms, inner) =>
            {
                Some((tms, inner))
            }
            _ => None,
        })
        .collect();
    assert!(
        outside.is_empty(),
        "unanswered kinds outside the census: {outside:?}"
    );
}

/// The lean handshake trims only the PUMP WINDOWS — the request byte sequence
/// must stay identical to the full handshake (the device only answers after
/// seeing the exact captured Pro Control sequence).
#[test]
fn lean_handshake_sends_the_identical_request_sequence() {
    assert_eq!(handshake_sends(false), handshake_sends(true));
}

#[test]
fn name_fallback_only_confirms_a_uniquely_mapped_slot() {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // Unique name at the target slot ⇒ the fallback may confirm.
    assert!(name_maps_uniquely(
        &names(&["Cliff", "Target", "Lead"]),
        "Target",
        ListIndex(1)
    ));
    // Same name in two slots ⇒ can't prove which is the target ⇒ fail closed.
    assert!(!name_maps_uniquely(
        &names(&["Target", "Lead", "Target"]),
        "Target",
        ListIndex(0)
    ));
    // Name present but at a DIFFERENT slot than claimed ⇒ fail closed.
    assert!(!name_maps_uniquely(
        &names(&["Cliff", "Target"]),
        "Target",
        ListIndex(0)
    ));
    // Name absent ⇒ fail closed.
    assert!(!name_maps_uniquely(
        &names(&["Cliff", "Lead"]),
        "Target",
        ListIndex(0)
    ));
}

// HW (fw 1.8.58): a plain handshake's `PresetLoaded` echo lands ~1 s after it returns.
// The wait must check the window that delivers it, including the last one.
#[test]
fn active_slot_live_reads_an_echo_that_lands_a_second_after_the_handshake() {
    let mut echo = Vec::new();
    proto::field_varint(&mut echo, 1, 1); // tabEnum = My Presets
    proto::field_varint(&mut echo, 6, 402); // device slot = list index 401
    let late = crate::sim_device::frame_multi(&crate::sim_device::preset_message(11, &echo));
    let t = crate::test_support::ScriptedTransport::default().with_pending(vec![
        vec![],
        vec![],
        vec![],
        late,
    ]);
    let mut s = crate::test_support::session_over(&t, Vec::new());
    assert_eq!(s.active_slot_live(), Some(ListIndex(401)));
}

// The song-BPM restore target: only a name at exactly one My Presets position.
#[test]
fn active_slot_by_name_needs_a_unique_non_empty_name() {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        unique_list_index(&names(&["Cliff", "Guitar", "Lead"]), "Guitar"),
        Some(ListIndex(1))
    );
    assert_eq!(
        unique_list_index(&names(&["Guitar", "Lead", "Guitar"]), "Guitar"),
        None
    );
    assert_eq!(
        unique_list_index(&names(&["Cliff", "Lead"]), "Guitar"),
        None
    );
    assert_eq!(
        unique_list_index(&names(&["Empty", "Empty"]), "Empty"),
        None
    );
}

#[test]
fn extract_active_graph_reads_nodes_and_routing() {
    // A parallel preset: two amp groups + a top-level template + splitMix.
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"Instrument Parallel 1",
            "splitMix":{"mode":"even"},
            "guitarNodes":{
                "G2":[{"FenderId":"ACD_TweedDeluxeCabIR","nodeId":"ACD_TweedDeluxeCabIR","dspUnitParameters":{"bypass":false,"instvolume":0.64}}],
                "G3":[{"FenderId":"ACD_PrincetonReverb65NoFxCabIR","dspUnitParameters":{"bypass":true}}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.template.as_deref(), Some("Instrument Parallel 1"));
    assert!(g.split_mix.is_some());
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.nodes[0].group_id, "G2");
    assert_eq!(g.nodes[0].model, "ACD_TweedDeluxeCabIR");
    assert!(!g.nodes[0].bypassed);
    assert_eq!(g.nodes[1].group_id, "G3");
    assert!(g.nodes[1].bypassed);
    // No `info` in this partial → honest None (no fabricated name/slot).
    assert_eq!(g.name, None);
    assert_eq!(g.slot, None);
}

#[test]
fn extract_active_graph_reads_dual_cab_params() {
    // A CabSim block carrying a dual cab, serialized exactly as the device does
    // (literal lowercase keys cabsimid / cab2simid / cabsim2enabled).
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{
            "G1":[
                {"FenderId":"ACD_HiwattDR103CanMod","dspUnitParameters":{"bypass":false}},
                {"FenderId":"ACD_CabSimTMS","dspUnitParameters":{"bypass":false,"cabsimid":"Mar1960aV30Alt","cab2simid":"Mar1960aV30Alt","cabsim2enabled":true,"dualcabblend":0.4}}
            ]}}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.nodes.len(), 2);
    // The bare amp head carries no cab params.
    assert_eq!(g.nodes[0].cab_sim_id, None);
    assert_eq!(g.nodes[0].cab_sim2_enabled, None);
    // The CabSim carries both cabinets + the dual flag.
    assert_eq!(g.nodes[1].model, "ACD_CabSimTMS");
    assert_eq!(g.nodes[1].cab_sim_id.as_deref(), Some("Mar1960aV30Alt"));
    assert_eq!(g.nodes[1].cab_sim_id2.as_deref(), Some("Mar1960aV30Alt"));
    assert_eq!(g.nodes[1].cab_sim2_enabled, Some(true));
}

#[test]
fn extract_active_graph_amp_half_stack_keeps_cab_id_but_no_split() {
    // Preset 003 (Cello): a HALF-STACK amp node (`...CabIR`) carries cabsim params.
    // `cabsimid` names its built-in cab (so the strip can draw a head-over-cab
    // half-stack), but `cabsim2enabled` here means DUAL MIC on the SAME cab (mic
    // sm57 + miccab2 r121, one cabinet) — NOT a dual cab — so the split fields must
    // stay None (only the standalone `ACD_CabSimTMS` block carries a real 2nd cab).
    // Params copied verbatim from the live device dump.
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{
            "G1":[
                {"FenderId":"ACD_HiwattDR103CanModCabIR","dspUnitParameters":{"bypass":false,"cab2simid":"Mar1960aV30Alt","cabsim2enabled":true,"cabsimid":"Mar1960aV30Alt","dualcabblend":0.5,"mic":"sm57","miccab2":"r121"}}
            ]}}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.nodes.len(), 1);
    assert_eq!(g.nodes[0].model, "ACD_HiwattDR103CanModCabIR");
    // The amp keeps its cab id (for the half-stack art)…
    assert_eq!(g.nodes[0].cab_sim_id.as_deref(), Some("Mar1960aV30Alt"));
    // …but the dual-cab SPLIT fields are suppressed (dual-mic, not dual-cab).
    assert_eq!(g.nodes[0].cab_sim_id2, None);
    assert_eq!(g.nodes[0].cab_sim2_enabled, None);
}

#[test]
fn scene_names_from_slot_json_recovers_truncated_partial() {
    // Field-9 plaintext partial shaped like the HW reads (probe --slotread-x):
    // device-truncated INSIDE the final scene's `uuid`, after every sceneName.
    let json = br#"{"info":{"displayName":"Guitar"},"audioGraph":{"guitarNodes":{}},"scenes":[{"sceneName":"Dist","uuid":"aaa"},{"sceneName":"Celestial","uuid":"bbb"},{"sceneName":"Swell","uuid":"cc"#;
    let names = scene_names_from_slot_json(json).expect("tolerant parse");
    // The unwind cuts at the last comma INSIDE the truncated scene object —
    // after its sceneName — so ALL names survive, the final uuid doesn't
    // (matches HW: slot 1 = 17264 B, 8/8 names present, cut mid-uuid).
    assert_eq!(
        names,
        vec![
            "Dist".to_string(),
            "Celestial".to_string(),
            "Swell".to_string()
        ]
    );

    // Complete document: all names, slot order.
    let json = br#"{"scenes":[{"sceneName":"A"},{"sceneName":"B"}],"lastLoadedScene":0}"#;
    assert_eq!(
        scene_names_from_slot_json(json).unwrap(),
        vec!["A".to_string(), "B".to_string()]
    );

    // No scenes key (truncated before it) → None, NOT Some([]).
    assert_eq!(
        scene_names_from_slot_json(br#"{"info":{"displayName":"Cello"}"#),
        None
    );

    // Empty scenes array → Some([]) (definitely no scenes).
    assert_eq!(
        scene_names_from_slot_json(br#"{"scenes":[]}"#),
        Some(vec![])
    );
}

#[test]
fn extract_active_graph_tolerates_truncation() {
    // Truncated partial: audioGraph present but no template/splitMix/info.
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{"guitarNodes":{"G1":[{"FenderId":"ACD_TM59Bassman"}]}}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.nodes.len(), 1);
    assert_eq!(g.template, None);
    assert_eq!(g.split_mix, None);
    // No splitMix → a single series stage holding the one block.
    assert_eq!(
        g.stages,
        vec![Stage::Series {
            blocks: g.nodes.clone()
        }]
    );
}

#[test]
fn extract_active_graph_normalizes_one_based_user_slot() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"info":{"displayName":"Lead","userSlot":4}}"#).unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.name.as_deref(), Some("Lead"));
    assert_eq!(g.slot, Some(ListIndex(3)));
}

#[test]
fn extract_partial_template_from_truncated_json() {
    let text = r#"{"audioGraph":{"guitarNodes":{},"splitMix":{},"template":"gtrParallel2"#;
    assert_eq!(
        extract_partial_json_string(text, "template").as_deref(),
        Some("gtrParallel2")
    );
    assert!(is_known_routing_template(Some("gtrParallel2")));
    assert!(!is_known_routing_template(Some("gtrPara")));
}

#[test]
fn current_preset_events_supply_name_and_zero_based_user_slot() {
    assert_eq!(
        extract_current_preset_display_name(b"\x12\x09\xb2\x01\x06\x0a\x04Lead").as_deref(),
        Some("Lead")
    );
    assert_eq!(
        extract_loaded_user_slot(b"\x12\x06\x5a\x04\x08\x01\x30\x04"),
        Some(ListIndex(3))
    );
    assert_eq!(
        extract_loaded_user_slot(b"\x12\x06\x5a\x04\x08\x04\x30\x04"),
        None
    );
}

#[test]
fn name_fallback_requires_a_unique_my_presets_match() {
    let presets = vec![
        PresetEntry {
            slot: ListIndex(0),
            name: "Clean".to_string(),
        },
        PresetEntry {
            slot: ListIndex(1),
            name: "Lead".to_string(),
        },
        PresetEntry {
            slot: ListIndex(2),
            name: "Lead".to_string(),
        },
    ];
    assert_eq!(
        unique_preset_slot_by_name(&presets, "Clean"),
        Some(ListIndex(0))
    );
    assert_eq!(unique_preset_slot_by_name(&presets, "Lead"), None);
    assert_eq!(unique_preset_slot_by_name(&presets, "Missing"), None);
}

// Pin the routing→stages mapping against REAL device-shaped JSON (template +
// 7 guitar slots + splitMix control inventory), not a hardcoded preset. This is the
// dual-split path from the handoff: XO Boost → split[57 Deluxe ‖ 65 Princeton]
// → Space Echo → split[Small Hall ‖ Filtron] → Small Hall. Each series block
// before/between/after the splits must keep its series role; both splits must
// survive with the right lane assignment.
#[test]
fn build_stages_dual_split_assigns_roles() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrParallel2",
            "splitMix":{"splitPoints":[{"nodeId":"split1"},{"nodeId":"split2"}],
                        "mixPoints":[{"nodeId":"mix1"},{"nodeId":"mix2"}]},
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_EPBooster","nodeId":"n1"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe","nodeId":"n2"}],
                "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx","nodeId":"n3"}],
                "G4":[{"FenderId":"ACD_SpaceEcho","nodeId":"n4"}],
                "G5":[{"FenderId":"ACD_TMSmallHall","nodeId":"n5"}],
                "G6":[{"FenderId":"ACD_MicroTronIV","nodeId":"n6"}],
                "G7":[{"FenderId":"ACD_TMSmallHall","nodeId":"n7"}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);

    assert_eq!(g.stages.len(), 5, "pre · split1 · mid · split2 · post");
    // 1. XO Boost stays in SERIES before the first split (the old bug pulled it
    //    into a lane).
    assert_eq!(stage_series(&g.stages[0]), vec!["ACD_EPBooster"]);
    // 2. First split: lane A = 57 Deluxe, lane B = 65 Princeton (not shifted).
    assert_eq!(
        stage_split(&g.stages[1]),
        (
            vec!["ACD_TweedDeluxe".into()],
            vec!["ACD_PrincetonReverb65NoFx".into()]
        )
    );
    // 3. Space Echo runs in SERIES between the two splits.
    assert_eq!(stage_series(&g.stages[2]), vec!["ACD_SpaceEcho"]);
    // 4. SECOND split survives (the old bug flattened it): Small Hall ‖ Filtron.
    assert_eq!(
        stage_split(&g.stages[3]),
        (
            vec!["ACD_TMSmallHall".into()],
            vec!["ACD_MicroTronIV".into()]
        )
    );
    // 5. Trailing Small Hall stays in series after the last mix.
    assert_eq!(stage_series(&g.stages[4]), vec!["ACD_TMSmallHall"]);
}

#[test]
fn build_stages_single_split_pre_and_tail() {
    // One split: G1 pre-series, G2/G3 lanes, G4..G7 post-series tail. Real
    // payloads carry all three split controls regardless of active template.
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrParallel1",
            "splitMix":{"splitPoints":[{"nodeId":"split1"},{"nodeId":"split2"},{"nodeId":"split3"}],
                        "mixPoints":[{"nodeId":"mix1"},{"nodeId":"mix2"},{"nodeId":"mix3"}]},
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_KlonCentaur"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe"}],
                "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"}],
                "G4":[{"FenderId":"ACD_SpaceEcho"}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.stages.len(), 3);
    assert!(
        matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_KlonCentaur")
    );
    assert!(matches!(&g.stages[1], Stage::Split { a, b }
        if a[0].model == "ACD_TweedDeluxe" && b[0].model == "ACD_PrincetonReverb65NoFx"));
    assert!(matches!(&g.stages[2], Stage::Series { blocks } if blocks[0].model == "ACD_SpaceEcho"));
}

#[test]
fn build_stages_series_when_no_split_points() {
    // A series preset: no splitPoints → one series run of all blocks in order.
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrSeries",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_TubeScreamer"},{"FenderId":"ACD_PlexiAmp"}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.stages.len(), 1);
    assert!(matches!(&g.stages[0], Stage::Series { blocks } if blocks.len() == 3));
}

#[test]
fn route_graph_dual_inputs_join_before_series_tail() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrMicSeries",
            "guitarNodes":{"G1":[{"FenderId":"ACD_Compressor"}]},
            "micNodes":{"M1":[{"FenderId":"ACD_StudioPreamp"}]}
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);

    let inputs = g.inputs.expect("gtrMicSeries should expose a dual input");
    assert_eq!(inputs.a.kind, "guitar");
    assert_eq!(inputs.b.kind, "mic");
    assert!(inputs.a.blocks.is_empty());
    assert!(inputs.b.blocks.is_empty());
    assert_eq!(g.input_type, None);
    assert_eq!(g.output_type.as_deref(), Some("out"));
    assert!(matches!(&g.stages[0], Stage::Series { blocks }
        if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
            == vec!["ACD_Compressor", "ACD_StudioPreamp"]));
}

#[test]
fn route_graph_gtr_mic_parallel_uses_independent_rails() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrMicParallel",
            "guitarNodes":{"G1":[{"FenderId":"ACD_Compressor"}]},
            "micNodes":{"M1":[{"FenderId":"ACD_StudioPreamp"}]}
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);

    assert!(g.stages.is_empty());
    assert_eq!(g.input_type, None);
    assert_eq!(g.output_type, None);
    let lanes = g.lanes.expect("parallel gtr/mic should expose rails");
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0].input, "guitar");
    assert_eq!(lanes[0].output, "out1");
    assert_eq!(lanes[0].blocks[0].model, "ACD_Compressor");
    assert_eq!(lanes[1].input, "mic");
    assert_eq!(lanes[1].output, "out2");
    assert_eq!(lanes[1].blocks[0].model, "ACD_StudioPreamp");
}

#[test]
fn route_graph_split_outputs_label_out1_and_out2() {
    let gtr: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrSplit",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_Compressor"}],
                "G2":[{"FenderId":"ACD_DeluxeReverb"}],
                "G3":[{"FenderId":"ACD_SpaceEcho"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&gtr, None);

    assert_eq!(g.output_type, None);
    assert!(
        matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_Compressor")
    );
    let outputs = g.outputs.expect("gtrSplit should expose split outputs");
    assert_eq!(outputs.a.kind, "out1");
    assert_eq!(outputs.a.blocks[0].model, "ACD_DeluxeReverb");
    assert_eq!(outputs.b.kind, "out2");
    assert_eq!(outputs.b.blocks[0].model, "ACD_SpaceEcho");

    let mic: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"micSplit",
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M2":[{"FenderId":"ACD_LA2AComp"}],
                "M3":[{"FenderId":"ACD_RoomVerb"}]
            }
        }}"#,
    )
    .unwrap();
    let m = extract_active_graph(&mic, None);
    assert_eq!(m.input_type.as_deref(), Some("mic"));
    let m_outputs = m.outputs.expect("micSplit should expose split outputs");
    assert_eq!(m_outputs.a.kind, "out1");
    assert_eq!(m_outputs.b.kind, "out2");
}

// HW-confirmed (My Presets slot 27 "Split outputs", 65 Deluxe Reverb → SPLIT):
// a device group is itself an ordered mini-chain that can hold multiple blocks
// (adding a 2nd effect in Pro Control landed inside G2's own array, not G4), so
// gtrSplit/micSplit assign ONE WHOLE GROUP per output lane (G2→out1, G3→out2),
// not a bunched multi-group half like the old ["G2","G3","G4"]/["G5","G6","G7"].
#[test]
fn route_graph_gtr_split_lane_is_one_full_group_in_order() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrSplit",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_DeluxeReverb65NoFx"}],
                "G2":[{"FenderId":"ACD_UserIRTMS"},{"FenderId":"ACD_TMSmallHall"}],
                "G3":[{"FenderId":"ACD_ExternalCab"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    let outputs = g.outputs.expect("gtrSplit should expose split outputs");
    assert_eq!(outputs.a.kind, "out1");
    assert_eq!(
        outputs
            .a
            .blocks
            .iter()
            .map(|n| n.model.clone())
            .collect::<Vec<_>>(),
        vec!["ACD_UserIRTMS", "ACD_TMSmallHall"]
    );
    assert_eq!(outputs.b.kind, "out2");
    assert_eq!(outputs.b.blocks[0].model, "ACD_ExternalCab");
}

#[test]
fn route_graph_mic_series_runs_all_groups_in_series() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"micSeries",
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M2":[{"FenderId":"ACD_LA2AComp"}],
                "M3":[{"FenderId":"ACD_RoomVerb"}],
                "M4":[{"FenderId":"ACD_TMSmallHall"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.input_type.as_deref(), Some("mic"));
    assert_eq!(g.stages.len(), 1);
    assert!(matches!(&g.stages[0], Stage::Series { blocks }
        if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
            == vec!["ACD_StudioPreamp", "ACD_LA2AComp", "ACD_RoomVerb", "ACD_TMSmallHall"]));
}

#[test]
fn route_graph_mic_parallel1_splits_m2_m3_around_m1_and_m4() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"micParallel1",
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M2":[{"FenderId":"ACD_LA2AComp"}],
                "M3":[{"FenderId":"ACD_RoomVerb"}],
                "M4":[{"FenderId":"ACD_TMSmallHall"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.stages.len(), 3);
    assert!(
        matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_StudioPreamp")
    );
    assert!(matches!(&g.stages[1], Stage::Split { a, b }
        if a[0].model == "ACD_LA2AComp" && b[0].model == "ACD_RoomVerb"));
    assert!(
        matches!(&g.stages[2], Stage::Series { blocks } if blocks[0].model == "ACD_TMSmallHall")
    );
}

// gtrParallel1 with EVERY group multi/populated (G1/G2 two blocks each, G3 one,
// G4..G7 all populated) — the direct regression guard for the old bug's
// tail-concat path silently bunching/dropping groups beyond the split lanes.
#[test]
fn build_stages_single_split_multi_block_groups_and_full_tail() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrParallel1",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe"},{"FenderId":"ACD_KlonCentaur"}],
                "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"}],
                "G4":[{"FenderId":"ACD_SpaceEcho"}],
                "G5":[{"FenderId":"ACD_TMSmallHall"}],
                "G6":[{"FenderId":"ACD_MicroTronIV"}],
                "G7":[{"FenderId":"ACD_RoomVerb"}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.stages.len(), 3);
    assert!(matches!(&g.stages[0], Stage::Series { blocks }
        if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
            == vec!["ACD_Compressor", "ACD_EPBooster"]));
    assert!(matches!(&g.stages[1], Stage::Split { a, b }
        if a.iter().map(|x| x.model.as_str()).collect::<Vec<_>>() == vec!["ACD_TweedDeluxe", "ACD_KlonCentaur"]
            && b.iter().map(|x| x.model.as_str()).collect::<Vec<_>>() == vec!["ACD_PrincetonReverb65NoFx"]));
    assert!(matches!(&g.stages[2], Stage::Series { blocks }
        if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
            == vec!["ACD_SpaceEcho", "ACD_TMSmallHall", "ACD_MicroTronIV", "ACD_RoomVerb"]));
}

// Same shape as `build_stages_dual_split_assigns_roles` but every group carries
// 2 blocks — proves a populated LANE (not just a populated group) preserves its
// own internal per-group order.
#[test]
fn build_stages_dual_split_multi_block_per_group() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrParallel2",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_EPBooster"},{"FenderId":"ACD_Compressor"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe"},{"FenderId":"ACD_KlonCentaur"}],
                "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"},{"FenderId":"ACD_StudioPreamp"}],
                "G4":[{"FenderId":"ACD_SpaceEcho"},{"FenderId":"ACD_RoomVerb"}],
                "G5":[{"FenderId":"ACD_TMSmallHall"},{"FenderId":"ACD_LA2AComp"}],
                "G6":[{"FenderId":"ACD_MicroTronIV"},{"FenderId":"ACD_UserIRTMS"}],
                "G7":[{"FenderId":"ACD_TMSmallHall"},{"FenderId":"ACD_ExternalCab"}]
            }}}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert_eq!(g.stages.len(), 5);
    assert_eq!(
        stage_series(&g.stages[0]),
        vec!["ACD_EPBooster", "ACD_Compressor"]
    );
    assert_eq!(
        stage_split(&g.stages[1]),
        (
            vec!["ACD_TweedDeluxe".into(), "ACD_KlonCentaur".into()],
            vec![
                "ACD_PrincetonReverb65NoFx".into(),
                "ACD_StudioPreamp".into()
            ]
        )
    );
    assert_eq!(
        stage_series(&g.stages[2]),
        vec!["ACD_SpaceEcho", "ACD_RoomVerb"]
    );
    assert_eq!(
        stage_split(&g.stages[3]),
        (
            vec!["ACD_TMSmallHall".into(), "ACD_LA2AComp".into()],
            vec!["ACD_MicroTronIV".into(), "ACD_UserIRTMS".into()]
        )
    );
    assert_eq!(
        stage_series(&g.stages[4]),
        vec!["ACD_TMSmallHall", "ACD_ExternalCab"]
    );
}

#[test]
fn route_graph_dual_inputs_join_multi_block_per_group() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrMicSeries",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe"}]
            },
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M2":[{"FenderId":"ACD_LA2AComp"},{"FenderId":"ACD_RoomVerb"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert!(matches!(&g.stages[0], Stage::Series { blocks }
        if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
            == vec!["ACD_Compressor", "ACD_EPBooster", "ACD_TweedDeluxe",
                    "ACD_StudioPreamp", "ACD_LA2AComp", "ACD_RoomVerb"]));
}

// The three gtrMicMix aliases had ZERO coverage before this test — they carry
// NO stages at all (unlike gtrMicSeries), just the two full input rails.
#[test]
fn route_graph_gtr_mic_mix_aliases_expose_full_groups_as_inputs() {
    for template in ["gtrMicMix", "gtrMicMix2", "gtrMicMix3"] {
        let v: serde_json::Value = serde_json::from_str(&format!(
            r#"{{"audioGraph":{{
                "template":"{template}",
                "guitarNodes":{{"G1":[{{"FenderId":"ACD_Compressor"}}]}},
                "micNodes":{{"M1":[{{"FenderId":"ACD_StudioPreamp"}}]}}
            }}}}"#
        ))
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert!(
            g.stages.is_empty(),
            "{template} should carry no stages, only inputs"
        );
        let inputs = g.inputs.expect("should expose dual inputs");
        assert_eq!(inputs.a.kind, "guitar");
        assert_eq!(inputs.a.blocks[0].model, "ACD_Compressor");
        assert_eq!(inputs.b.kind, "mic");
        assert_eq!(inputs.b.blocks[0].model, "ACD_StudioPreamp");
        assert_eq!(g.outputs, None);
        assert_eq!(g.lanes, None);
    }
}

#[test]
fn route_graph_gtr_mic_parallel_multi_block_rails() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"gtrMicParallel",
            "guitarNodes":{
                "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                "G2":[{"FenderId":"ACD_TweedDeluxe"}]
            },
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M3":[{"FenderId":"ACD_RoomVerb"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    let lanes = g.lanes.expect("gtrMicParallel should expose rails");
    assert_eq!(
        lanes[0]
            .blocks
            .iter()
            .map(|b| b.model.clone())
            .collect::<Vec<_>>(),
        vec!["ACD_Compressor", "ACD_EPBooster", "ACD_TweedDeluxe"]
    );
    assert_eq!(
        lanes[1]
            .blocks
            .iter()
            .map(|b| b.model.clone())
            .collect::<Vec<_>>(),
        vec!["ACD_StudioPreamp", "ACD_RoomVerb"]
    );
}

// The micSplit sibling of `route_graph_gtr_split_lane_is_one_full_group_in_order`
// — the same asymmetric multi-block-per-lane guard, applied to the ONE split
// template that never got it (its own existing test only covered a
// single-block-per-lane shape, which is exactly the shape that hid the
// original bug on gtrSplit until a real preset exposed it).
#[test]
fn route_graph_mic_split_lane_is_one_full_group_in_order() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"audioGraph":{
            "template":"micSplit",
            "micNodes":{
                "M1":[{"FenderId":"ACD_StudioPreamp"}],
                "M2":[{"FenderId":"ACD_LA2AComp"},{"FenderId":"ACD_RoomVerb"}],
                "M3":[{"FenderId":"ACD_TMSmallHall"}]
            }
        }}"#,
    )
    .unwrap();
    let g = extract_active_graph(&v, None);
    assert!(
        matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_StudioPreamp")
    );
    let outputs = g.outputs.expect("micSplit should expose split outputs");
    assert_eq!(outputs.a.kind, "out1");
    assert_eq!(
        outputs
            .a
            .blocks
            .iter()
            .map(|n| n.model.clone())
            .collect::<Vec<_>>(),
        vec!["ACD_LA2AComp", "ACD_RoomVerb"]
    );
    assert_eq!(outputs.b.kind, "out2");
    assert_eq!(outputs.b.blocks[0].model, "ACD_TMSmallHall");
}

#[test]
fn push_split_drops_stage_only_when_both_lanes_empty() {
    let mut stages: Vec<Stage> = Vec::new();
    push_split(&mut stages, Vec::new(), Vec::new());
    assert!(
        stages.is_empty(),
        "an all-empty split slot pair must not draw an empty fork/mix"
    );

    let node = GraphNode {
        group_id: "G3".to_string(),
        node_id: "n1".to_string(),
        model: "ACD_SpaceEcho".to_string(),
        bypassed: false,
        cab_sim_id: None,
        cab_sim_id2: None,
        cab_sim2_enabled: None,
        params: std::collections::HashMap::new(),
    };
    push_split(&mut stages, Vec::new(), vec![node]);
    assert_eq!(
        stages.len(),
        1,
        "one populated lane must still draw the split"
    );
}

// Build a synthetic PresetListResponse and confirm extraction. This pins the
// descent logic (TMS→preset→listResponse→records→displayName) without hardware.
#[test]
fn extract_preset_list_parses_records() {
    // Construct from the inside out using the same wire helpers the encoder uses.
    fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        o.push(((field << 3) | 2) as u8); // field<16, single-byte tag
        o.push(inner.len() as u8);
        o.extend_from_slice(inner);
        o
    }
    fn str_field1(s: &str) -> Vec<u8> {
        ld(1, s.as_bytes())
    }
    // PresetListResponse{ listEnum=1 (field1 varint), record="65 Deluxe", record="Solo Lead" }
    let mut resp = vec![0x08, 0x01]; // field1 varint listEnum=1
    resp.extend(ld(2, &str_field1("65 Deluxe")));
    resp.extend(ld(2, &str_field1("Solo Lead")));
    // presetMessage[5] = PresetListResponse
    let preset_msg = ld(PRESET_LIST_RESPONSE, &resp);
    // FenderMessageTMS[2] = presetMessage
    let tms = ld(TMS_PRESET, &preset_msg);

    let names = extract_preset_list(&tms).expect("should parse");
    assert_eq!(
        names,
        vec!["65 Deluxe".to_string(), "Solo Lead".to_string()]
    );
}

// The tolerant extractor must also reject other lists' responses: after a
// flooded handshake truncated the My-Presets reply, the complete FACTORY
// reply (listEnum=4, 249 records) was served as My Presets — silently, with
// factory names in the Level tab.
#[test]
fn extract_preset_list_rejects_other_list_enums() {
    fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        o.push(((field << 3) | 2) as u8);
        o.push(inner.len() as u8);
        o.extend_from_slice(inner);
        o
    }
    let body = |list_enum: u8| {
        let mut resp = vec![0x08, list_enum];
        resp.extend(ld(2, &ld(1, b"'65 Deluxe Reverb")));
        ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
    };
    assert!(extract_preset_list(&body(4)).is_none()); // Factory
    assert!(extract_preset_list(&body(3)).is_none()); // Cloud
    assert_eq!(
        extract_preset_list(&body(1)),
        Some(vec!["'65 Deluxe Reverb".to_string()])
    );
}

// Longest-wins must never let a LONGER Factory response beat My Presets —
// the incident shape (truncated 504-record My-Presets read + complete
// 249-record Factory read in the same accumulated reports).
#[test]
fn best_preset_list_ignores_a_longer_factory_response() {
    fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        o.push(((field << 3) | 2) as u8);
        o.push(inner.len() as u8);
        o.extend_from_slice(inner);
        o
    }
    fn report(body: &[u8]) -> Vec<u8> {
        let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
        r.extend_from_slice(body);
        r
    }
    let list = |list_enum: u8, names: &[&str]| {
        let mut resp = vec![0x08, list_enum];
        for n in names {
            resp.extend(ld(2, &ld(1, n.as_bytes())));
        }
        ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
    };
    let reports = vec![
        report(&list(4, &["'65 Deluxe", "Unchain Ed", "Cutting"])), // Factory, longer
        report(&list(1, &["Guitar", "Cello"])),                     // My Presets
    ];
    let best = best_preset_list_from_reports(&reports).expect("my presets");
    assert_eq!(best, vec!["Guitar".to_string(), "Cello".to_string()]);
}

// The field-9-polluted rig a load harvest must never create: carrier (9, 3) is the
// field-8 slot-read reply's OWN payload, and `latest_json_payload` takes the LATEST of its
// three carriers with no other tiebreak. A session whose accumulated reports carry a fresh
// field-3 live push and a stray field-9 reply after it will silently read the field-9
// content — why `rich_load_collect` clears `raw` before every harvest.
fn len_delim(field: u32, inner: &[u8]) -> Vec<u8> {
    let mut o = Vec::new();
    o.push(((field << 3) | 2) as u8);
    o.push(inner.len() as u8);
    o.extend_from_slice(inner);
    o
}
fn json_report(body: &[u8]) -> Vec<u8> {
    let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
    r.extend_from_slice(body);
    r
}
/// A `currentPresetDataChanged`(3) → presetJson(1) push of `json`, as one report.
fn field3_push(json: &[u8]) -> Vec<u8> {
    json_report(&len_delim(TMS_PRESET, &len_delim(3, &len_delim(1, json))))
}

#[test]
fn latest_json_payload_from_reports_takes_a_later_field9_reply() {
    let fresh_json = b"FRESH_LIVE_PUSH".to_vec();
    let stray_json = b"STRAY_FIELD9".to_vec();
    // Healthy session: only the fresh currentPresetDataChanged(3)->presetJson(1) push.
    let clean = vec![field3_push(&fresh_json)];
    assert_eq!(latest_json_payload_from_reports(&clean), fresh_json);
    // Polluted session: the same fresh push, THEN a stray presetDataChanged(9)->…(3) reply
    // (the field-8 carrier) — shorter, and it still wins.
    let polluted = vec![
        field3_push(&fresh_json),
        json_report(&len_delim(
            TMS_PRESET,
            &len_delim(9, &len_delim(3, &stray_json)),
        )),
    ];
    assert_eq!(
        latest_json_payload_from_reports(&polluted),
        stray_json,
        "latest-wins picked the stray field-9 reply over the earlier field-3 push"
    );
}

/// BUG→GATE (online e2e `doctor-apply.online.spec.ts`, the post-discard working-copy read;
/// HW 2026-10-14, fw 1.8.58, 3/3 at a 100 ms settle): a session opened inside the device's
/// inactivity drop receives the previous session's queued replies FIRST — last of them that
/// session's handshake copy of the working copy BEFORE its loads, here the applied block a
/// Doctor discard had just thrown away. The unit's carrier sizes were 2871 (stale, one block
/// more) → 2451 (the restore's neighbour) → 2691 → 2691 (the restored copy). Longest-wins
/// read the stale one; the latest copy is the restored one.
#[test]
fn latest_json_payload_reads_past_a_longer_inherited_copy() {
    let stale = b"STALE_WORKING_COPY_WITH_THE_APPLIED_EQ".to_vec();
    let neighbour = b"NEIGHBOUR_SLOT".to_vec();
    let restored = b"RESTORED_COPY".to_vec();
    assert!(stale.len() > restored.len() && stale.len() > neighbour.len());
    let reports = vec![
        field3_push(&stale),
        field3_push(&neighbour),
        field3_push(&restored),
        field3_push(&restored),
    ];
    assert_eq!(latest_json_payload_from_reports(&reports), restored);
    // A heartbeat reply after the push carries no preset JSON and changes nothing.
    let mut with_beat = reports.clone();
    with_beat.push(json_report(&len_delim(4, &len_delim(5, b""))));
    assert_eq!(latest_json_payload_from_reports(&with_beat), restored);
}

// The Factory harvest gates on listEnum == 4: it must pick the Factory reply
// out of the shared accumulator and ignore the My-Presets (listEnum 1) reply,
// even when My Presets is longer.
#[test]
fn best_factory_list_extracts_list_enum_4() {
    fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        o.push(((field << 3) | 2) as u8);
        o.push(inner.len() as u8);
        o.extend_from_slice(inner);
        o
    }
    fn report(body: &[u8]) -> Vec<u8> {
        let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
        r.extend_from_slice(body);
        r
    }
    let list = |list_enum: u8, names: &[&str]| {
        let mut resp = vec![0x08, list_enum];
        for n in names {
            resp.extend(ld(2, &ld(1, n.as_bytes())));
        }
        ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
    };
    let reports = vec![
        report(&list(1, &["Guitar", "Cello", "Synth"])), // My Presets, longer
        report(&list(4, &["'65 Deluxe", "Cutting"])),    // Factory
    ];
    let factory = best_factory_list_from_reports(&reports).expect("factory list");
    assert_eq!(
        factory,
        vec!["'65 Deluxe".to_string(), "Cutting".to_string()]
    );
    // And a reports set with NO factory reply yields nothing.
    assert!(best_factory_list_from_reports(&[report(&list(1, &["Guitar"]))]).is_none());
}

#[test]
fn preset_list_uses_terminal_35_tail_frame() {
    fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        o.push(((field << 3) | 2) as u8);
        o.push(inner.len() as u8);
        o.extend_from_slice(inner);
        o
    }
    fn str_field1(s: &str) -> Vec<u8> {
        ld(1, s.as_bytes())
    }
    fn report(magic: u8, body: &[u8]) -> Vec<u8> {
        let mut r = vec![0x00, magic, 0x00, body.len() as u8];
        r.extend_from_slice(body);
        r
    }

    let mut resp = vec![0x08, 0x01];
    resp.extend(ld(2, &str_field1("Preset")));
    resp.extend(ld(2, &str_field1("Empty")));
    let tms = ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp));

    let split = tms.len() - 1;
    let reports = vec![report(0x33, &tms[..split]), report(0x35, &tms[split..])];

    let streams_only = proto::reassemble_streams(&reports)
        .into_iter()
        .find_map(|s| extract_preset_list(&s.body))
        .expect("tolerant streams decode");
    assert_eq!(streams_only.last().map(String::as_str), Some("Empt"));

    let best = best_preset_list_from_reports(&reports).expect("best list");
    assert_eq!(best, vec!["Preset".to_string(), "Empty".to_string()]);
}

#[test]
fn dig_returns_none_for_absent_branch() {
    let body = [0x08, 0x01]; // a bare varint, no presetMessage
    assert!(dig(&body, TMS_PRESET, PRESET_LIST_RESPONSE).is_none());
}

// The currentFwResponse frame: settingsMessage(3) → currentFwResponse(2)
// → data(1) = "1.7.75". Byte layout matches the frame observed live
// (device on 1.7.75): 1a 0a 12 08 0a 06 31 2e 37 2e 37 35.
#[test]
fn extract_fw_version_parses_current_fw_response() {
    let body = [
        0x1a, 0x0a, // settingsMessage, len 10
        0x12, 0x08, // currentFwResponse, len 8
        0x0a, 0x06, b'1', b'.', b'7', b'.', b'7', b'5', // data = "1.7.75"
    ];
    assert_eq!(extract_fw_version(&body).as_deref(), Some("1.7.75"));
}

#[test]
fn extract_fw_version_ignores_other_settings_frames() {
    // settingsMessage carrying reampModeActive (field 30), not currentFwResponse.
    let body = [0x1a, 0x05, 0xf2, 0x01, 0x02, 0x08, 0x01];
    assert_eq!(extract_fw_version(&body), None);
    // currentFwResponse with an empty data string → None, not Some("").
    let empty = [0x1a, 0x04, 0x12, 0x02, 0x0a, 0x00];
    assert_eq!(extract_fw_version(&empty), None);
}

#[test]
fn is_level_param_includes_levels_excludes_gain() {
    assert!(super::is_level_param("outputLevel"));
    assert!(super::is_level_param("volume"));
    assert!(super::is_level_param("masterVolume"));
    assert!(!super::is_level_param("gain"));
    assert!(!super::is_level_param("driveLevel")); // drive wins → excluded
    assert!(!super::is_level_param("bass"));
}

#[test]
fn extract_level_blocks_filters_to_level_controls() {
    let json = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            {
                "nodeId": "ACD_TwinReverb65NoFx",
                "dspUnitParameters": {
                    "outputLevel": 0.29, "gain": 0.43,
                    "bass": 0.22, "bypass": false, "gatePreset": "off"
                }
            },
            { "nodeId": "VOL_Pedal", "dspUnitParameters": { "volume": 0.8 } }
        ] } }
    });
    let mut got = super::extract_level_blocks(&json);
    got.sort_by(|a, b| a.parameter_id.cmp(&b.parameter_id));
    assert_eq!(got.len(), 2, "got {got:?}");
    assert_eq!(got[0].parameter_id, "outputLevel");
    assert_eq!(got[0].node_id, "ACD_TwinReverb65NoFx");
    assert_eq!(got[0].group_id, "G1");
    assert!((got[0].value - 0.29).abs() < 1e-6);
    assert_eq!(got[1].parameter_id, "volume");
}

#[test]
fn extract_level_blocks_tolerates_missing_graph() {
    assert!(super::extract_level_blocks(&serde_json::json!({})).is_empty());
}

// BUG→GATE: the preset lane's PICKER used to run the name-substring rule, which
// disagreed with the run-time class gate — it offered `ACD_TMRumbleV3.level` (an amp
// knob the solve then refuses) and hid the raw-dB `ACD_Boost.gain` the footswitch and
// scene-handle pickers offer. One classifier for every picker. (Issue 3, later: `mix`
// itself became excluded from every picker's safe-default list too — see the test body.)
#[test]
fn extract_level_candidates_uses_the_classifier_not_the_name_rule() {
    let json = serde_json::json!({
        "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": "bass", "FenderId": "ACD_TMRumbleV3",
              "dspUnitParameters": { "level": 0.5, "volume": 0.6 } },
            { "nodeId": "boost", "FenderId": "ACD_Boost",
              "dspUnitParameters": { "gain": 2.5 } },
            { "nodeId": "chorus", "FenderId": "ACD_Chorus",
              "dspUnitParameters": { "mix": 0.4 } }
        ] } }
    });
    let got: Vec<(String, String)> = super::extract_level_candidates(&json)
        .into_iter()
        .map(|b| (b.node_id, b.parameter_id))
        .collect();
    assert!(
        !got.iter().any(|(n, _)| n == "bass"),
        "`level` on ACD_TMRumbleV3 is an amp knob and `volume` on an amp is the breakup \
         knob — neither may be offered: {got:?}"
    );
    assert!(
        got.contains(&("boost".to_string(), "gain".to_string())),
        "ACD_Boost.gain is raw dB with ~1:1 authority — the name rule hid it: {got:?}"
    );
    // Issue 3 ("hide Mix everywhere"): a wet/mix control is EXCLUDED from this
    // safe-default candidate list, even though the classifier still recognises it
    // (floored, not gutted, when explicitly named elsewhere) — see
    // `footswitch::is_levelable_param`'s doc.
    assert!(
        !got.iter().any(|(n, p)| n == "chorus" && p == "mix"),
        "issue 3: Mix is never a default/pickable candidate: {got:?}"
    );
    assert_eq!(
        crate::param_class::classify("ACD_Chorus", "mix").class,
        crate::param_class::ParamClass::WetMix,
        "…the classifier's own verdict is untouched"
    );
    // The classifier still reads the block's FenderId, so the raw-dB value passes through
    // unscaled (the old `(0.0..=1.0)` value filter would have dropped 2.5).
    let boost = super::extract_level_candidates(&json)
        .into_iter()
        .find(|b| b.node_id == "boost")
        .expect("boost candidate");
    assert!((boost.value - 2.5).abs() < 1e-6);
    assert_eq!(boost.model_id, "ACD_Boost");
}

#[test]
fn extract_level_candidates_tolerates_missing_graph() {
    assert!(super::extract_level_candidates(&serde_json::json!({})).is_empty());
}

// The real wire case: a preset JSON truncated mid-object after audioGraph
// (the device sends a partial in the change event). Tolerant parsing must
// still recover audioGraph.guitarNodes.
#[test]
fn tolerant_parse_recovers_truncated_audiograph() {
    let truncated = r#"{"ampControl":{"a":false},"audioGraph":{"guitarNodes":{"G1":[{"nodeId":"ACD_TM59Bassman","dspUnitParameters":{"outputLevel":0.5}}]}},"instrumentOutput":{"out1":true}},"scenes"#;
    assert!(
        serde_json::from_str::<serde_json::Value>(truncated).is_err(),
        "should be invalid"
    );
    let v = super::tolerant_parse_json(truncated).expect("tolerant parse recovers it");
    let blocks = super::extract_level_blocks(&v);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].node_id, "ACD_TM59Bassman");
    assert_eq!(blocks[0].parameter_id, "outputLevel");
}

#[test]
fn tolerant_parse_unwinds_nested_array_object_order() {
    let truncated = r#"{"audioGraph":{"splitMix":{"splitPoints":[{"parameters":{"levelA":1.0,"x"#;
    let v = super::tolerant_parse_json(truncated).expect("nested partial recovers");
    assert_eq!(
        v["audioGraph"]["splitMix"]["splitPoints"][0]["parameters"]["levelA"],
        1.0
    );
}

#[test]
fn tolerant_parse_passes_through_valid_json() {
    let v = super::tolerant_parse_json(r#"{"a":1,"b":[2,3]}"#).unwrap();
    assert_eq!(v["a"], 1);
}

/// The silent-wrong-plan class: a large preset's field-8 read comes back
/// TAIL-TRUNCATED, `tolerant_parse_json` salvages the prefix, and a planner reading
/// the salvaged `ftsw` sees "no footswitches" on a preset that has them. The
/// discriminator must separate a section that never ARRIVED from one the preset
/// legitimately carries empty — a preset really can have 20 empty `ftsw` rows, and a
/// real device export has been seen with 9 rows rather than 20, so neither
/// emptiness nor row count can be the test.
#[test]
fn json_section_complete_separates_a_truncated_tail_from_a_genuinely_empty_section() {
    let body = r#"{"ampControl":{"a":false},"audioGraph":{"guitarNodes":{"G1":[{"nodeId":"N"}]}},"ftsw":[[{"func":"on-off","customLabel":"BOOST"}],[]],"info":{"displayName":"Big"},"scenes":[{"sceneName":"A"}]}"#;
    assert!(super::json_section_complete(body, "ftsw"));
    assert!(super::json_section_complete(body, "scenes"));

    // Cut INSIDE `scenes` — the tail is gone but `ftsw` already arrived whole, so
    // footswitch planning off this body is sound and must not be refused.
    let cut = body.find(r#""scenes":[{"sceneName"#).expect("cut point");
    let mid_scenes = &body[..cut + r#""scenes":[{"sceneN"#.len()];
    assert!(super::json_section_complete(mid_scenes, "ftsw"));
    assert!(!super::json_section_complete(mid_scenes, "scenes"));

    // Cut INSIDE `ftsw` — the salvaged array is a plausible-looking prefix, which is
    // exactly the shape that used to plan zero jobs silently.
    let cut = body.find(r#","customLabel"#).expect("cut point");
    let mid_ftsw = &body[..cut];
    assert!(super::tolerant_parse_json(mid_ftsw).is_some());
    assert!(!super::json_section_complete(mid_ftsw, "ftsw"));

    // Cut BEFORE `ftsw` — the key never appears at all.
    let cut = body.find(r#","ftsw""#).expect("cut point");
    assert!(!super::json_section_complete(&body[..cut], "ftsw"));

    // A COMPLETE body whose sections are empty is legitimate, not truncated: 20
    // empty rows is a real preset (no footswitch assignments), and `"scenes":[]` is
    // the device shape for a scene-less preset.
    let rows = vec!["[]"; 20].join(",");
    let empty = format!(r#"{{"ftsw":[{rows}],"scenes":[]}}"#);
    assert!(super::json_section_complete(&empty, "ftsw"));
    assert!(super::json_section_complete(&empty, "scenes"));

    // The key appearing as a string VALUE is not the section — a body whose only
    // "ftsw" is a footswitch label must still read as truncated.
    let labelled = r#"{"audioGraph":{"guitarNodes":{"G1":[{"customLabel":"ftsw"#;
    assert!(!super::json_section_complete(labelled, "ftsw"));
}

// ─── Live-push decoders (shared by the monitor + the listener) ────────────

/// Encode a protobuf varint into `out` (test helper — proto's encoders are
/// module-private, so the push-decoder tests hand-build their input frames).
fn put_varint_t(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}
fn field_varint_t(out: &mut Vec<u8>, field: u32, value: u64) {
    put_varint_t(out, (field as u64) << 3); // wire type 0
    put_varint_t(out, value);
}
fn field_bytes_t(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    put_varint_t(out, ((field as u64) << 3) | 2);
    put_varint_t(out, value.len() as u64);
    out.extend_from_slice(value);
}

/// Build a `FenderMessageTMS{ presetMessage[2]{ <field>{ <inner bytes> } } }`.
fn tms_preset(inner_field: u32, inner: &[u8]) -> Vec<u8> {
    let mut pm = Vec::new();
    field_bytes_t(&mut pm, inner_field, inner);
    let mut o = Vec::new();
    field_bytes_t(&mut o, 2, &pm); // presetMessage = TMS field 2
    o
}

#[test]
fn decode_preset_loaded_reads_all_addressing_fields() {
    // PresetLoaded{ tab=1, setlist=2, setlistSong=3, song=4, songPreset=5,
    //   presetSlot=11, sync=1 } — build inner with field_varint, decode back.
    let mut inner = Vec::new();
    for (f, v) in [
        (1u32, 1u64),
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 5),
        (6, 11),
        (7, 1),
    ] {
        field_varint_t(&mut inner, f, v);
    }
    let body = tms_preset(11, &inner);
    let pl = super::decode_preset_loaded(&body).expect("is a PresetLoaded");
    assert_eq!(pl.tab_enum, 1);
    assert_eq!(pl.setlist_slot, 2);
    assert_eq!(pl.song_slot, 4);
    assert_eq!(pl.preset_slot, 11);
    assert_eq!(pl.sync, 1);
    // A non-PresetLoaded body decodes to None.
    assert!(super::decode_preset_loaded(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_info_changed_reads_name_dirty_favorite() {
    // CurrentPresetInfoChanged{ displayName="Lead", isDirty=1, isFavorite=0 }.
    let mut inner = Vec::new();
    field_bytes_t(&mut inner, 1, b"Lead");
    field_varint_t(&mut inner, 2, 1);
    // isFavorite omitted (proto3 false default) → should decode as false.
    let body = tms_preset(22, &inner);
    let (name, dirty, fav) = super::decode_info_changed(&body).expect("is InfoChanged");
    assert_eq!(name, "Lead");
    assert!(dirty);
    assert!(!fav);
    // With isFavorite=1 set explicitly.
    let mut inner2 = Vec::new();
    field_bytes_t(&mut inner2, 1, b"Clean");
    field_varint_t(&mut inner2, 3, 1);
    let (name2, dirty2, fav2) = super::decode_info_changed(&tms_preset(22, &inner2)).unwrap();
    assert_eq!(name2, "Clean");
    assert!(!dirty2);
    assert!(fav2);
    assert!(super::decode_info_changed(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_scene_loaded_reads_slot_and_scene_name() {
    // SceneLoaded{ sceneSlot=2, sceneJson=LZ4("{\"sceneName\":\"Verse\"}") }.
    let json = br#"{"sceneName":"Verse","other":1}"#;
    let lz4 = proto::lz4_block_compress_stored(json);
    let mut inner = Vec::new();
    field_varint_t(&mut inner, 1, 2);
    field_bytes_t(&mut inner, 2, &lz4);
    let body = tms_preset(102, &inner);
    let (slot, name) = super::decode_scene_loaded(&body).expect("is SceneLoaded");
    assert_eq!(slot, 2);
    assert_eq!(name.as_deref(), Some("Verse"));
    // A SceneLoaded with no sceneJson → slot only, name None (handoff renders anyway).
    let mut inner2 = Vec::new();
    field_varint_t(&mut inner2, 1, 8);
    let (slot2, name2) = super::decode_scene_loaded(&tms_preset(102, &inner2)).unwrap();
    assert_eq!(slot2, 8);
    assert_eq!(name2, None);
    assert!(super::decode_scene_loaded(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_scene_list_reads_repeated_names() {
    // sceneListResponse{ dummy=1, sceneList=["Clean","Crunch","Lead"] }.
    let mut inner = Vec::new();
    field_varint_t(&mut inner, 1, 1); // dummy
    for n in ["Clean", "Crunch", "Lead"] {
        field_bytes_t(&mut inner, 2, n.as_bytes());
    }
    let body = tms_preset(125, &inner);
    let names = super::decode_scene_list(&body).expect("is sceneListResponse");
    assert_eq!(names, vec!["Clean", "Crunch", "Lead"]);
    // An empty scene list is a valid Some(vec![]) (preset with no FS scenes).
    let mut empty = Vec::new();
    field_varint_t(&mut empty, 1, 1);
    assert_eq!(
        super::decode_scene_list(&tms_preset(125, &empty)),
        Some(vec![])
    );
    assert!(super::decode_scene_list(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_current_preset_data_yields_active_graph_with_known_template() {
    // currentPresetDataChanged{ presetJson = LZ4(series preset JSON) }.
    let json = br#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{"G1":[{"FenderId":"ACD_TM59Bassman","dspUnitParameters":{"bypass":false}}]}}}"#;
    let lz4 = proto::lz4_block_compress_stored(json);
    let mut inner = Vec::new();
    field_bytes_t(&mut inner, 1, &lz4);
    let body = tms_preset(3, &inner);
    let g = super::decode_current_preset_data(&body).expect("known-template graph");
    assert_eq!(g.template.as_deref(), Some("gtrSeries"));
    assert_eq!(g.nodes.len(), 1);
    assert_eq!(g.nodes[0].model, "ACD_TM59Bassman");
    // A payload whose template is NOT a known routing template → None (guard).
    let bad = br#"{"audioGraph":{"template":"gtrUnkn","guitarNodes":{"G1":[{"FenderId":"X"}]}}}"#;
    let lz4bad = proto::lz4_block_compress_stored(bad);
    let mut innerb = Vec::new();
    field_bytes_t(&mut innerb, 1, &lz4bad);
    assert!(super::decode_current_preset_data(&tms_preset(3, &innerb)).is_none());
    // Not a currentPresetDataChanged → None.
    assert!(super::decode_current_preset_data(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_current_preset_live_reads_the_scene_trio_from_one_document() {
    // currentPresetDataChanged whose preset JSON carries scenes[].sceneName (slot
    // order), lastLoadedScene and ftsw — the live-scene trio in one document.
    let json = br#"{"ftsw":[[{"func":"scene","sceneSlot":1,"isActive":true}],[]],"lastLoadedScene":1,"scenes":[{"sceneName":"Clean","uuid":"a"},{"sceneName":"Lead","uuid":"b"}]}"#;
    let lz4 = proto::lz4_block_compress_stored(json);
    let mut inner = Vec::new();
    field_bytes_t(&mut inner, 1, &lz4);
    let live = super::decode_current_preset_live(&tms_preset(3, &inner)).expect("field-3");
    assert_eq!(live.scene_names, Some(vec!["Clean".into(), "Lead".into()]));
    assert_eq!(live.last_loaded_scene, Some(1));
    assert!(live.ftsw.is_some());
    // A truncated partial that lost the tail (no scenes/lastLoadedScene/ftsw)
    // still decodes — each half is independently None (the monitor keeps caches).
    let lean = br#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{}},"bpm":120}"#;
    let lz4lean = proto::lz4_block_compress_stored(lean);
    let mut innerl = Vec::new();
    field_bytes_t(&mut innerl, 1, &lz4lean);
    let live = super::decode_current_preset_live(&tms_preset(3, &innerl)).expect("field-3");
    assert_eq!(live.scene_names, None);
    assert_eq!(live.last_loaded_scene, None);
    assert!(live.ftsw.is_none());
    // Not a currentPresetDataChanged → None.
    assert!(super::decode_current_preset_live(&[0x08, 0x01]).is_none());
}

#[test]
fn decode_current_preset_live_survives_the_real_truncated_dump() {
    // The REAL Tier-4 capture (probe --dump-currentpresetdata, Guitar, 1.7.75):
    // a 17,263 B field-3 payload truncated mid-final-scene-`uuid`. The tolerant
    // parse must still recover all 8 slot-ordered names + lastLoadedScene(8 =
    // base) + the 10-switch ftsw. Fixture is gitignored (device-derived) — the
    // test auto-skips when absent (fresh worktree convention).
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/currentpresetdata-guitar-1.7.75.json");
    let Ok(text) = std::fs::read(&path) else {
        eprintln!("fixture missing — skipping ({})", path.display());
        return;
    };
    let lz4 = proto::lz4_block_compress_stored(&text);
    let mut inner = Vec::new();
    field_bytes_t(&mut inner, 1, &lz4);
    let live = super::decode_current_preset_live(&tms_preset(3, &inner)).expect("field-3");
    assert_eq!(
        live.scene_names.as_deref(),
        Some(
            &[
                "Arpeges",
                "Reverb",
                "Delay + Reverb",
                "Celestial",
                "Lofi",
                "Dist",
                "Swell",
                "Klon"
            ]
            .map(String::from)[..]
        )
    );
    assert_eq!(live.last_loaded_scene, Some(super::BASE_SCENE_SLOT));
    let map = crate::footswitch::scene_fs_map(&live.ftsw.expect("ftsw"));
    // Active assignments only (Lofi slot 4 + Swell slot 6 are isActive:false).
    assert_eq!(map.get(&0), Some(&5)); // Arpeges → switch 5 (FS6)
    assert_eq!(map.get(&5), Some(&7)); // Dist → switch 7 (FS8)
    assert_eq!(map.get(&4), None); // Lofi: inactive switch → em-dash
}

// Preset-list harvest gates (online e2e 2026-10-05/06: the seed read 473 / 496 / 459 of
// 504 as complete). `ScriptedTransport` + `my_presets_list_frames` script the device side.
use crate::test_support::{my_presets_list_frames, session_over, streamed, ScriptedTransport};

/// A reply that pauses one window with no growth yet must not pass the fast path —
/// only a strict (terminal-0x35) decode licenses it.
#[test]
fn list_my_presets_waits_out_a_one_window_stall_in_the_handshake_reply() {
    let frames = my_presets_list_frames(504);
    let cut = frames.len() - 9;
    let t = ScriptedTransport::default().with_pending(vec![Vec::new(), frames[cut..].to_vec()]);
    let mut s = session_over(&t, frames[..cut].to_vec());
    assert_eq!(s.list_my_presets().unwrap().len(), 504);
}

/// The hot path is unchanged: an already-complete handshake reply returns after ONE
/// silent window, with no heartbeat.
#[test]
fn list_my_presets_returns_a_complete_handshake_reply_after_one_window() {
    let t = ScriptedTransport::default();
    let mut s = session_over(&t, my_presets_list_frames(504));
    assert_eq!(s.list_my_presets().unwrap().len(), 504);
    assert_eq!(t.idle_ms(), 700, "exactly one silent window");
    assert_eq!(t.heartbeats(), 0);
}

/// The handshake's last write is ~300 ms back when the harvest starts, so one SILENT
/// 700 ms window crosses the device's inactivity drop mid-stream.
fn in_flight_handshake_reply() -> (ScriptedTransport, Session) {
    let frames = my_presets_list_frames(504);
    let t = ScriptedTransport::device()
        .with_idle(300)
        .with_pending(streamed(&frames[60..], 40));
    let s = session_over(&t, frames[..60].to_vec());
    (t, s)
}

#[test]
fn list_my_presets_keeps_the_session_alive_while_the_list_streams() {
    let (t, mut s) = in_flight_handshake_reply();
    assert_eq!(s.list_my_presets().unwrap().len(), 504);
    assert!(t.heartbeats() > 0);
}

/// The strict read waits out a handshake reply still in flight — heartbeating, so the
/// device doesn't drop it — instead of draining (which lapses the session and loses it)
/// and re-requesting.
#[test]
fn list_my_presets_strict_waits_out_an_in_flight_handshake_reply() {
    let (t, mut s) = in_flight_handshake_reply();
    assert_eq!(s.list_my_presets_strict().unwrap().len(), 504);
    assert_eq!(t.list_requests(), 0);
}

/// The strict re-arm's own reply streams past the inactivity drop; it must survive.
#[test]
fn list_my_presets_strict_rearm_outlives_the_inactivity_drop() {
    let frames = my_presets_list_frames(504);
    let t = ScriptedTransport::device().with_reply(streamed(&frames, 40));
    let mut s = session_over(&t, Vec::new());
    assert_eq!(s.list_my_presets_strict().unwrap().len(), 504);
    assert_eq!(t.list_requests(), 1);
}

/// The quiet field-8 read: a multi-frame reply streaming past the inactivity drop
/// arrives whole; the read re-arms the lapsed session it starts on, and the next read
/// on the session it kept alive needs no re-arm.
#[test]
fn slot_read_survives_the_inactivity_drop_and_rearms_only_a_lapsed_session() {
    use crate::test_support::preset_data_frames;
    let req = |slot| proto::preset_data_request(1, slot, None);
    let t = ScriptedTransport::device()
        .with_reply_to(&req(5), streamed(&preset_data_frames(17_000), 30))
        .with_reply_to(&req(6), streamed(&preset_data_frames(9_000), 30));
    let mut s = session_over(&t, Vec::new());
    s.pump_silent(1_000).unwrap(); // a drained, lapsed session
    assert_eq!(
        s.read_slot_preset_json(crate::slot::ds(5))
            .unwrap()
            .unwrap()
            .len(),
        17_000
    );
    assert_eq!(
        s.read_slot_preset_json(crate::slot::ds(6))
            .unwrap()
            .unwrap()
            .len(),
        9_000
    );
    assert_eq!(t.count_sent(&proto::connection_request()), 1);
    assert_eq!((t.live_rearms(), t.dropped_writes()), (0, 0));
}

/// A small reply that lands inside the send window ends after its stability slices,
/// not the full budget.
#[test]
fn slot_read_of_a_small_preset_stops_once_stable() {
    use crate::test_support::preset_data_frames;
    let t = ScriptedTransport::device().with_reply_to(
        &proto::preset_data_request(1, 3, None),
        vec![preset_data_frames(3_500)],
    );
    let mut s = session_over(&t, Vec::new());
    assert_eq!(
        s.read_slot_preset_json(crate::slot::ds(3))
            .unwrap()
            .unwrap()
            .len(),
        3_500
    );
    assert!(
        t.heartbeats() <= 1,
        "{} heartbeats — ran past stable",
        t.heartbeats()
    );
}

/// The `extra_burst` and discovery handshake windows (2000 / 1800 ms) keep their
/// streaming replies alive, and the handshake never sends a batch the firmware
/// drops (≥4).
#[test]
fn handshake_keeps_the_extra_burst_and_discovery_replies_alive() {
    use crate::test_support::preset_data_frames;
    let extra = proto::preset_data_request(1, 5, None);
    let t = ScriptedTransport::device()
        .with_reply_to(&extra, streamed(&preset_data_frames(17_000), 30));
    let mut s = session_over(&t, Vec::new());
    s.handshake(true, Some(extra), false, false).unwrap();
    s.pump_collect_alive(2_000).unwrap(); // the callers' harvest
    assert_eq!(s.try_preset_data_json().unwrap().len(), 17_000);
    assert_eq!(t.dropped_writes(), 0);
    let sent = t.sent.lock().unwrap();
    assert!(sent
        .iter()
        .all(|b| !matches!(proto::batch_status(b), Some(n) if n >= 4)));
}

/// A write after a long silent wait reopens the client first — a lapsed client
/// answers `connectionError` and applies nothing (the setter is silently lost).
#[test]
fn a_write_after_a_long_wait_reopens_the_client_first() {
    let t = ScriptedTransport::device();
    let mut s = session_over(&t, Vec::new());
    s.pump_silent(1_000).unwrap();
    s.clear_user_preset(ListIndex(415)).unwrap();
    assert_eq!(t.dropped_writes(), 0);
    assert_eq!(t.count_sent(&proto::connection_request()), 1);
    // The escape hatch for experiments that measure lapsed behavior itself.
    s.set_auto_reopen(false);
    s.pump_silent(1_000).unwrap();
    s.clear_user_preset(ListIndex(415)).unwrap();
    assert_eq!(t.dropped_writes(), 1);
}

/// The reopen's own reply window is not dropped: its reports reach `raw` with the next
/// batch — on the send-only (heartbeat) path too.
#[test]
fn the_reopen_reply_reaches_raw() {
    let frames = crate::test_support::preset_data_frames(100);
    let t = ScriptedTransport::device()
        .with_reply_to(&proto::connection_request(), vec![frames.clone()]);
    let mut s = session_over(&t, Vec::new());
    s.pump_silent(1_000).unwrap();
    s.heartbeat().unwrap();
    s.pump_silent(50).unwrap();
    assert_eq!(s.raw, frames);
}

#[test]
fn a_write_on_a_live_client_does_not_reopen_it() {
    let t = ScriptedTransport::device();
    let mut s = session_over(&t, Vec::new());
    s.pump_silent(300).unwrap();
    s.clear_user_preset(ListIndex(415)).unwrap();
    assert_eq!(t.count_sent(&proto::connection_request()), 0);
    assert_eq!(t.live_rearms(), 0);
}

/// Every handshake window is followed by the next request, so a window is the burst's
/// longest host silence: even the `TMP_HANDSHAKE_SCALE` bisect knob can't push one
/// past the inactivity drop.
#[test]
fn handshake_windows_stay_below_the_inactivity_drop() {
    for lean in [false, true] {
        for w in handshake_base_windows(lean) {
            assert_eq!(handshake_window(w, None), w);
            assert!(handshake_window(w, Some(100.0)) < HID_INACTIVITY_DROP_MS);
        }
    }
}

/// The Factory list waits out a reply still streaming (a decodable list is not a
/// complete one) and keeps the session alive while it does.
#[test]
fn list_factory_presets_waits_out_a_streaming_reply() {
    let frames = crate::test_support::preset_list_frames(4, 120);
    let t = ScriptedTransport::device()
        .with_idle(300)
        .with_pending(streamed(&frames[10..], 10));
    let mut s = session_over(&t, frames[..10].to_vec());
    assert_eq!(s.list_factory_presets().unwrap().len(), 120);
}

/// The import echo trails the upload by more than the inactivity drop; the wait keeps
/// the client alive so the echo is not discarded.
#[test]
fn import_preset_keeps_the_echo_window_alive() {
    let bytes = b"{\"info\":{}}".to_vec();
    let body = proto::import_preset_request(&proto::lz4_block_compress_stored(&bytes));
    let echo = crate::test_support::import_echo_frames(1, 7);
    let t = ScriptedTransport::device().with_reply_to(&body, vec![Vec::new(), Vec::new(), echo]);
    let mut s = session_over(&t, Vec::new());
    assert_eq!(s.import_preset_echo(&bytes).unwrap(), Some((1, 7)));
}

/// A body carrying a HW-proven discard shape is refused before a single frame leaves.
#[test]
fn import_preset_refuses_a_discard_shape_before_sending() {
    let doc = br#"{"ftsw":[[{"func":"on-off"},{"func":"param","valueType":2}]]}"#;
    let bytes = crate::backup::xor_jld(doc);
    let t = ScriptedTransport::device();
    let mut s = session_over(&t, Vec::new());
    assert!(s.import_preset(&bytes).unwrap_err().contains("stack"));
    assert!(t.sent.lock().unwrap().is_empty());
}

/// Callers have always received a LAPSED session, and the follow-on re-arm recipes
/// assume one — a harvest that ends on a heartbeat window must still hand one back.
#[test]
fn list_my_presets_lets_the_session_lapse_after_a_keepalive_harvest() {
    let frames = my_presets_list_frames(504);
    let t = ScriptedTransport::default();
    let mut s = session_over(&t, frames[..60].to_vec());
    assert!(s.list_my_presets().unwrap().len() < 504);
    assert!(t.heartbeats() > 0);
    assert!(
        t.idle_ms() > HID_INACTIVITY_DROP_MS,
        "{} ms idle",
        t.idle_ms()
    );
}

// ─── Template substitution never reaches a save ─────────────────────────────

fn saved_events(sim: &crate::sim_device::SimDevice) -> usize {
    sim.events()
        .iter()
        .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(_)))
        .count()
}

/// `loadScene(N≠8)` on a scene-less preset swaps in the unit's empty template (presetError
/// 6, static RE); the save choke point refuses it and sends nothing.
#[test]
fn a_scene_recall_on_a_scene_less_preset_blocks_the_save() {
    let sim = crate::sim_device::SimDevice::new();
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load");
    assert!(!s.template_substituted());
    s.load_scene(0).expect("recall");
    assert!(s.template_substituted(), "presetError 6 was answered");
    assert_eq!(
        s.save_current_preset(ListIndex(0)).expect_err("refused"),
        super::preset::TEMPLATE_SAVE_REFUSED
    );
    assert_eq!(saved_events(&sim), 0, "nothing reached the device");
    // A real load restores the stored body and clears the guard.
    s.load_preset(ListIndex(1)).expect("load another");
    s.load_preset(ListIndex(0)).expect("reload");
    assert!(!s.template_substituted());
    s.save_current_preset(ListIndex(0)).expect("save");
    assert_eq!(saved_events(&sim), 1);
}

/// A body rejected at load (presetError 6 on the load itself, device-confirmed) blocks the
/// save the same way; the next load that lands clears it.
#[test]
fn a_rejected_load_blocks_the_save_until_a_load_lands() {
    let sim = crate::sim_device::SimDevice::new();
    sim.arm(crate::sim_device::Injection::RejectNextLoad);
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(3)).expect("load");
    assert!(s.template_substituted());
    assert!(s.save_current_preset(ListIndex(3)).is_err());
    assert_eq!(saved_events(&sim), 0);
    s.load_preset(ListIndex(4)).expect("load another");
    assert!(!s.template_substituted());
}

/// The mark lives with the device, not the session: a fresh `Session` on the same unit
/// still refuses the save, and only a load that lands clears it.
#[test]
fn the_template_guard_survives_a_new_session() {
    let sim = crate::sim_device::SimDevice::new();
    sim.arm(crate::sim_device::Injection::RejectNextLoad);
    Session::from_transport(Box::new(sim.clone()))
        .load_preset(ListIndex(3))
        .expect("load");
    let mut s = Session::from_transport(Box::new(sim.clone()));
    assert!(
        s.template_substituted(),
        "the next session sees the template"
    );
    assert_eq!(
        s.save_current_preset(ListIndex(3)).expect_err("refused"),
        super::preset::TEMPLATE_SAVE_REFUSED
    );
    assert_eq!(saved_events(&sim), 0);
    s.load_preset(ListIndex(4)).expect("load another");
    assert!(!Session::from_transport(Box::new(sim.clone())).template_substituted());
}

/// The base sentinel (8) and an in-range scene leave the guard clear.
#[test]
fn a_valid_scene_recall_keeps_saves_open() {
    let sim =
        crate::sim_device::SimDevice::new().with_preset_json(crate::sim_device::SCENED_PRESET_JSON);
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load");
    s.load_scene(BASE_SCENE_SLOT).expect("base");
    s.load_scene(2).expect("scene 3");
    assert!(!s.template_substituted());
    s.save_current_preset(ListIndex(0)).expect("save");
    assert_eq!(saved_events(&sim), 1);
}

/// A scene index past the preset's scenes aborts the device server (static RE): the fake
/// records it, so a caller that sends one is caught offline.
#[test]
fn a_scene_index_past_the_scenes_aborts_the_server() {
    let sim = crate::sim_device::SimDevice::new().with_preset_json(
        r#"{"audioGraph":{"guitarNodes":{"G1":[]}},"scenes":[{"sceneName":"A"},{"sceneName":"B"}]}"#,
    );
    let mut s = Session::from_transport(Box::new(sim.clone()));
    s.load_preset(ListIndex(0)).expect("load");
    let _ = s.load_scene(2);
    assert!(sim
        .events()
        .iter()
        .any(|e| matches!(e, crate::sim_device::SimEvent::ServerAbort { .. })));
}

#[test]
fn scene_index_ok_takes_base_always_and_a_scene_only_when_the_document_has_it() {
    let two = serde_json::json!({"scenes": [{"sceneName": "A"}, {"sceneName": "B"}]});
    let none = serde_json::json!({"scenes": []});
    let absent = serde_json::json!({"audioGraph": {}});
    assert!(scene_index_ok(&two, BASE_SCENE_SLOT));
    assert!(scene_index_ok(&none, BASE_SCENE_SLOT));
    assert!(scene_index_ok(&two, 0) && scene_index_ok(&two, 1));
    assert!(!scene_index_ok(&two, 2));
    assert!(!scene_index_ok(&none, 0));
    assert!(!scene_index_ok(&absent, 0));
}

/// A stored `lastLoadedScene` on a scene-less preset is no re-stamp target: recalling it
/// would swap in the empty template. A cut (short) list never drops it.
#[test]
fn a_scene_less_preset_has_no_scene_to_restore() {
    use crate::scene_jobs::last_loaded_scene;
    let doc = |scenes: serde_json::Value, last: u32| serde_json::json!({"scenes": scenes, "lastLoadedScene": last});
    assert_eq!(last_loaded_scene(&doc(serde_json::json!([]), 3)), None);
    assert_eq!(
        last_loaded_scene(&doc(serde_json::json!([]), BASE_SCENE_SLOT)),
        Some(BASE_SCENE_SLOT)
    );
    assert_eq!(
        last_loaded_scene(&doc(serde_json::json!([{"sceneName": "A"}]), 3)),
        Some(3),
        "a short list may be a cut read; the device wrote the index itself"
    );
}

/// #262: the live-sync monitor's receive path. N pushes pumped one tick at a time leave
/// `raw` empty after every drain (no growth with uptime, no re-parse), each push decodes
/// on the tick its stream closes — the newest included, with no later stream to unblock
/// it — and a stream still open across ticks is held, not lost.
#[test]
fn drained_pushes_stay_bounded_and_decode_as_they_close() {
    let push = |i: usize| crate::sim_device::preset_data_changed(1, &[b'a' + (i % 26) as u8; 150]);
    const N: usize = 200;
    let mut batches: Vec<Vec<Vec<u8>>> = Vec::new();
    for i in 0..N {
        let frames = crate::sim_device::frame_multi(&push(i));
        assert!(frames.len() > 2, "a multi-frame push");
        // Split each push across two ticks: the first leaves its stream open.
        let (a, b) = frames.split_at(1);
        batches.push(a.to_vec());
        batches.push(b.to_vec());
    }
    let t = crate::test_support::ScriptedTransport::default().with_pending(batches);
    let mut s = crate::test_support::session_over(&t, Vec::new());
    let mut pushes = proto::Reassembler::new(proto::SingleFrame::ClosesStream);
    let mut decoded = Vec::new();
    for tick in 0..2 * N {
        s.pump_silent(1).unwrap();
        let closed = s.drain_closed_pushes(&mut pushes);
        assert!(
            s.raw.is_empty(),
            "tick {tick}: raw kept {} reports",
            s.raw.len()
        );
        if tick % 2 == 0 {
            assert!(
                closed.is_empty(),
                "tick {tick}: an open stream closed early"
            );
            assert!(
                pushes.pending_len() > 0,
                "tick {tick}: the open stream was dropped"
            );
        } else {
            assert_eq!(
                closed.len(),
                1,
                "tick {tick}: the closed push waited for a later one"
            );
            assert_eq!(pushes.pending_len(), 0);
        }
        decoded.extend(closed);
    }
    assert_eq!(decoded.len(), N);
    for (i, body) in decoded.iter().enumerate() {
        assert_eq!(body, &push(i), "push {i} reassembled byte-identically");
    }
}

/// The incremental reassembler closes exactly the streams the batch one does, in order.
#[test]
fn final_reassembler_matches_the_batch_reassembly() {
    let mut reports = Vec::new();
    for len in [10usize, 70, 200, 5, 130] {
        reports.extend(crate::sim_device::frame_multi(&vec![7u8; len]));
    }
    // An open stream cut by a new start, then an orphan continuation left open at the end.
    reports.push(vec![0x00, 0x33, 0x00, 2, 1, 2]);
    reports.push(vec![0x00, 0x33, 0x00, 1, 3]);
    reports.push(vec![0x00, 0x34, 0x00, 1, 4]);
    let batch = proto::reassemble_streams_final(&reports);
    let mut r = proto::Reassembler::new(proto::SingleFrame::ClosesStream);
    let mut closed = Vec::new();
    for rep in &reports {
        r.push(rep, &mut closed);
    }
    assert_eq!(
        closed.len(),
        batch.len() - 1,
        "all but the open tail closed"
    );
    for (a, b) in closed.iter().zip(&batch) {
        assert_eq!(a.body, b.body);
    }
    assert_eq!(r.pending_len(), batch.last().unwrap().body.len());
}

#[test]
fn doctor_params_keep_the_parametric_eq_filters_with_bools_as_numbers() {
    let v = serde_json::json!({
        "filter2frequency": 2600.0, "filter2gaindb": 0.0, "filter2q": 0.707,
        "filter2type": 2, "filter2bypass": false, "filter5bypass": true,
        "bypass": true, "filterbypass": true, "filter2colour": 1.0, "mix": 0.3
    });
    let got = doctor_params(v.as_object());
    assert_eq!(got.get("filter2frequency"), Some(&2600.0));
    assert_eq!(got.get("filter2type"), Some(&2.0));
    assert_eq!(got.get("filter2bypass"), Some(&0.0));
    assert_eq!(got.get("filter5bypass"), Some(&1.0));
    assert_eq!(got.get("mix"), Some(&0.3));
    // The node's own bypass, a filter field with no band number and an unknown
    // filter field stay out.
    for k in ["bypass", "filterbypass", "filter2colour"] {
        assert!(!got.contains_key(k), "{k}");
    }
}

/// A session whose handshake carried a `PresetLoaded` echo for 0-based `list_index`.
fn session_echoing(list_index: u64) -> Session {
    let mut echo = Vec::new();
    proto::field_varint(&mut echo, 1, 1); // tabEnum = My Presets
    proto::field_varint(&mut echo, 6, list_index + 1); // device slot = list index + 1
    let raw = crate::sim_device::frame_multi(&crate::sim_device::preset_message(11, &echo));
    crate::test_support::session_over(&crate::test_support::ScriptedTransport::default(), raw)
}

/// BUG→GATE #177 (HW fw 1.8.58): the connection opened right after a load still echoed the
/// slot loaded before it, so a load that landed was refused and its scratch import stranded.
/// A refused confirm releases its connection and retries on a fresh one; the first echo
/// that names the target confirms.
#[test]
fn a_stale_load_echo_is_retried_on_a_fresh_connection() {
    let mut echoes = vec![26u64, 10]; // popped from the end: stale 10 first, then 26
    let mut released = 0;
    let s = super::preset::confirm_on_fresh_connection(
        || {
            Ok(session_echoing(
                echoes.pop().expect("one connection per attempt"),
            ))
        },
        |_| released += 1,
        ListIndex(26),
        Some("Friedman HBE"),
        super::preset::LOAD_CONFIRM_ATTEMPTS,
    )
    .expect("the second connection's echo names the target");
    assert_eq!(s.loaded_slot(), Some(ListIndex(26)));
    assert_eq!(
        released, 1,
        "the stale attempt's connection is released first"
    );
}

/// The retry never relaxes the guard: an echo that keeps naming another slot refuses after
/// the last attempt, and the refusal says which slot the unit reported.
#[test]
fn an_echo_that_keeps_naming_another_slot_still_refuses() {
    let mut connects = 0;
    let mut released = 0;
    let e = super::preset::confirm_on_fresh_connection(
        || {
            connects += 1;
            Ok(session_echoing(10))
        },
        |_| released += 1,
        ListIndex(26),
        Some("Friedman HBE"),
        super::preset::LOAD_CONFIRM_ATTEMPTS,
    )
    .err()
    .expect("a load that never shows up refuses");
    assert_eq!(connects, super::preset::LOAD_CONFIRM_ATTEMPTS);
    assert_eq!(released, super::preset::LOAD_CONFIRM_ATTEMPTS);
    assert!(
        e.contains("reports list index 10 as loaded") && e.contains("refusing the save"),
        "{e}"
    );
}
