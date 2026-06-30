//! Audition clip cache: re-auditioning a preset returns the already-rendered
//! clip instead of re-running the (expensive) re-amp pass. The render itself lives in
//! `lib::audition_render` (MEASURE — drives the device); this is the pure in-memory
//! cache it consults.
//!
//! A clip handle is the rendered `data:audio/wav;base64,…` URL. The key tags the slot
//! plus the state that affects the audio (the pickup topology) — re-rendering with a
//! different topology is a cache miss. Caveat: an OFFLINE edit to the preset is NOT
//! reflected in the tag, so the cache lives only for the app session and a freshly
//! edited preset keeps its prior clip until restart (re-audition is a session win, not
//! a correctness oracle).

use std::collections::HashMap;

/// Cache key for one preset's rendered clip: slot + a tag of the state that affects
/// the audio (the topology id), so a different render is a distinct entry.
pub fn clip_key(slot: u32, state_tag: &str) -> String {
    format!("{slot}:{state_tag}")
}

/// Caches rendered clip handles (WAV data URLs) by key so re-auditioning skips re-amp.
#[derive(Debug, Default)]
pub struct ClipCache {
    entries: HashMap<String, String>,
}

impl ClipCache {
    /// The cached clip handle for `key`, or `None` on a miss.
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries.get(key).cloned()
    }

    /// Store a freshly rendered clip handle under `key`.
    pub fn insert(&mut self, key: &str, handle: String) {
        self.entries.insert(key.to_string(), handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC — a cache hit returns the stored handle; a different state tag is a miss.
    #[test]
    fn clip_cache_get_insert_and_keying() {
        let mut cache = ClipCache::default();
        let key = clip_key(11, "humbucker");
        assert_eq!(cache.get(&key), None);

        cache.insert(&key, "data:audio/wav;base64,AAAA".into());
        assert_eq!(
            cache.get(&key).as_deref(),
            Some("data:audio/wav;base64,AAAA")
        );

        // A different topology tag → distinct key → still a miss.
        assert_eq!(cache.get(&clip_key(11, "single-coil")), None);
        // A different slot → distinct key.
        assert_eq!(cache.get(&clip_key(12, "humbucker")), None);
    }
}
