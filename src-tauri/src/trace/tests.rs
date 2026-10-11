//! Recorder tests: each runs its own recorder on its own temporary run directory, so
//! none touches the process-global one (which stays off: tests never set
//! `TMP_TRACE_DIR`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::capture::{CaptureCtx, CaptureJob};
use super::record::{Backend, Config, Recorder};
use crate::hid::HidTransport;
use crate::proto;
use crate::session::Session;
use crate::test_support::ScriptedTransport;

struct Run {
    root: PathBuf,
    cfg: Config,
}

impl Run {
    fn new(tag: &str) -> Run {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("tct-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cfg = Config {
            dir: root.join("trace"),
            process: "test".into(),
            run_id: Some("r1".into()),
            git_sha: None,
            tree_key: None,
            unit_config: Some("stock".into()),
            wav_pair: true,
            fw_file: Some(root.join("LAST_DEVICE_FW")),
        };
        Run { root, cfg }
    }

    fn start(&self) -> Arc<Recorder> {
        Recorder::start(self.cfg.clone()).unwrap()
    }

    fn lines(&self, rec: &Recorder) -> Vec<serde_json::Value> {
        rec.drain(Duration::from_secs(30));
        let path = self
            .cfg
            .dir
            .join(format!("test-{}.tct.jsonl", std::process::id()));
        let text = std::fs::read_to_string(path).unwrap();
        super::format::read_trace(&text).unwrap()
    }

    fn events<'a>(lines: &'a [serde_json::Value], ev: &str) -> Vec<&'a serde_json::Value> {
        lines.iter().filter(|l| l["ev"] == ev).collect()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn wrap(rec: &Arc<Recorder>, t: &ScriptedTransport) -> Box<dyn HidTransport> {
    let inner = t.clone();
    rec.open(Backend::Sim, move || {
        Ok(Box::new(inner) as Box<dyn HidTransport>)
    })
    .unwrap()
}

/// The reopen scenario `session.rs` tests (a write after a long wait reopens the client
/// first), run once bare and once recorded.
fn reopen_scenario(hid: Box<dyn HidTransport>) -> Vec<Vec<u8>> {
    let mut s = Session::from_transport(hid);
    s.pump_silent(1_000).unwrap();
    s.heartbeat().unwrap();
    s.pump_silent(50).unwrap();
    std::mem::take(&mut s.raw)
}

fn reopen_device() -> (ScriptedTransport, Vec<Vec<u8>>) {
    let frames = crate::test_support::preset_data_frames(100);
    let t = ScriptedTransport::device()
        .with_reply_to(&proto::connection_request(), vec![frames.clone()]);
    (t, frames)
}

#[test]
fn every_call_and_the_reopen_are_recorded_and_replies_are_unchanged() {
    let (bare_t, frames) = reopen_device();
    let bare = reopen_scenario(Box::new(bare_t));
    assert_eq!(bare, frames);

    let run = Run::new("wire");
    let rec = run.start();
    let (t, _) = reopen_device();
    let recorded = reopen_scenario(wrap(&rec, &t));
    assert_eq!(recorded, bare, "the recorder must not change any reply");

    let lines = run.lines(&rec);
    assert_eq!(lines[0]["ev"], "header");
    assert_eq!(lines[0]["tct"], 1);
    assert_eq!(lines[0]["unit_config"], "stock");
    // seq is gap-free.
    for (i, l) in lines.iter().enumerate() {
        assert_eq!(l["seq"], i as u64);
    }
    let open = Run::events(&lines, "open");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["backend"], "sim");
    assert_eq!(open[0]["ok"], true);

    let outs = Run::events(&lines, "out");
    let calls: Vec<&str> = outs.iter().map(|o| o["call"].as_str().unwrap()).collect();
    // pump(1000), the reopen's connectionRequest, the heartbeat, pump(50).
    assert_eq!(calls, ["pump", "transact", "send", "pump"]);
    assert_eq!(
        outs[1]["body"],
        super::format::hex(&proto::connection_request())
    );
    assert_eq!(outs[2]["body"], super::format::hex(&proto::heartbeat()));
    assert!(outs[0]["body"].is_null());
    assert_eq!(outs[0]["pump_ms"], 1000);

    // Each `in` names its `out` and carries the raw reports verbatim.
    let ins = Run::events(&lines, "in");
    assert_eq!(ins.len(), 4);
    for (o, i) in outs.iter().zip(&ins) {
        assert_eq!(i["of"], o["seq"]);
    }
    let reopen_reports: Vec<String> = ins[1]["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_string())
        .collect();
    let want: Vec<String> = frames.iter().map(|f| super::format::hex(f)).collect();
    assert_eq!(reopen_reports, want);
    assert_eq!(ins[1]["k"].as_array().unwrap().len(), frames.len());

    // The nominal clock sums the pump windows the session asked for.
    assert_eq!(outs[3]["t_nom_ms"], 1000 + 100);
    assert_eq!(Run::events(&lines, "close").len(), 1);
    assert!(Run::events(&lines, "drop").is_empty());
}

