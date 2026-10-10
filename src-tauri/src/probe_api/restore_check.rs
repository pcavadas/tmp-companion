//! `probe --restore-check <deviceSlot>`: does a same-slot `loadPreset` discard an UNSAVED
//! edit? fw 1.8.58 skips a same-slot load while the working copy is clean (tmp-audit Q4: the
//! dirty flag clear, and the load identity and current scene unchanged), and
//! `leveller::restore_saved_preset` is the app's one "discard unsaved edits" path.
//!
//! For each edit kind — a `presetLevel` set (the HW-shown control, Q34(d)), a `bypass` flip
//! and a float write (both `changeParameter`), and an `insertNode` append — this applies the
//! edit unsaved on its own connection and confirms it in the working copy, then either sends
//! the bare same-slot load (`raw`) or runs the product's `restore_saved_preset` (`product`),
//! and reads the working copy back on a fresh connection.
//!
//! Scratch-zone slots only. It saves nothing, ends every trial on a real reload (another
//! scratch slot, then the target), reloads the preset that was active at the start, and
//! sends a fresh-connection re-amp OFF.

use super::SCRATCH_SLOTS;
use crate::leveller;
use crate::session::Session;
use serde_json::Value;

/// Appended by the insert trial: one cheap block, legal in any guitar group.
const INSERT_FENDER_ID: &str = "ACD_Boost";

#[derive(Clone, Debug)]
enum Edit {
    Level {
        from: f64,
        to: f32,
    },
    Bypass {
        group: String,
        node: String,
        from: bool,
    },
    Float {
        group: String,
        node: String,
        param: String,
        from: f64,
        to: f32,
    },
    Insert {
        group: String,
        was: usize,
    },
}

impl Edit {
    fn label(&self) -> String {
        match self {
            Edit::Level { from, to } => format!("setPresetLevel {from:.3}→{to:.3}"),
            Edit::Bypass { group, node, from } => {
                format!("changeParameter {group}/{node}.bypass {from}→{}", !from)
            }
            Edit::Float {
                group,
                node,
                param,
                from,
                to,
            } => {
                format!("changeParameter {group}/{node}.{param} {from:.3}→{to:.3}")
            }
            Edit::Insert { group, .. } => {
                format!("insertNode {INSERT_FENDER_ID} → {group} (append)")
            }
        }
    }

    /// The edited field as `doc` holds it, for the report.
    fn observed(&self, doc: &Value) -> String {
        match self {
            Edit::Level { .. } => format!("{:?}", crate::audiograph::preset_level(doc)),
            Edit::Bypass { group, node, .. } => format!("{:?}", param(doc, group, node, "bypass")),
            Edit::Float {
                group,
                node,
                param: p,
                ..
            } => format!("{:?}", param(doc, group, node, p)),
            Edit::Insert { group, .. } => {
                format!("{} × {INSERT_FENDER_ID}", insert_count(doc, group))
            }
        }
    }

