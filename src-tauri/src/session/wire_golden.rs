//! Wire golden: every request the session's public commands put on the wire, with its
//! transport op and pump window, in order, committed as `wire_golden.txt`
//! (`wire_golden.e2e.txt` under the `e2e` feature).
//!
//! The session's modules move transport, timing and command code between files without
//! changing behaviour. This test is the proof: the same calls must send the same bodies
//! with the same windows in the same order. Regenerate (only for a deliberate wire or
//! timing change) with `TMP_UPDATE_WIRE_GOLDEN=1 cargo test --lib wire_golden`, once per
//! feature set (`--features e2e`).

use std::sync::{Arc, Mutex};

use super::Session;
use crate::hid::HidTransport;
use crate::sim_device::SimDevice;
use crate::slot::ListIndex;

/// Delegates to a [`SimDevice`] and logs `(op, window, body hex)` per call.
struct Recorder {
    inner: SimDevice,
    log: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn note(&self, op: &str, ms: u64, body: &[u8]) {
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        self.log
            .lock()
            .expect("golden log")
            .push(format!("{op} {ms} {hex}"));
    }
}

impl HidTransport for Recorder {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.note("send", 0, body);
        self.inner.send(body)
    }
    fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.note("transact", ms, body);
        self.inner.transact(body, ms)
    }
    fn transact_chunked(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.note("chunked", ms, body);
        self.inner.transact_chunked(body, ms)
    }
    fn pump(&self, ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.note("pump", ms, &[]);
        self.inner.pump(ms)
    }
    fn transact_eager(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.note("eager", ms, body);
        self.inner.transact_eager(body, ms)
    }
    fn template_mark(&self) -> std::sync::Arc<crate::hid::TemplateMark> {
        crate::hid::HidTransport::template_mark(&self.inner)
    }
}

const RIG: &str = r#"{"audioGraph":{"guitarNodes":{"G1":[
    {"FenderId":"ACD_TubeScreamer","nodeId":"ACD_TubeScreamer","dspUnitParameters":{"bypass":false,"level":0.5}},
    {"FenderId":"ACD_JC120","nodeId":"ACD_JC120","dspUnitParameters":{"bypass":false,"outputLevel":0.4}}
]}},
"scenes":[
    {"guitarNodes":{"G1":{"ACD_JC120":{"dspUnitParameters":{"bypass":false,"outputLevel":0.6}}}}},
    {"guitarNodes":{"G1":{"ACD_TubeScreamer":{"dspUnitParameters":{"bypass":true}}}}}
],
"ftsw":[[{"func":"onOff","nodeId":"ACD_TubeScreamer"}]]}"#;

/// One labelled step: its outcome (`ok`/`err`) is part of the golden too.
fn step<T>(log: &Arc<Mutex<Vec<String>>>, label: &str, r: Result<T, String>) {
    let outcome = if r.is_ok() { "ok" } else { "err" };
    log.lock()
        .expect("golden log")
        .push(format!("# {label} -> {outcome}"));
}

fn mark(log: &Arc<Mutex<Vec<String>>>, label: &str) {
    log.lock().expect("golden log").push(format!("# {label}"));
}

