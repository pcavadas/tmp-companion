//! Support-bundle export — tar a snapshot of user-shareable diagnostics (recent
//! logs, the captured device settings, app/OS info, and an opt-in preset) into a
//! single file in ~/Downloads that a user can attach when reporting a problem.
//!
//! Every TEXT member is scrubbed of the user's home-directory path (→ `~`) before
//! it's written, so a shared bundle never leaks the account name.
use crate::*;

/// Keep only the last 200 KB of each log file — enough for a recent trace without
/// shipping a multi-megabyte rotated log.
const LOG_TAIL_CAP: usize = 200 * 1024;

#[derive(Serialize)]
pub(crate) struct SupportBundleResult {
    path: String,
}

/// Write a diagnostics bundle to `<download_dir>/tmp-companion-report-<stamp>.tar`.
///
/// `firmware` / `preset_json` / `preset_name` are supplied by the frontend (the
/// preset is opt-in). Returns the absolute path of the written file.
#[tauri::command]
pub(crate) async fn save_support_bundle<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    firmware: Option<String>,
    preset_json: Option<String>,
    preset_name: Option<String>,
) -> Result<SupportBundleResult, String> {
    // File + subprocess I/O off the invoke thread (the connect_device shape).
    tauri::async_runtime::spawn_blocking(move || {
        build_bundle(&app, firmware, preset_json, preset_name)
    })
    .await
    .map_err(|e| format!("bundle task join: {e}"))?
}

fn build_bundle<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    firmware: Option<String>,
    preset_json: Option<String>,
    preset_name: Option<String>,
) -> Result<SupportBundleResult, String> {
    use tauri::Manager;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock before epoch: {e}"))?
        .as_secs();
    let (stamp, iso) = fmt_utc(now);

    let download = app
        .path()
        .download_dir()
        .map_err(|e| format!("resolve download dir: {e}"))?;
    let out_path = download.join(format!("tmp-companion-report-{stamp}.tar"));

    // Everything text-y gets this stripped → `~`. Empty when HOME is unset (scrub
    // then no-ops, tested).
    let home = std::env::var("HOME").unwrap_or_default();

    let file = std::fs::File::create(&out_path).map_err(|e| format!("create bundle: {e}"))?;
    let mut builder = tar::Builder::new(file);

    // logs/<name> — every *.log in the app log dir, tail-capped + scrubbed.
    if let Ok(log_dir) = app.path().app_log_dir() {
        if let Ok(entries) = std::fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|x| x.to_str()) != Some("log") {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&p) else {
                    continue;
                };
                let capped = tail_cap(&bytes, LOG_TAIL_CAP);
                let name = p.file_name().and_then(|x| x.to_str()).unwrap_or("log.log");
                append_scrubbed(&mut builder, &format!("logs/{name}"), capped, &home, now)?;
            }
        }
    }

    // device-settings.json — the 1B backup-scan capture, when present.
    if let Ok(dir) = profiles::app_config_dir(app) {
        let ds = dir.join("support").join("device-settings.json");
        if let Ok(bytes) = std::fs::read(&ds) {
            append_scrubbed(&mut builder, "device-settings.json", &bytes, &home, now)?;
        }
    }

    // meta.json — app + OS + firmware + the opt-in preset's name. App identity via
    // `app_info` (the one place that owns the display name + the
    // package_info-not-CARGO_PKG_VERSION gotcha).
    let info = app_info(app.clone());
    let meta = serde_json::json!({
        "app": info.name,
        "version": info.version,
        "firmware": firmware,
        "macos": macos_product_version(),
        "created": iso,
        "preset_name": preset_name,
    });
    let meta_str = serde_json::to_string_pretty(&meta).map_err(|e| format!("encode meta: {e}"))?;
    append_scrubbed(&mut builder, "meta.json", meta_str.as_bytes(), &home, now)?;

    // preset-graph.json — only when a preset was picked. Named for what it IS: the
    // app's PARSED signal-chain graph (the shared scan store's ActiveGraph), not the
    // device's raw presetJson — a triager must not mistake it for ground truth.
    if let Some(pj) = preset_json {
        append_scrubbed(&mut builder, "preset-graph.json", pj.as_bytes(), &home, now)?;
    }

    builder
        .finish()
        .map_err(|e| format!("finalize bundle: {e}"))?;

    Ok(SupportBundleResult {
        path: out_path.to_string_lossy().into_owned(),
    })
}