    /// `Some(true)` = `doc` shows the edit, `Some(false)` = the original, `None` = neither.
    fn shows_edit(&self, doc: &Value) -> Option<bool> {
        match self {
            Edit::Level { from, to } => {
                let now = crate::audiograph::preset_level(doc)?;
                close(now, f64::from(*to))
                    .then_some(true)
                    .or_else(|| close(now, *from).then_some(false))
            }
            Edit::Bypass { group, node, from } => param(doc, group, node, "bypass")?
                .as_bool()
                .map(|b| b != *from),
            Edit::Float {
                group,
                node,
                param: p,
                from,
                to,
            } => {
                let now = param(doc, group, node, p)?.as_f64()?;
                close(now, f64::from(*to))
                    .then_some(true)
                    .or_else(|| close(now, *from).then_some(false))
            }
            Edit::Insert { group, was } => {
                let now = insert_count(doc, group);
                (now == was + 1)
                    .then_some(true)
                    .or_else(|| (now == *was).then_some(false))
            }
        }
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn group_nodes<'a>(doc: &'a Value, group: &str) -> impl Iterator<Item = &'a Value> {
    doc.pointer(&format!("/audioGraph/guitarNodes/{group}"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn param<'a>(doc: &'a Value, group: &str, node: &str, p: &str) -> Option<&'a Value> {
    group_nodes(doc, group)
        .find(|n| crate::audiograph::node_id(n) == Some(node))?
        .get("dspUnitParameters")?
        .get(p)
}

fn insert_count(doc: &Value, group: &str) -> usize {
    group_nodes(doc, group)
        .filter(|n| n.get("FenderId").and_then(Value::as_str) == Some(INSERT_FENDER_ID))
        .count()
}

/// The four trial edits, picked off the slot's loaded working copy: G1's first node with a
/// bool `bypass`, and its first node with a float `level`/`outputLevel`/`gain`.
fn pick_edits(doc: &Value) -> Result<Vec<Edit>, String> {
    let level = crate::audiograph::preset_level(doc).ok_or("no presetLevel in the working copy")?;
    let to_level = if level > 0.5 {
        level - 0.2
    } else {
        level + 0.2
    };
    let mut edits = vec![Edit::Level {
        from: level,
        to: to_level as f32,
    }];
    let g = "G1".to_string();
    let bypass = group_nodes(doc, &g).find_map(|n| {
        let b = n.get("dspUnitParameters")?.get("bypass")?.as_bool()?;
        Some(Edit::Bypass {
            group: g.clone(),
            node: crate::audiograph::node_id(n)?.to_string(),
            from: b,
        })
    });
    edits.push(bypass.ok_or("G1 has no node with a bool bypass")?);
    let float = group_nodes(doc, &g).find_map(|n| {
        let dp = n.get("dspUnitParameters")?;
        let (p, v) = ["level", "outputLevel", "gain"]
            .iter()
            .find_map(|p| Some((*p, dp.get(*p)?.as_f64()?)))?;
        let to = if v > 0.5 { v - 0.1 } else { v + 0.1 };
        Some(Edit::Float {
            group: g.clone(),
            node: crate::audiograph::node_id(n)?.to_string(),
            param: p.to_string(),
            from: v,
            to: to as f32,
        })
    });
    edits.push(float.ok_or("G1 has no node with a float level/outputLevel/gain")?);
    edits.push(Edit::Insert {
        group: g.clone(),
        was: insert_count(doc, &g),
    });
    Ok(edits)
}

/// The trial slot: its list index, its name (every read is checked against it, so a
/// document from the bounce slot can never pass for the target's), and the bounce slot.
struct Target {
    list: u32,
    name: String,
    other: u32,
}

/// What a fresh connection sees: the dirty flag its handshake reported, and the working copy.
struct Seen {
    dirty: Option<bool>,
    doc: Value,
}

impl Target {
    fn check_name(&self, doc: Value) -> Result<Value, String> {
        match doc.pointer("/info/displayName").and_then(Value::as_str) {
            Some(n) if n == self.name => Ok(doc),
            n => Err(format!("the working copy is {n:?}, not {:?}", self.name)),
        }
    }

    /// A guaranteed real reload: load the bounce slot, then the target, on one connection,
    /// waiting for the target's own `currentPresetInfoChanged`.
    fn real_load(&self) -> Result<Seen, String> {
        let mut s = Session::connect()?;
        s.load_preset(self.other)?;
        for _ in 0..3 {
            s.heartbeat()?;
            s.pump_collect_alive(150)?;
        }
        s.clear_raw();
        s.load_preset(self.list)?;
        if !s.await_active_preset(&self.name, 10) || s.loaded_slot() != Some(self.list) {
            return Err(format!(
                "real reload of list {} not confirmed (loaded {:?}, active {:?})",
                self.list,
                s.loaded_slot(),
                s.active_preset_name()
            ));
        }
        let dirty = s.active_preset_dirty();
        let doc = self.check_name(s.live_complete_value()?)?;
        Ok(Seen { dirty, doc })
    }

