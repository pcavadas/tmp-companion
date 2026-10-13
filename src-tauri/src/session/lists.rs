//! Preset-list reads: My Presets (lenient and strict), Factory, the re-arm and re-read
//! paths and the list-response extraction. Calls link primitives; never touches
//! `self.hid`, the clock or a literal window directly.

use super::*;

/// Parse a FenderMessageTMS body, descend `presetMessage → presetListResponse`,
/// and collect the `displayName` of each record. Returns None if this body
/// doesn't carry a PresetListResponse.
pub(super) fn extract_preset_list(body: &[u8]) -> Option<Vec<String>> {
    // My Presets only — the handshake also collects the Factory/Cloud responses
    // on the same session (the wrong-list hazard is documented on the helper).
    // A missing listEnum is treated as My Presets (lean sessions request only 1).
    extract_preset_list_for(body, LIST_ENUM_MY_PRESETS)
}

/// Preset-list `listEnum` values (My Presets = 1, Factory = 4, Cloud = 3).
pub(super) const LIST_ENUM_MY_PRESETS: u64 = 1;

pub(super) const LIST_ENUM_FACTORY: u64 = 4;

/// `extract_preset_list`, gated to an explicit `listEnum` (field 1 of the
/// `PresetListResponse`). A missing listEnum defaults to My Presets, so it only
/// matches `want_enum == 1` (Factory/Cloud must be present explicitly).
pub(super) fn extract_preset_list_for(body: &[u8], want_enum: u64) -> Option<Vec<String>> {
    let resp = dig(body, TMS_PRESET, PRESET_LIST_RESPONSE)?;
    if proto::first_varint(&resp, 1).unwrap_or(LIST_ENUM_MY_PRESETS) != want_enum {
        return None;
    }
    // PresetListResponse.record (field 2, repeated) → PresetListRecord.displayName (field 1).
    let names: Vec<String> = resp
        .iter()
        .filter(|(f, _)| *f == 2)
        .filter_map(|(_, v)| v.as_bytes())
        .filter_map(|rec| {
            let rf = proto::parse(rec);
            field1(&rf)
                .and_then(|v| v.as_bytes())
                .map(|s| String::from_utf8_lossy(s).into_owned())
        })
        .collect();
    Some(names)
}

/// Display names → `PresetEntry` rows: the 0-based list index IS the slot (the
/// device's 1-based `userSlot` is derived in the setters, not here).
pub(super) fn preset_entries(names: Vec<String>) -> Vec<PresetEntry> {
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| PresetEntry {
            slot: ListIndex(i as u32),
            name,
        })
        .collect()
}

pub(super) fn best_preset_list_from_reports(reports: &[Vec<u8>]) -> Option<Vec<String>> {
    proto::reassemble_streams(reports)
        .into_iter()
        .chain(proto::reassemble_streams_final(reports))
        .filter_map(|s| extract_preset_list(&s.body))
        .max_by_key(|names| (names.len(), names.last().map_or(0, |s| s.len())))
}

/// Longest FACTORY (listEnum = 4) preset list from the accumulated reports. Same
/// dual-stream-rule harvest as `best_preset_list_from_reports`, but gated to the
/// Factory list — the handshake requests My/Factory/Cloud on one session, so this
/// picks the Factory reply out of the shared accumulator.
pub(super) fn best_factory_list_from_reports(reports: &[Vec<u8>]) -> Option<Vec<String>> {
    proto::reassemble_streams(reports)
        .into_iter()
        .chain(proto::reassemble_streams_final(reports))
        .filter_map(|s| extract_preset_list_for(&s.body, LIST_ENUM_FACTORY))
        .max_by_key(|names| (names.len(), names.last().map_or(0, |s| s.len())))
}