/// Scrub the home path out of one text member, then append it.
fn append_scrubbed<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    bytes: &[u8],
    home: &str,
    mtime: u64,
) -> Result<(), String> {
    let text = scrub_home(&String::from_utf8_lossy(bytes), home);
    append_text(builder, name, text.as_bytes(), mtime)
}

/// Append one text member with mode 0644 and the bundle's mtime.
fn append_text<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    bytes: &[u8],
    mtime: u64,
) -> Result<(), String> {
    let mut h = tar::Header::new_gnu();
    h.set_size(bytes.len() as u64);
    h.set_mode(0o644);
    h.set_mtime(mtime);
    h.set_cksum();
    builder
        .append_data(&mut h, name, std::io::Cursor::new(bytes))
        .map_err(|e| format!("tar append {name}: {e}"))
}

/// `sw_vers -productVersion` (e.g. "14.5"); best-effort `None` on any failure.
fn macos_product_version() -> Option<String> {
    let out = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

// ─── Pure helpers (device/FS-free, unit-tested) ───────────────────────────────

/// Replace every occurrence of the user's home path with `~`. A no-op when `home`
/// is empty (HOME unset) — never turns an empty needle into a `~` explosion.
fn scrub_home(content: &str, home: &str) -> String {
    if home.is_empty() {
        return content.to_string();
    }
    content.replace(home, "~")
}

/// Keep only the LAST `cap` bytes of `bytes` (all of it when shorter).
fn tail_cap(bytes: &[u8], cap: usize) -> &[u8] {
    if bytes.len() <= cap {
        bytes
    } else {
        &bytes[bytes.len() - cap..]
    }
}

/// UTC calendar date `(year, month, day)` for a count of days since 1970-01-01.
/// Howard Hinnant's `civil_from_days` (public-domain), avoiding a chrono dep.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `(compact "yyyymmdd-hhmmss", iso "yyyy-mm-ddThh:mm:ssZ")` for a UNIX timestamp.
fn fmt_utc(secs: u64) -> (String, String) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    (
        format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}"),
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_replaces_home_and_no_ops_without_it() {
        let home = "/Users/alice";
        assert_eq!(
            scrub_home("log at /Users/alice/Library/Logs/x.log", home),
            "log at ~/Library/Logs/x.log"
        );
        // Content without the home path is untouched.
        assert_eq!(scrub_home("no home here", home), "no home here");
        // Empty HOME → identity (never explodes into `~~~`).
        assert_eq!(scrub_home("/Users/alice/x", ""), "/Users/alice/x");
    }

    #[test]
    fn tail_cap_keeps_the_last_bytes_at_the_boundary() {
        let data: Vec<u8> = (0..10u8).collect();
        // Shorter than cap → all of it.
        assert_eq!(tail_cap(&data, 20), &data[..]);
        // Exactly cap → all of it (boundary).
        assert_eq!(tail_cap(&data, 10), &data[..]);
        // Longer than cap → the LAST `cap` bytes.
        assert_eq!(tail_cap(&data, 3), &[7, 8, 9]);
    }

    #[test]
    fn fmt_utc_formats_known_timestamps() {
        assert_eq!(
            fmt_utc(0),
            (
                "19700101-000000".to_string(),
                "1970-01-01T00:00:00Z".to_string()
            )
        );
        // 1_600_000_000 == 2020-09-13T12:26:40Z.
        assert_eq!(
            fmt_utc(1_600_000_000),
            (
                "20200913-122640".to_string(),
                "2020-09-13T12:26:40Z".to_string()
            )
        );
    }
}
