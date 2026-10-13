//! The two address spaces of a My Presets entry, as two types the compiler keeps apart.
//!
//! - [`ListIndex`]: the **0-based** position in the My Presets list (`list_my_presets`, the
//!   UI, `PresetEntry.slot`).
//! - [`DeviceSlot`]: the **1-based** `userSlot` the device addresses presets by on the wire
//!   (`loadPreset`, `saveCurrentPreset`, `clearUserPreset`, `moveUserPreset`, the field-8
//!   slot read, the backup DB's `slot` column). HW-confirmed: device `userSlot` = list
//!   index + 1.
//!
//! A `clear` once deleted a real preset because its guard checked list-index space while
//! the clear acted in device-slot space (`danger.md`). With both spaces a bare `u32`, only
//! hand-written `+ 1` / `- 1` arithmetic at every call site kept them apart. Here the
//! conversion lives in exactly one place ([`From`] both ways), every Session method states
//! its space in its signature, and a guard and the mutation it protects are checked in the
//! same space by the type system.
//!
//! The wire and IPC shapes are unchanged: both types serialize as a plain number
//! (`ListIndex` transparently; `DeviceSlot` through `u32`, refusing 0 on the way in).

use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// A 0-based My Presets list index.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListIndex(pub u32);

impl ListIndex {
    /// The raw 0-based index.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The index as a `usize`, for indexing the list itself.
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    /// The 1-based device slot this list index addresses.
    pub fn device_slot(self) -> DeviceSlot {
        DeviceSlot::from(self)
    }
}

// Debug prints the bare number, so a `{:?}` of a slot list (an error naming the scratch
// zone, say) reads the same as it did when slots were plain integers.
impl fmt::Debug for ListIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for DeviceSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for ListIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A 1-based device `userSlot`. Never 0.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct DeviceSlot(NonZeroU32);

impl DeviceSlot {
    /// The raw 1-based slot.
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The slot as the wire's `u64` field value.
    pub const fn wire(self) -> u64 {
        self.0.get() as u64
    }

    /// The 0-based list index this device slot holds.
    pub fn list_index(self) -> ListIndex {
        ListIndex::from(self)
    }
}

impl From<ListIndex> for DeviceSlot {
    /// List index `i` is device slot `i + 1`. Saturates at `u32::MAX`, far above any real
    /// list (the unit holds a few hundred presets).
    fn from(i: ListIndex) -> Self {
        DeviceSlot(NonZeroU32::MIN.saturating_add(i.0))
    }
}

impl From<DeviceSlot> for ListIndex {
    /// Device slot `s` is list index `s - 1` (never underflows: `s >= 1`).
    fn from(s: DeviceSlot) -> Self {
        ListIndex(s.0.get() - 1)
    }
}

/// The error for a device slot of 0, which addresses nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroDeviceSlot;

impl fmt::Display for ZeroDeviceSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("device slots are 1-based; 0 is not a slot")
    }
}

impl std::error::Error for ZeroDeviceSlot {}

impl TryFrom<u32> for DeviceSlot {
    type Error = ZeroDeviceSlot;
    fn try_from(n: u32) -> Result<Self, Self::Error> {
        NonZeroU32::new(n).map(DeviceSlot).ok_or(ZeroDeviceSlot)
    }
}

impl TryFrom<i64> for DeviceSlot {
    type Error = String;
    /// A backup-DB `slot` column value: refuses 0, negatives and anything past `u32`.
    fn try_from(n: i64) -> Result<Self, Self::Error> {
        u32::try_from(n)
            .ok()
            .and_then(NonZeroU32::new)
            .map(DeviceSlot)
            .ok_or_else(|| format!("device slot {n} is not a 1-based slot"))
    }
}

impl From<DeviceSlot> for u32 {
    fn from(s: DeviceSlot) -> u32 {
        s.get()
    }
}

