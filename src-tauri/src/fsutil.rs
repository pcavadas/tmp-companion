//! Crash-safe file writes for user-owned data (#280).
//!
//! Every write of data the user would miss (the settings store, the saved-block library,
//! the revert snapshots `bulk_revert` restores from, a calibration capture, the cached
//! device settings) goes through [`write_atomic`], or through [`commit`] when a library
//! writes the temp file itself. A plain `fs::write` truncates in place, so a crash or a
//! full disk mid-write leaves a half-written file; a rename with no `sync_all` can land
//! before the data does. Here the bytes reach disk first, then an atomic same-directory
//! rename swaps them in, then the directory entry is synced.
//!
//! Also home to [`app_config_dir_for_cli`], the app's config dir for code that runs
//! without a Tauri `AppHandle` (the `probe` CLI).

use std::io::Write;
use std::path::{Path, PathBuf};

// Test-only failure injection: when set, `write_atomic` fails its temp-file write right
// after `File::create`. Root ignores permission bits (and cloud test runs are root), so a
// read-only directory cannot stand in for this.
#[cfg(test)]
thread_local! {
    static FAIL_TEMP_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Scoped [`FAIL_TEMP_WRITE`]: on while the guard lives, off again on drop (even on a
/// panic), so one test's injection can never leak into the next.
#[cfg(test)]
pub(crate) struct FailTempWrite;

#[cfg(test)]
impl FailTempWrite {
    pub(crate) fn on() -> Self {
        FAIL_TEMP_WRITE.with(|f| f.set(true));
        Self
    }
}

#[cfg(test)]
impl Drop for FailTempWrite {
    fn drop(&mut self) {
        FAIL_TEMP_WRITE.with(|f| f.set(false));
    }
}

/// Held for the whole of a [`write_atomic`] call. The writes are small and rare (a
/// settings save, a snapshot), so one process-wide lock costs nothing.
static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Remove whatever sits at `path`, a leftover file or directory alike. Errors are
/// ignored: an already-gone path is the common case.
pub(crate) fn remove_any(path: &Path) {
    // `remove_file` handles the common case (a plain leftover file) and any
    // already-gone path (its error is simply ignored). It errors on a
    // directory, so only THEN fall back to `remove_dir_all` — checked via
    // `path.exists()` so a merely-already-gone path doesn't trigger it too.
    if std::fs::remove_file(path).is_err() && path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// The deterministic temp path [`write_atomic`] stages `path` in: `<path>.tmp`, in the
/// same directory, so the final rename is a same-filesystem atomic replace.
pub(crate) fn temp_path(path: &Path) -> Result<PathBuf, String> {
    let mut name = path
        .file_name()
        .ok_or_else(|| format!("no file name in path {}", path.display()))?
        .to_os_string();
    name.push(".tmp");
    Ok(path.with_file_name(name))
}

/// Write `bytes` to `path` atomically, creating parent dirs as needed: write a
/// same-directory temp file, `sync_all` it, `rename` it onto `path`, then sync the
/// directory. A failure at any step leaves the previous `path` untouched and no temp
/// file behind.
///
/// The temp path is deterministic rather than uniquely named, so it is self-healing
/// against a leftover from an earlier failed write: whatever sits there is cleared FIRST,
/// and again on this call's own failure path, so one bad leftover can never wedge every
/// future write.
///
/// Writes are serialized process-wide ([`WRITE_LOCK`]): two calls for the same `path`
/// share its temp path, so without the lock one could clear or rename away the other's
/// staged bytes (the settings commands and `calibrate_profile` both save `profiles.json`).
///
/// Windows keeps the atomic replace and syncs the temp file's contents, but cannot sync
/// the directory entry, so a successful write there is not guaranteed to survive a
/// sudden power loss.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _serialized = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = temp_path(path)?;
    remove_any(&tmp);
    let staged: Result<(), String> = (|| {
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
        #[cfg(test)]
        if FAIL_TEMP_WRITE.with(std::cell::Cell::get) {
            return Err(format!("write {}: injected test failure", tmp.display()));
        }
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", tmp.display()))
    })();
    if let Err(e) = staged {
        remove_any(&tmp);
        return Err(e);
    }
    commit(&tmp, path)
}

/// Finish an atomic write whose temp file `tmp` someone else wrote (a WAV encoder that
/// only takes a path, say): `sync_all` it, rename it onto `path`, sync the directory.
/// `tmp` must sit in `path`'s directory. On failure `tmp` is removed and `path` is left
/// as it was.
pub(crate) fn commit(tmp: &Path, path: &Path) -> Result<(), String> {
    let synced = std::fs::OpenOptions::new()
        .write(true)
        .open(tmp)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("sync {}: {e}", tmp.display()));
    if let Err(e) = synced {
        remove_any(tmp);
        return Err(e);
    }
    std::fs::rename(tmp, path).map_err(|e| {
        remove_any(tmp);
        format!("rename {} to {}: {e}", tmp.display(), path.display())
    })?;
    // The rename is durable only once the DIRECTORY ENTRY itself is synced: without
    // this, a power loss right after a successful rename can still lose it. `File::open`
    // on a directory succeeds read-only, so no write access to the parent is needed.
    // Unix only: Windows refuses to open a directory as a plain file.
    #[cfg(unix)]
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| format!("sync parent dir {}: {e}", parent.display()))?;
    }
    Ok(())
}