impl Session {
    /// Enumerate the "My Presets" list (listEnum = 1). The response can be
    /// large and multi-packet, so we accumulate raw reports across pump windows
    /// and reassemble cumulatively until the harvest stops GROWING — a mid-flood
    /// partial reassembly DECODES under the tolerant rule, so the first decodable
    /// harvest can be a tail-cut of a response still arriving (HW: 277–328 of 504
    /// records after back-to-back lean sessions, while the same device answered
    /// Pro Control's single long session in full — the flood streams slower under
    /// session churn; accepting the first decode was the truncation, not the
    /// device). Two consecutive no-growth pump windows = complete.
    ///
    /// KEEPALIVE ([`HID_INACTIVITY_DROP_MS`]): every window is kept alive while the list is
    /// unproven, and a harvest that ends on such a window then [`Self::lapse`]s — callers
    /// keep getting the LAPSED session they always got.
    pub fn list_my_presets(&mut self) -> Result<Vec<PresetEntry>, String> {
        // The handshake already issued preset_list_request(1) and accumulated
        // its reply; check that first. Only re-query if it isn't there yet (a short
        // send window — the keepalive windows below collect the reply).
        let mut names = self.best_preset_list();
        if names.is_none() {
            self.send_and_collect(
                &proto::preset_list_request(1, proto::BATCH_DRAIN),
                LIST_REQUEST_MS,
            )?;
        }
        // Growth metric = record count + total content bytes, not count alone:
        // the tolerant decode's final-frame tail case can GROW IN CONTENT at a
        // constant count (the historical `"Empty"`-decoded-as-`"Empt"` garble),
        // and a count-only check would accept mid-tail.
        let weigh = |n: &Option<Vec<String>>| -> usize {
            n.as_ref()
                .map_or(0, |v| v.len() + v.iter().map(String::len).sum::<usize>())
        };
        let mut proven = self.my_presets_proven(names.as_deref());
        let (mut stable, mut grew, mut alive) = (0u32, false, false);
        for _ in 0..12 {
            let prev = weigh(&names);
            alive = !proven;
            if alive {
                self.keepalive_pump()?;
                self.keepalive_pump()?;
            } else {
                self.pump_silent(LIST_QUIET_WINDOW_MS)?;
            }
            if let Some(found) = self.best_preset_list() {
                names = Some(found);
            }
            proven = self.my_presets_proven(names.as_deref());
            let cur = weigh(&names);
            if cur > 0 && cur == prev {
                stable += 1;
                // A list that was already complete before this call (the common
                // case — the handshake's reply fully accumulated) needs only ONE
                // quiet confirm window; the two-window rule applies when this
                // call actually watched the flood grow. Keeps the truncation fix
                // without taxing every tolerant read a flat 1.4 s — the read sits
                // in a 3×-per-preset loop under bulk runs (`replace_inplace_with`).
                // The fast path needs PROOF: a decodable list is not a complete one.
                if stable >= if grew || !proven { 2 } else { 1 } {
                    break;
                }
            } else {
                grew = grew || cur > prev;
                stable = 0;
            }
        }
        if alive {
            self.lapse()?;
        }
        let names =
            names.ok_or_else(|| "no PresetListResponse received from device".to_string())?;
        Ok(preset_entries(names))
    }

    /// Whether the tolerant harvest `names` is PROVEN complete: a strict
    /// (terminal-0x35) decode of the same length is present.
    pub(super) fn my_presets_proven(&self, names: Option<&[String]>) -> bool {
        names.is_some_and(|n| {
            self.harvest_preset_list_strict()
                .is_some_and(|strict| strict.len() == n.len())
        })
    }

    /// Keepalive windows until a complete (strict) My-Presets decode lands — at most ~1 s,
    /// and none past a window that brought nothing new (that reply is done or dropped).
    pub(super) fn await_strict_list(&mut self) -> Result<Option<Vec<String>>, String> {
        for _ in 0..3 {
            if let Some(found) = self.harvest_preset_list_strict() {
                return Ok(Some(found));
            }
            let before = self.raw.len();
            self.keepalive_pump()?;
            if self.raw.len() == before {
                break;
            }
        }
        Ok(self.harvest_preset_list_strict())
    }

    /// Re-request My Presets — the shared retry of [`Self::list_my_presets_strict`] and
    /// [`Self::reread_my_presets`]. A lapsed client is reopened by the write itself
    /// (`TrackedHid`); a live one is not re-armed (that only draws a `connectionError`).
    pub(super) fn rearm_my_presets_request(&mut self) -> Result<(), String> {
        self.send_and_collect(
            &proto::preset_list_request(1, proto::BATCH_DRAIN),
            LIST_REQUEST_MS,
        )
    }

    /// Re-read the My-Presets list on this HELD session (no reopen). `raw` MUST be cleared:
    /// a stale truncated list left in it out-weighs the fresh reply's mid-flood partials in
    /// the tolerant harvest.
    /// Quiet sessions only — a re-arm on a live one draws a `connectionError`.
    pub fn reread_my_presets(&mut self) -> Result<Vec<PresetEntry>, String> {
        self.drain_until_quiet(DRAIN_QUIET_WINDOW_MS, DRAIN_MAX_WINDOWS)?;
        self.raw.clear();
        self.rearm_my_presets_request()?;
        self.list_my_presets()
    }