/// Drive every command method once, in a fixed order, over one session. `lapse` and
/// `device_backup` stay out: both wait on the wall clock, so their pump counts vary
/// from run to run against a fake that answers instantly.
fn record() -> Vec<String> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sim = SimDevice::new().with_preset_json(RIG);
    let mut s = Session::from_transport(Box::new(Recorder {
        inner: sim.clone(),
        log: Arc::clone(&log),
    }));
    let l = &log;
    step(l, "handshake", s.handshake(true, None, true, false));
    step(l, "list_my_presets", s.list_my_presets());
    step(l, "reread_my_presets", s.reread_my_presets());
    step(l, "list_factory_presets", s.list_factory_presets());
    step(l, "list_my_presets_strict", s.list_my_presets_strict());
    step(l, "request_scene_list", s.request_scene_list());
    step(l, "heartbeat", s.heartbeat());
    step(l, "load_preset", s.load_preset(ListIndex(400)));
    step(l, "load_preset_raw", s.load_preset_raw(401, 1));
    step(
        l,
        "load_factory_preset",
        s.load_factory_preset(ListIndex(3)),
    );
    step(l, "load_preset again", s.load_preset(ListIndex(400)));
    step(l, "confirm_active", s.confirm_active(ListIndex(400), None));
    step(l, "begin_live_edit", s.begin_live_edit());
    step(l, "set_reamp_mode on", s.set_reamp_mode(true));
    step(l, "read_reamp_state", s.read_reamp_state());
    step(l, "set_reamp_mode off", s.set_reamp_mode(false));
    step(l, "set_preset_level", s.set_preset_level(0.5));
    step(
        l,
        "change_parameter",
        s.change_parameter("G1", "ACD_JC120", "outputLevel", 0.5),
    );
    step(
        l,
        "change_parameter_bool",
        s.change_parameter_bool("G1", "ACD_TubeScreamer", "bypass", true),
    );
    step(l, "load_scene", s.load_scene(0));
    step(
        l,
        "set_node_scene_edit",
        s.set_node_scene_edit("G1", "ACD_JC120", true),
    );
    step(l, "load_scene base", s.load_scene(super::BASE_SCENE_SLOT));
    step(l, "live_complete_value", s.live_complete_value());
    step(l, "live_audio_graph", s.live_audio_graph(|_| true));
    mark(l, &format!("live_ftsw -> {}", s.live_ftsw().is_some()));
    step(
        l,
        "replace_node",
        s.replace_node("G1", "ACD_TubeScreamer", "ACD_Boost"),
    );
    step(
        l,
        "insert_node",
        s.insert_node("G1", None, "ACD_KingOfTone"),
    );
    step(
        l,
        "insert_node_at_index",
        s.insert_node_at_index("G1", 0, "ACD_TMSpring63"),
    );
    step(l, "remove_node", s.remove_node("G1", "ACD_TMSpring63"));
    step(
        l,
        "replace_node_with_block",
        s.replace_node_with_block("G1", "ACD_Boost", "ACD_Boost", 0),
    );
    step(
        l,
        "read_node_param_str",
        s.read_node_param_str("G1", "ACD_JC120", "gatePreset"),
    );
    step(
        l,
        "set_footswitch_assignment",
        s.set_footswitch_assignment(
            1,
            1,
            r#"{"func":"onOff","nodeId":"ACD_JC120"}"#,
            false,
            None,
        ),
    );
    step(
        l,
        "clear_footswitch_assignment",
        s.clear_footswitch_assignment(1, 1),
    );
    step(
        l,
        "rename_current_preset",
        s.rename_current_preset("Golden"),
    );
    step(
        l,
        "save_current_preset",
        s.save_current_preset(ListIndex(400)),
    );
    step(
        l,
        "read_slot_preset_json",
        s.read_slot_preset_json(crate::slot::ds(401)),
    );
    step(
        l,
        "read_slot_preset_json_live",
        s.read_slot_preset_json_live(crate::slot::ds(401)),
    );
    step(
        l,
        "capture_full_preset_json",
        s.capture_full_preset_json(Some(ListIndex(400)), 200),
    );
    step(l, "harvest_slot_read", s.harvest_slot_read());
    step(l, "import_preset", s.import_preset(b"not a preset body"));
    step(
        l,
        "move_user_preset",
        s.move_user_preset(ListIndex(409), ListIndex(410)),
    );
    step(l, "clear_user_preset", s.clear_user_preset(ListIndex(410)));
    step(l, "add_song", s.add_song("Golden Song"));
    step(l, "rename_song", s.rename_song(0, "Golden Song 2"));
    step(l, "set_song_notes", s.set_song_notes(0, "notes"));
    step(l, "set_song_bpm_active", s.set_song_bpm_active(0, true));
    step(l, "song_presets", s.song_presets(0));
    step(
        l,
        "assign_song_preset",
        s.assign_song_preset(0, 0, ListIndex(400), "A", 1, 0, 2),
    );
    step(l, "move_song_preset", s.move_song_preset(0, 0, 1));
    step(l, "swap_song_preset", s.swap_song_preset(0, 0, 1));
    step(l, "clear_song_preset", s.clear_song_preset(0, 0));
    step(l, "load_song", s.load_song(0, 0, 400));
    step(l, "set_tap_tempo_bpm", s.set_tap_tempo_bpm(120.0));
    step(l, "song_list_strict", s.song_list_strict());
    step(l, "add_setlist", s.add_setlist("Golden Set"));
    step(l, "rename_setlist", s.rename_setlist(0, "Golden Set 2"));
    step(l, "add_setlist_song", s.add_setlist_song(0, 0));
    step(l, "move_setlist_song", s.move_setlist_song(0, 0, 0));
    step(l, "setlist_songs_strict", s.setlist_songs_strict(0));
    step(l, "remove_setlist_song", s.remove_setlist_song(0, 0));
    step(l, "remove_setlist", s.remove_setlist(0));
    step(l, "remove_song", s.remove_song(0));
    mark(
        l,
        &format!("active_slot_live -> {:?}", s.active_slot_live()),
    );
    mark(
        l,
        &format!(
            "resolve_unique_my_preset_slot -> {:?}",
            s.resolve_unique_my_preset_slot(Some("E2E Rig"))
        ),
    );
    step(l, "rearm_active_info", s.rearm_active_info());
    step(l, "pump_collect_alive", s.pump_collect_alive(700));
    step(
        l,
        "send_and_collect_alive",
        s.send_and_collect_alive(&crate::proto::heartbeat(), 400),
    );
    step(l, "drain_until_quiet", s.drain_until_quiet(100, 3));
    let out = log.lock().expect("golden log").clone();
    out
}

#[test]
fn the_session_commands_put_the_same_requests_on_the_wire() {
    let got = record().join("\n") + "\n";
    // The `e2e` feature changes what the fake answers (its live working copy), so each
    // feature set keeps its own golden.
    let name = if cfg!(feature = "e2e") {
        "wire_golden.e2e.txt"
    } else {
        "wire_golden.txt"
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/session")
        .join(name);
    if std::env::var_os("TMP_UPDATE_WIRE_GOLDEN").is_some() {
        std::fs::write(&path, &got).expect("write wire golden");
        return;
    }
    let want = std::fs::read_to_string(&path).expect("read wire golden");
    if got != want {
        let _ = std::fs::write(std::env::temp_dir().join("wire_golden.got.txt"), &got);
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| got.lines().count().min(want.lines().count()));
        panic!(
            "wire golden differs at line {} (got {} lines, want {}):\n got: {:?}\nwant: {:?}",
            first + 1,
            got.lines().count(),
            want.lines().count(),
            got.lines().nth(first),
            want.lines().nth(first)
        );
    }
}