    /// The dirty flag and the working copy, read on a fresh connection.
    fn read(&self) -> Result<Seen, String> {
        let mut s = Session::connect()?;
        s.rearm_active_info()?;
        s.await_active_preset(&self.name, 8);
        let dirty = s.active_preset_dirty();
        let doc = self.check_name(s.live_complete_value()?)?;
        Ok(Seen { dirty, doc })
    }

    /// The bare same-slot load on a fresh connection: the field-3 push it draws, then a
    /// working-copy re-prompt.
    fn bare_same_slot_load(&self) -> Result<(Option<Value>, Value), String> {
        let mut s = Session::connect_lean()?;
        s.clear_raw();
        s.load_preset(self.list)?;
        for _ in 0..5 {
            s.heartbeat()?;
            s.pump_collect_alive(200)?;
        }
        let push = s
            .complete_graph_value()
            .ok()
            .and_then(|d| self.check_name(d).ok());
        Ok((push, self.check_name(s.live_complete_value()?)?))
    }
}

/// Apply `edit` unsaved on its own connection and prove it is in the working copy.
fn apply(t: &Target, edit: &Edit) -> Result<(), String> {
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    match edit {
        Edit::Level { to, .. } => {
            s.set_preset_level(*to)?;
        }
        Edit::Bypass { group, node, from } => {
            s.change_parameter_bool(group, node, "bypass", !from)?
        }
        Edit::Float {
            group,
            node,
            param,
            to,
            ..
        } => s.change_parameter(group, node, param, *to)?,
        Edit::Insert { group, was } => {
            let roster =
                crate::blockcaps::roster_from_preset(&t.check_name(s.live_complete_value()?)?);
            crate::blockcaps::check_insert(
                &roster,
                &mut crate::blockcaps::counts(&roster),
                group,
                None,
                INSERT_FENDER_ID,
            )?;
            if !s.insert_node_once(group, None, INSERT_FENDER_ID, Some(*was))? {
                return Err(format!(
                    "insert unconfirmed (presetError={})",
                    s.saw_preset_error()
                ));
            }
        }
    }
    let doc = t.check_name(s.live_complete_value()?)?;
    match edit.shows_edit(&doc) {
        Some(true) => Ok(()),
        _ => Err(format!(
            "the edit is not in the working copy ({})",
            verdict(edit, &doc)
        )),
    }
}

fn verdict(edit: &Edit, doc: &Value) -> String {
    let v = match edit.shows_edit(doc) {
        Some(true) => "KEPT the edit (no-op)",
        Some(false) => "REVERTED (real reload)",
        None => "neither value",
    };
    format!("{v} [{}]", edit.observed(doc))
}

fn timed_restore(list: u32) -> Result<u128, String> {
    let t0 = std::time::Instant::now();
    leveller::restore_saved_preset(list)?;
    Ok(t0.elapsed().as_millis())
}

fn trial(t: &Target, edit: &Edit, product: bool, out: &mut String) -> Result<(), String> {
    apply(t, edit)?;
    let after = t.read()?;
    out.push_str(&format!("    after the edit: isDirty={:?}\n", after.dirty));
    if product {
        let ms = timed_restore(t.list)?;
        let seen = t.read()?;
        out.push_str(&format!(
            "    product restore_saved_preset ({ms} ms) → isDirty={:?}, working copy {}\n",
            seen.dirty,
            verdict(edit, &seen.doc)
        ));
    } else {
        let (push, wc) = t.bare_same_slot_load()?;
        let seen = t.read()?;
        out.push_str(&format!(
            "    raw same-slot load → field-3 push {}, re-prompt {}; then isDirty={:?}\n",
            push.as_ref()
                .map_or("absent".to_string(), |p| verdict(edit, p)),
            verdict(edit, &wc),
            seen.dirty
        ));
    }
    let clean = t.real_load()?;
    if edit.shows_edit(&clean.doc) != Some(false) {
        return Err(format!(
            "cleanup real reload did not restore the original ({}) — stop and inspect slot {}",
            verdict(edit, &clean.doc),
            t.list + 1
        ));
    }
    out.push_str(&format!(
        "    cleanup real reload → isDirty={:?}\n",
        clean.dirty
    ));
    Ok(())
}

pub fn probe_restore_check(device_slot: u32, trials: u32) -> Result<String, String> {
    let list = device_slot.checked_sub(1).ok_or("the slot is 1-based")?;
    if !SCRATCH_SLOTS.contains(&list) {
        return Err(format!(
            "list {list} is outside the scratch zone {SCRATCH_SLOTS:?} — refusing a working-copy edit"
        ));
    }
    let other = if list == SCRATCH_SLOTS[0] {
        list + 1
    } else {
        list - 1
    };
    let _reamp_off = super::ReampOffGuard;

    // Where the unit started, so it can be left as found, and the target's name.
    let (start_name, start_list, name) = {
        let mut s = Session::connect()?;
        let presets = s.list_my_presets()?;
        s.rearm_active_info()?;
        s.pump_collect_alive(400)?;
        let active = s.active_preset_name();
        let hits: Vec<u32> = presets
            .iter()
            .filter(|p| Some(&p.name) == active.as_ref())
            .map(|p| p.slot)
            .collect();
        let name = presets
            .iter()
            .find(|p| p.slot == list)
            .map(|p| p.name.clone())
            .filter(|n| !crate::session::is_empty_slot_name(n))
            .ok_or_else(|| format!("list {list} is empty or not listed"))?;
        (active, (hits.len() == 1).then(|| hits[0]), name)
    };
    let t = Target { list, name, other };
    let mut out = format!(
        "[probe --restore-check] list {list} {:?} (device slot {device_slot}), bounce list {other}; \
         started on {start_name:?} (list {start_list:?})\n",
        t.name
    );

    let res = (|| -> Result<(), String> {
        let base = t.real_load()?;
        out.push_str(&format!(
            "  baseline real reload → isDirty={:?}\n",
            base.dirty
        ));
        // A clean flag: the restore cannot rely on the same-slot load, so it loads a
        // neighbour first. The read proves the target ended up loaded and clean.
        for _ in 0..trials {
            let ms = timed_restore(t.list)?;
            let seen = t.read()?;
            out.push_str(&format!(
                "  clean flag: product restore_saved_preset ({ms} ms) → isDirty={:?}, target loaded\n",
                seen.dirty
            ));
        }
        for edit in pick_edits(&base.doc)? {
            out.push_str(&format!("  {}\n", edit.label()));
            for _ in 0..trials {
                trial(&t, &edit, false, &mut out)?;
                trial(&t, &edit, true, &mut out)?;
            }
        }
        Ok(())
    })();

    // Leave the unit as found: a real reload of the starting preset when it is known, so a
    // trial that failed mid-edit cannot leave its unsaved edit active.
    let back = match start_list {
        Some(l) if l != list => Session::connect().and_then(|mut s| {
            s.load_preset(l)?;
            s.heartbeat()?;
            s.pump_collect_alive(400)?;
            Ok(format!(
                "reloaded list {l} (loaded echo {:?})",
                s.loaded_slot()
            ))
        }),
        Some(_) => t
            .real_load()
            .map(|_| format!("reloaded list {list} (the starting preset)")),
        None => Ok("starting preset unknown — left on the target".to_string()),
    };
    let back = back.map_err(|e| format!("reload of the starting preset FAILED: {e}"));
    out.push_str(&format!(
        "  end: {}\n",
        back.as_deref().unwrap_or_else(|e| e.as_str())
    ));
    match (res, back) {
        (Ok(()), Ok(_)) => Ok(out),
        (Err(e), _) | (Ok(()), Err(e)) => Err(format!("{e}\n{out}")),
    }
}