impl From<DeviceSlot> for i64 {
    fn from(s: DeviceSlot) -> i64 {
        i64::from(s.get())
    }
}

impl std::str::FromStr for DeviceSlot {
    type Err = String;
    /// Parse a CLI device-slot argument: a positive integer, never a silent 0.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let n: u32 = s
            .trim()
            .parse()
            .map_err(|_| format!("{s:?} is not a device slot (a whole number from 1)"))?;
        DeviceSlot::try_from(n).map_err(|e| format!("{s:?}: {e}"))
    }
}

impl std::str::FromStr for ListIndex {
    type Err = String;
    /// Parse a CLI list-index argument: a whole number from 0.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim()
            .parse()
            .map(ListIndex)
            .map_err(|_| format!("{s:?} is not a list index (a whole number from 0)"))
    }
}

impl fmt::Display for DeviceSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Parse a comma-separated slot list (a CLI argument). Any entry that does not parse
/// refuses the whole list: a dropped entry would silently act on fewer presets than asked.
pub fn parse_slot_list<T: std::str::FromStr>(s: &str) -> Result<Vec<T>, String>
where
    T::Err: fmt::Display,
{
    s.split(',')
        .map(str::trim)
        .map(|x| x.parse::<T>().map_err(|e| format!("{x:?}: {e}")))
        .collect()
}

/// A device slot from a literal, for tests.
#[cfg(test)]
pub(crate) fn ds(n: u32) -> DeviceSlot {
    DeviceSlot::try_from(n).expect("test device slots are 1-based")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_index_and_device_slot_are_one_apart() {
        assert_eq!(DeviceSlot::from(ListIndex(0)).get(), 1);
        assert_eq!(ListIndex(27).device_slot().get(), 28);
        assert_eq!(
            ListIndex::from(DeviceSlot::try_from(28u32).unwrap()),
            ListIndex(27)
        );
        for i in [0, 1, 255, 511] {
            assert_eq!(ListIndex(i).device_slot().list_index(), ListIndex(i));
        }
    }

    #[test]
    fn device_slot_zero_is_refused() {
        assert_eq!(DeviceSlot::try_from(0u32), Err(ZeroDeviceSlot));
        assert!(DeviceSlot::try_from(-1i64).is_err());
        assert!(DeviceSlot::try_from(i64::from(u32::MAX) + 1).is_err());
        assert!("0".parse::<DeviceSlot>().is_err());
        assert!("abc".parse::<DeviceSlot>().is_err());
        assert_eq!("12".parse::<DeviceSlot>().unwrap().get(), 12);
        assert!("x".parse::<ListIndex>().is_err());
        assert!(serde_json::from_str::<DeviceSlot>("0").is_err());
    }

    #[test]
    fn slot_lists_refuse_any_bad_entry() {
        let ok: Vec<DeviceSlot> = parse_slot_list("3, 4,5").unwrap();
        assert_eq!(ok.iter().map(|s| s.get()).collect::<Vec<_>>(), [3, 4, 5]);
        assert!(parse_slot_list::<DeviceSlot>("3,abc").is_err());
        assert!(parse_slot_list::<DeviceSlot>("3,0").is_err());
        for bad in ["3,,4", "3,", ""] {
            assert!(parse_slot_list::<DeviceSlot>(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            parse_slot_list::<ListIndex>("0,1").unwrap(),
            [ListIndex(0), ListIndex(1)]
        );
    }

    #[test]
    fn serde_shape_is_a_plain_number() {
        assert_eq!(serde_json::to_string(&ListIndex(4)).unwrap(), "4");
        assert_eq!(
            serde_json::to_string(&DeviceSlot::from(ListIndex(4))).unwrap(),
            "5"
        );
        assert_eq!(
            serde_json::from_str::<ListIndex>("4").unwrap(),
            ListIndex(4)
        );
        assert_eq!(serde_json::from_str::<DeviceSlot>("5").unwrap().get(), 5);
    }
}
