//! e2e UI bridge (`--features e2e`): carries the backend's `tmp://` events and every
//! `Channel` send from the windowless MockRuntime app to the page, in both tiers.
//!
//! Under MockRuntime nothing reaches a webview: an `emit`'s JS half and a Channel send's
//! `runCallback` eval both land in the mock dispatcher and go nowhere. So the server
//! captures them in-process instead:
//! - events through one `listen_any` listener per known `tmp://` name ([`UI_EVENTS`];
//!   Tauri has no wildcard listener and an `Event` does not carry its name);
//! - Channel sends through the Builder's `channel_interceptor`, which sees every send on a
//!   Channel a command received as an argument (`__CHANNEL__:<id>`), before the
//!   eval-or-fetch split, so large payloads are caught too.
//!
//! Both feed ONE bounded ring ([`RING_CAP`] entries, oldest dropped and counted), served by
//! `GET /ui/events?after=<seq>` on the side lane so rows reach the page while a run holds
//! the serial queue. `e2e/bridge-client.js` polls it and dispatches each entry to the
//! page's `listen` handler or `Channel.onmessage`.
//!
//! Not captured: a Channel's end-of-stream (`{end: true}`) notification, which bypasses
//! the interceptor (the page's Channel just stops receiving), and a Channel a command
//! builds itself with `Channel::new` (none does today). A Channel send with a RAW body
//! is recorded as a JSON array of byte values, not reshaped for the page's Channel; every
//! Channel the commands carry today sends JSON.

use std::collections::VecDeque;
use std::sync::Mutex;

use tauri::Listener;

/// The ring's capacity. `tmp://leveling-lufs` streams about 5 a second for a whole run.
pub(crate) const RING_CAP: usize = 2048;

/// Every `tmp://` event name the backend emits: the monitor's live-sync stream, the
/// device attach/detach pair, the library backup's progress and the scene run's meter.
pub(crate) const UI_EVENTS: &[&str] = &[
    crate::monitor::EVT_LIVE_PRESET,
    crate::monitor::EVT_LIVE_SCENE,
    crate::monitor::EVT_SCENE_LIST,
    crate::monitor::EVT_SIGNAL_CHAIN,
    crate::monitor::EVT_SYNC,
    crate::watcher::EVT_ATTACHED,
    crate::watcher::EVT_DETACHED,
    "tmp://backup-progress",
    "tmp://leveling-lufs",
    "tmp://open-bug-report",
];

/// One captured delivery.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct UiEntry {
    /// Monotonic across the server's life (a ring clear never resets it).
    pub(crate) seq: u64,
    /// `"event"` or `"channel"`.
    pub(crate) kind: &'static str,
    /// The event name, or the Channel id (the `<id>` of `__CHANNEL__:<id>`) as a string.
    pub(crate) name: String,
    /// A Channel message's index (the page's Channel reorders by it); `None` for an event.
    pub(crate) index: Option<usize>,
    /// The payload as JSON.
    pub(crate) payload: serde_json::Value,
}

struct Ring {
    entries: VecDeque<UiEntry>,
    next_seq: u64,
    dropped: u64,
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    entries: VecDeque::new(),
    next_seq: 1,
    dropped: 0,
});

fn ring() -> std::sync::MutexGuard<'static, Ring> {
    RING.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Append one delivery, dropping (and counting) the oldest past [`RING_CAP`].
pub(crate) fn push(kind: &'static str, name: String, index: Option<usize>, payload: &str) {
    let payload = serde_json::from_str(payload)
        .unwrap_or_else(|_| serde_json::Value::String(payload.to_string()));
    let mut r = ring();
    let seq = r.next_seq;
    r.next_seq += 1;
    r.entries.push_back(UiEntry {
        seq,
        kind,
        name,
        index,
        payload,
    });
    while r.entries.len() > RING_CAP {
        r.entries.pop_front();
        r.dropped += 1;
    }
}