/// The app's bundle identifier, as `tauri.conf.json` declares it (a test pins the two
/// together). Tauri's per-app directories are named after it.
pub(crate) const APP_IDENTIFIER: &str = "dev.tmpcompanion.app";

/// The directory Tauri's `app_config_dir()` resolves to, for code with no `AppHandle`
/// (the `probe` CLI): the platform config dir joined with [`APP_IDENTIFIER`]. macOS:
/// `~/Library/Application Support`; Windows: `%APPDATA%`; elsewhere `$XDG_CONFIG_HOME`
/// when absolute, else `~/.config`. `None` when the home or `%APPDATA%` variable is unset.
pub(crate) fn app_config_dir_for_cli() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join(APP_IDENTIFIER))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tmp-companion-fsutil-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_write_creates_parents_and_leaves_no_temp_file_behind() {
        let dir = tmp_dir("happy");
        let path = dir.join("a").join("b").join("data.json");
        write_atomic(&path, b"one").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert!(!temp_path(&path).unwrap().exists(), "temp file leaked");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_write_replaces_the_file_and_a_failed_rename_cleans_up() {
        let dir = tmp_dir("replace");
        let path = dir.join("data.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // A non-empty destination directory makes rename fail on every platform.
        let blocked = dir.join("blocked.json");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("sentinel"), b"keep").unwrap();
        assert!(write_atomic(&blocked, b"x")
            .unwrap_err()
            .starts_with("rename "));
        assert_eq!(std::fs::read(blocked.join("sentinel")).unwrap(), b"keep");
        assert!(!dir.join("blocked.json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_temp_write_leaves_the_original_bytes_untouched() {
        // The regression this module exists for: `fs::write` truncates first, so a
        // write that died mid-flight left a half-written file. A failure before the
        // rename can only ever touch the discarded temp file.
        let dir = tmp_dir("failure");
        let path = dir.join("data.json");
        write_atomic(&path, b"original").unwrap();
        let result = {
            let _fail = FailTempWrite::on();
            write_atomic(&path, b"replacement")
        };
        assert!(result.is_err(), "an injected failure must fail the write");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(!temp_path(&path).unwrap().exists(), "temp file leaked");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_writes_to_one_path_all_succeed() {
        // They share `<path>.tmp`: unserialized, one call could rename the other's staged
        // bytes away and fail on its own rename.
        let dir = tmp_dir("concurrent");
        let path = dir.join("data.json");
        let writers: Vec<_> = (0..8u8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    (0..25).try_for_each(|_| write_atomic(&path, &[i; 4096]))
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap().unwrap();
        }
        let got = std::fs::read(&path).unwrap();
        assert!(
            got.len() == 4096 && got.iter().all(|&b| b == got[0]),
            "torn write"
        );
        assert!(!temp_path(&path).unwrap().exists(), "temp file leaked");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_leftover_temp_directory_cannot_wedge_later_writes() {
        let dir = tmp_dir("leftover");
        let path = dir.join("data.json");
        std::fs::create_dir_all(temp_path(&path).unwrap().join("junk")).unwrap();
        write_atomic(&path, b"ok").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn commit_finishes_a_temp_file_written_elsewhere() {
        let dir = tmp_dir("commit");
        let (tmp, path) = (dir.join("clip.wav.part"), dir.join("clip.wav"));
        std::fs::write(&tmp, b"RIFF").unwrap();
        commit(&tmp, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"RIFF");
        assert!(!tmp.exists());
        // A missing temp file is an error, and the destination is left alone.
        assert!(commit(&tmp, &path).unwrap_err().starts_with("sync "));
        assert_eq!(std::fs::read(&path).unwrap(), b"RIFF");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_identifier_matches_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json is JSON");
        assert_eq!(conf["identifier"], APP_IDENTIFIER);
    }
}