#[test]
fn unset_dir_means_no_recorder() {
    assert!(Config::from_vars(|_| None).is_none());
    assert!(Config::from_vars(|k| (k == "TMP_TRACE_DIR").then(String::new)).is_none());
    // Tests never arm the process-global recorder.
    assert!(super::recorder().is_none());
    let t = ScriptedTransport::device();
    let hid = super::open(Backend::Sim, || {
        Ok(Box::new(t.clone()) as Box<dyn HidTransport>)
    })
    .unwrap();
    hid.send(&proto::heartbeat()).unwrap();
    assert_eq!(t.heartbeats(), 1);
}

#[test]
fn a_full_queue_drops_without_blocking_and_says_so() {
    let run = Run::new("full");
    let (rec, writer) = Recorder::detached(run.cfg.clone(), 2).unwrap();
    let t = Instant::now();
    for _ in 0..5 {
        rec.settle(100, 100);
    }
    assert!(
        t.elapsed() < Duration::from_millis(200),
        "the caller blocked"
    );
    writer.spawn().unwrap();
    let lines = run.lines(&rec);
    assert_eq!(Run::events(&lines, "settle").len(), 2);
    let drops = Run::events(&lines, "drop");
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0]["what"], "other");
    assert_eq!(drops[0]["count"], 3);
}

fn four_channel_capture() -> crate::audio::Capture {
    let frames = 48_000;
    let mut interleaved = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let s = ((i as f32) * 0.05).sin();
        interleaved.extend_from_slice(&[0.5 * s, 0.4 * s, 0.9 * s, 0.0]);
    }
    crate::audio::Capture {
        interleaved,
        channels: 4,
        sample_rate: 48_000,
    }
}

#[test]
fn a_capture_record_carries_backend_identity_engage_and_its_wav() {
    let run = Run::new("capture");
    let rec = run.start();
    let t = ScriptedTransport::device();
    let echo = {
        let body = proto::set_reamp_mode(true);
        let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
        r.extend_from_slice(&body);
        r
    };
    let t = t.with_reply_to(&proto::set_reamp_mode(true), vec![vec![echo]]);
    let hid = wrap(&rec, &t);
    hid.transact(&proto::set_reamp_mode(true), 50).unwrap();
    let now = Instant::now();
    rec.capture(CaptureJob {
        backend: Backend::Sim.as_str(),
        t_start: now,
        t_end: now,
        stimulus: vec![0.1; 48_000],
        rate: 48_000,
        tail_ms: 800,
        result: Ok(four_channel_capture()),
        ctx: Some(CaptureCtx {
            label: "scene".into(),
            slot: Some(404),
            scene_slot: Some(2),
            switch: None,
        }),
    });
    let lines = run.lines(&rec);
    let caps = Run::events(&lines, "capture");
    assert_eq!(caps.len(), 1);
    let c = caps[0];
    assert_eq!(c["backend"], "sim");
    assert_eq!(c["channels"], 4);
    assert_eq!(c["peak"].as_array().unwrap().len(), 4);
    assert_eq!(c["rms"].as_array().unwrap().len(), 4);
    assert!(c["lufs_i"].as_f64().unwrap() < 0.0);
    assert_eq!(c["ctx"]["slot"], 404);
    assert_eq!(c["ctx"]["scene_slot"], 2);
    assert!(c["ctx"]["switch"].is_null());
    assert_eq!(c["engage"]["on"], true);
    assert_eq!(c["engage"]["echo"], true);
    let wav = c["wav"].as_str().unwrap();
    assert!(run.root.join(wav).is_file());
    assert!(c["err"].is_null());
    drop(hid);
}