/// The entries with `seq > after`, oldest first, plus the head (the last `seq` handed out,
/// 0 before any).
pub(crate) fn after(after: u64) -> (Vec<UiEntry>, u64) {
    let r = ring();
    let entries = r
        .entries
        .iter()
        .filter(|e| e.seq > after)
        .cloned()
        .collect();
    (entries, r.next_seq - 1)
}

/// How many entries the ring has dropped since the last [`clear`].
pub(crate) fn dropped() -> u64 {
    ring().dropped
}

/// Empty the ring (the offline `/sim/reset`). `seq` stays monotonic, so a page that read
/// the head before the reset never re-reads an old entry.
pub(crate) fn clear() {
    let mut r = ring();
    r.entries.clear();
    r.dropped = 0;
}

/// Subscribe the ring to every [`UI_EVENTS`] name on the built app.
pub(crate) fn listen_events<R: tauri::Runtime>(app: &tauri::App<R>) {
    for &name in UI_EVENTS {
        app.listen_any(name, move |e| {
            push("event", name.to_string(), None, e.payload());
        });
    }
}

/// Record one Channel send (the Builder's `channel_interceptor`). Returns `true`: the
/// message is consumed, since no webview exists to deliver it to.
pub(crate) fn intercept_channel<R: tauri::Runtime>(
    _webview: &tauri::Webview<R>,
    channel: tauri::ipc::CallbackFn,
    index: usize,
    body: &tauri::ipc::InvokeResponseBody,
) -> bool {
    let payload = match body {
        tauri::ipc::InvokeResponseBody::Json(s) => s.clone(),
        tauri::ipc::InvokeResponseBody::Raw(b) => serde_json::to_string(b).unwrap_or_default(),
    };
    push("channel", channel.0.to_string(), Some(index), &payload);
    true
}

/// `GET /ui/events?after=<seq>` → `{head, dropped, entries}`. A missing or bad `after`
/// reads as 0 (everything still in the ring).
pub(crate) fn route(query: &str) -> Vec<u8> {
    let since = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("after="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let (entries, head) = after(since);
    serde_json::to_vec(&serde_json::json!({
        "head": head,
        "dropped": dropped(),
        "entries": entries,
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ring is process-global; the tests that touch it hold this lock.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn the_ring_never_exceeds_its_cap_and_counts_what_it_drops() {
        let _g = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        let (_, head0) = after(0);
        for i in 0..RING_CAP + 5 {
            push("event", "tmp://leveling-lufs".into(), None, &format!("{i}"));
        }
        let (entries, head) = after(head0);
        assert_eq!(entries.len(), RING_CAP);
        assert_eq!(dropped(), 5);
        assert_eq!(head, head0 + RING_CAP as u64 + 5);
        // The oldest five went; the survivors run in order up to the head.
        assert_eq!(entries[0].payload, serde_json::json!(5));
        assert_eq!(entries.last().map(|e| e.seq), Some(head));
    }

    #[test]
    fn a_clear_empties_the_ring_but_keeps_seq_monotonic() {
        let _g = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        push("channel", "7".into(), Some(0), r#"{"slot":1}"#);
        let (_, before) = after(0);
        clear();
        assert!(after(0).0.is_empty());
        assert_eq!(dropped(), 0);
        push("channel", "7".into(), Some(1), r#"{"slot":2}"#);
        let (entries, head) = after(before);
        assert_eq!(head, before + 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].index, Some(1));
        assert_eq!(entries[0].payload, serde_json::json!({ "slot": 2 }));
    }

    #[test]
    fn the_route_reads_after_from_the_query_and_defaults_to_zero() {
        let _g = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        push(
            "event",
            "tmp://backup-progress".into(),
            None,
            r#"{"done":1}"#,
        );
        let (_, head) = after(0);
        let v: serde_json::Value =
            serde_json::from_slice(&route(&format!("after={head}"))).unwrap_or_default();
        assert_eq!(v["head"], serde_json::json!(head));
        assert_eq!(v["entries"], serde_json::json!([]));
        let all: serde_json::Value = serde_json::from_slice(&route("")).unwrap_or_default();
        assert_eq!(all["entries"].as_array().map(Vec::len), Some(1));
    }
}
