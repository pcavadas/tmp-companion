//! Preset-list entry types and the name/slot helpers shared by the list, read and
//! command modules. Pure: no transport, no clock, no I/O.

use super::*;

/// One entry in the device's "My Presets" list. `slot` is the **0-based list
/// index** ([`ListIndex`], what `Session::load_preset` and friends take — the
/// device's 1-based `userSlot` is [`DeviceSlot::from`] it); `name` is the display name.
/// NOTE: the list-index → userSlot mapping is confirmed on real hardware in M3
/// by loading a slot and reading back the current-preset info. Do NOT print
/// this field labeled "slot" without the index/userSlot distinction — that
/// ambiguity once steered a probe run at the wrong preset.
#[derive(Debug, Clone, Serialize)]
pub struct PresetEntry {
    /// The entry's 0-based list index (serialized as a plain number).
    pub slot: ListIndex,
    pub name: String,
}

/// True if a "My Presets" display name marks an EMPTY slot. The device shows
/// `--` at cleared positions (observed on 1.7.75); also treat a blank/`Empty`
/// name as empty defensively. Used to pick a scratch slot and to skip empties
/// when locating an imported preset.
pub fn is_empty_slot_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n == "--" || n == "\u{2014}" || n.eq_ignore_ascii_case("empty")
}

/// The one My Presets position `name` occupies; `None` for a duplicated, absent or
/// empty-slot name (it can't prove which preset is active).
pub(crate) fn unique_list_index(names: &[String], name: &str) -> Option<ListIndex> {
    if is_empty_slot_name(name) {
        return None;
    }
    let mut hits = names.iter().enumerate().filter(|(_, n)| *n == name);
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Some(ListIndex(i as u32)),
        _ => None,
    }
}

/// True iff `name` appears at EXACTLY ONE position in the My Presets `names` list and
/// that position is `list_index`. The uniqueness proof the [`Session::active_matches`]
/// name fallback needs before it may confirm a save target: a duplicated display name
/// (two slots, same name) or a name that matches no/other slot must fail closed.
pub(crate) fn name_maps_uniquely(names: &[String], name: &str, list_index: ListIndex) -> bool {
    let mut hit = None;
    for (i, n) in names.iter().enumerate() {
        if n == name {
            if hit.is_some() {
                return false; // duplicate name ⇒ can't prove which slot is the target
            }
            hit = Some(ListIndex(i as u32));
        }
    }
    hit == Some(list_index)
}

pub(super) fn unique_preset_slot_by_name(presets: &[PresetEntry], name: &str) -> Option<ListIndex> {
    let mut matches = presets.iter().filter(|p| p.name == name);
    let slot = matches.next()?.slot;
    matches.next().is_none().then_some(slot)
}

impl Session {
    /// Resolve the active graph to a 0-based My Presets list index when the
    /// truncated live JSON and handshake events did not carry a slot. The
    /// CurrentPresetInfoChanged display name is still available, so a unique
    /// name match is addressable. Duplicate names deliberately stay unresolved.
    pub fn resolve_unique_my_preset_slot(&mut self, name: Option<&str>) -> Option<ListIndex> {
        let name = name?;
        let presets = self.list_my_presets().ok()?;
        unique_preset_slot_by_name(&presets, name)
    }
}