#[test]
fn a_transport_drop_never_waits_behind_capture_work() {
    // The online lane's captures are ~10 s of 4-channel audio, metered and written as a
    // WAV in a debug build: a close (and a mid-session reopen) must not queue behind it.
    let run = Run::new("capwait");
    let rec = run.start();
    let frames = 48_000 * 30;
    let mut interleaved = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let s = ((i as f32) * 0.05).sin();
        interleaved.extend_from_slice(&[0.5 * s, 0.4 * s, 0.9 * s, 0.1 * s]);
    }
    let now = Instant::now();
    rec.capture(CaptureJob {
        backend: Backend::Device.as_str(),
        t_start: now,
        t_end: now,
        stimulus: vec![0.1; 48_000],
        rate: 48_000,
        tail_ms: 800,
        result: Ok(crate::audio::Capture {
            interleaved,
            channels: 4,
            sample_rate: 48_000,
        }),
        ctx: None,
    });
    let t = ScriptedTransport::device();
    let hid = wrap(&rec, &t);
    let started = Instant::now();
    drop(hid);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "close waited {:?} behind a capture",
        started.elapsed()
    );
    let lines = run.lines(&rec);
    let cap = Run::events(&lines, "capture")[0];
    assert_eq!(
        cap["t_nom_ms"], 0,
        "the capture keeps its place on the nominal clock"
    );
    assert_eq!(cap["frames"], frames);
    let close = Run::events(&lines, "close")[0];
    assert_eq!(
        close["t_nom_ms"], 1800,
        "later lines follow the capture's nominal length"
    );
}

#[test]
fn a_failed_capture_is_recorded_with_its_error() {
    let run = Run::new("capfail");
    let rec = run.start();
    let now = Instant::now();
    rec.capture(CaptureJob {
        backend: Backend::Device.as_str(),
        t_start: now,
        t_end: now,
        stimulus: vec![0.0; 10],
        rate: 48_000,
        tail_ms: 0,
        result: Err("no signal captured".into()),
        ctx: None,
    });
    let lines = run.lines(&rec);
    let c = Run::events(&lines, "capture")[0];
    assert_eq!(c["err"], "no signal captured");
    assert!(c["lufs_i"].is_null());
    assert!(c["wav"].is_null());
}

#[test]
fn field8_bodies_are_canonicalized_and_cut_ones_are_marked() {
    let run = Run::new("field8");
    let rec = run.start();
    rec.field8(
        404,
        br#"{"nodeId":"cab1","p":{"volume":0.489999920129776}}"#,
        Some("ok".into()),
    );
    rec.field8(405, br#"{"audioGraph":{"pres"#, None);
    let lines = run.lines(&rec);
    let st = Run::events(&lines, "state");
    assert_eq!(st[0]["kind"], "field8");
    assert_eq!(st[0]["complete"], true);
    assert_eq!(st[0]["pristine"], "ok");
    assert_eq!(st[0]["file"], "field8/404.json");
    let text = std::fs::read_to_string(run.root.join("field8/404.json")).unwrap();
    assert_eq!(text, r#"{"nodeId":"cab1","p":{"volume":0.49}}"#);
    assert_eq!(st[1]["complete"], false);
    assert_eq!(st[1]["bytes"], 20);
    assert!(st[1]["file"].is_null());
    assert!(st[1]["canon_sha256"].is_null());
}

#[test]
fn a_firmware_reply_is_recorded_and_written_only_for_a_device_session() {
    let fw_report = {
        let body = [
            0x1a, 0x0a, 0x12, 0x08, 0x0a, 0x06, b'1', b'.', b'8', b'.', b'5', b'8',
        ];
        let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
        r.extend_from_slice(&body);
        r
    };
    let req = proto::current_fw_request();
    for (backend, writes) in [(Backend::Sim, false), (Backend::Device, true)] {
        let run = Run::new("fw");
        let rec = run.start();
        let t = ScriptedTransport::device().with_reply_to(&req, vec![vec![fw_report.clone()]]);
        let inner = t.clone();
        let hid = rec
            .open(
                backend,
                move || Ok(Box::new(inner) as Box<dyn HidTransport>),
            )
            .unwrap();
        hid.transact(&req, 20).unwrap();
        let lines = run.lines(&rec);
        let fw = Run::events(&lines, "fw");
        assert_eq!(fw.len(), 1);
        assert_eq!(fw[0]["fw"], "1.8.58");
        let file = run.root.join("LAST_DEVICE_FW");
        assert_eq!(file.is_file(), writes, "{backend:?}");
        drop(hid);
    }
}