    /// Enumerate the FACTORY list (listEnum = 4). Mirrors [`list_my_presets`]: the
    /// handshake already issued `preset_list_request(4)` and accumulated its reply,
    /// so harvest from the shared accumulator first and only re-query if absent.
    pub fn list_factory_presets(&mut self) -> Result<Vec<PresetEntry>, String> {
        if best_factory_list_from_reports(&self.raw).is_none() {
            self.send_and_collect(
                &proto::preset_list_request(4, proto::BATCH_DRAIN),
                KEEPALIVE_WINDOW_MS,
            )?;
        }
        // A decodable list is not a complete one (the My-Presets truncation class).
        self.pump_until_stable(FACTORY_LIST_MAX_WINDOWS, KEEPALIVE_WINDOW_MS, |s| {
            best_factory_list_from_reports(&s.raw).map_or(0, |n| n.len())
        })?;
        let names = best_factory_list_from_reports(&self.raw);
        let names = names
            .ok_or_else(|| "no Factory PresetListResponse received from device".to_string())?;
        Ok(preset_entries(names))
    }

    /// Completeness-validated My-Presets list for the snapshot path: completeness is
    /// proven by the terminal 0x35 frame, not judged by time (the tolerant harvest once
    /// served 371 of 504 here, after a heavy field-8 sweep left the line flooded).
    /// 1. strict harvest of the handshake reports, waiting out a reply still in flight;
    /// 2. retries by RE-ARMING the open session — WITHOUT clearing `raw`, because
    ///    `assemble_startup_snapshot` harvests the startup graph from these same reports
    ///    (appending is safe: strict rejects the old truncated streams);
    /// 3. falls back to the tolerant list with a warning — a warned short list beats a
    ///    failed connect (truncation is tail-only, and the next reconnect re-reads).
    ///
    /// Every wait is kept alive ([`HID_INACTIVITY_DROP_MS`])
    /// and none ends in a lapse: the monitor keeps this session live straight after.
    ///
    /// NOT for the leveller/probe/clear call sites — they shape their own bursts
    /// and a re-arm would clobber their accumulator timing; they stay on the
    /// tolerant `list_my_presets`.
    pub fn list_my_presets_strict(&mut self) -> Result<Vec<PresetEntry>, String> {
        let mut names = if self.best_preset_list().is_some() {
            self.await_strict_list()?
        } else {
            None
        };
        for _attempt in 0..2 {
            if names.is_some() {
                break;
            }
            self.drain_until_quiet(DRAIN_QUIET_WINDOW_MS, DRAIN_MAX_WINDOWS)?;
            self.rearm_my_presets_request()?;
            names = self.await_strict_list()?;
        }
        if let Some(n) = names {
            return Ok(preset_entries(n));
        }
        let tolerant = self
            .best_preset_list()
            .ok_or_else(|| "no PresetListResponse received from device".to_string())?;
        log::warn!(
            "list_my_presets_strict: no complete decode after retries; serving the \
             tolerant longest-wins list ({} records — tail may be truncated)",
            tolerant.len()
        );
        Ok(preset_entries(tolerant))
    }

    /// Best My-Presets list decoded from all reports accumulated so far. Tries BOTH
    /// reassembly rules:
    /// - `streams()` keeps an interleaved 0x35 as its own stream, preserving list reads
    ///   that arrive mid-flood.
    /// - `streams_final()` folds a terminal 0x35 into the open stream, preserving the
    ///   final record tail ("Empty" was observed as "Empt" when this frame was dropped).
    ///
    /// Longest decoded list wins; complete list responses should have equal record
    /// counts except for the final-frame tail case.
    pub(super) fn best_preset_list(&self) -> Option<Vec<String>> {
        best_preset_list_from_reports(&self.raw)
    }

    /// Strict, completeness-validated My-Presets list: `streams_final()` only
    /// (terminal-0x35 rule) + strict parsing at every level, so a tail-truncated
    /// multi-packet response is rejected rather than clipped. Records missing a
    /// displayName map to "" (NOT dropped) so list indices — and therefore the
    /// `userSlot = index + 1` invariant — are preserved. Longest COMPLETE wins.
    pub fn harvest_preset_list_strict(&self) -> Option<Vec<String>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::preset_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .map(|r| {
                        proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }
}
