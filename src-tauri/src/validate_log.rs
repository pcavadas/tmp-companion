//! External-validation log (P5): the ONE JSON-lines contract between this app's
//! measurement seams and `scripts/level-validate.sh`, whose ffmpeg `ebur128` read is
//! the only meter in the loop this repo did not write.
//!
//! WHY A LOG AND NOT A SELF-CHECK: a `lufs.rs` regression that fools the Rust unit
//! tests fools every self-consistency assertion too. So the seam that already holds a
//! leveled sound's PCM writes that PCM to disk and records what the run PROMISED
//! (`target_lufs`) next to it; an independent BS.1770 implementation then judges the
//! file. Nothing here measures anything.
//!
//! WHY EMISSION LIVES AT THE RE-MEASURE, NOT AT THE SOLVE: the solve captures at its
//! REFERENCE level, so its PCM is not the saved preset's output. Only the strict
//! re-measure path ([`crate::leveller::measure_sound_asis_strict`]) captures the sound
//! a player would actually hear from the SAVED state, which is the only thing worth
//! validating externally.
//!
//! OPT-IN AND INERT BY DEFAULT: every entry point returns immediately unless
//! `TMP_E2E_VALIDATE_LOG` is set, checked BEFORE any work — a production run never
//! allocates, never writes a WAV, never touches the filesystem. This mirrors the
//! `--dump-wav` add-on pattern in `probe_api/level.rs`.
//!
//! THE ROW CARRIES IDENTITY, NEVER A POSITION. `scene_slot`/`switch` are stamped by
//! the caller that measured that exact sound. An earlier design zipped a scene batch's
//! results positionally against its request; the batched runner FILTERS failed scenes
//! out of the returned vec (`commands/level_scenes.rs`), so any mid-batch failure
//! either mislabels every later row or (with a length guard) drops the whole batch
//! silently — both of which read as a green run with nothing checked.
//!
//! FAILURE IS EMITTED, NEVER SWALLOWED. A row whose WAV dump failed still lands, with
//! `wav: null` + `wav_error`, so the shell FAILs it by name. Emitting nothing would
//! leave the consumer with zero rows, which it reports as "nothing to re-measure" —
//! the exact false-green this file exists to prevent.

use crate::audio;
use crate::lufs;

/// Path of the JSON-lines log, when external validation is armed. `None` (the
/// production default) means every other entry point here is a no-op.
pub(crate) fn log_path() -> Option<String> {
    std::env::var("TMP_E2E_VALIDATE_LOG")
        .ok()
        .filter(|p| !p.is_empty())
}

/// Path of the DOCTOR log — a SEPARATE file, and separate arming, from the leveling one.
///
/// WHY NOT THE SAME FILE. The leveling consumer (`scripts/level-validate.sh`) grades every
/// row against `target_lufs` and treats a row it cannot parse a target from as a hard FAIL.
/// A Doctor sound has no loudness target at all — it is captured so an independent tool can
/// re-judge the SPECTRAL premise a diagnosis rests on — so mixing the two into one file
/// would either wreck the leveling gate or force a fake target into the Doctor rows. Two
/// files, one row shape, no cross-contamination.
///
/// Inert by default, exactly like [`log_path`]: unset means [`emit_doctor`] returns before
/// any work.
pub(crate) fn doctor_log_path() -> Option<String> {
    std::env::var("TMP_E2E_DOCTOR_LOG")
        .ok()
        .filter(|p| !p.is_empty())
}

/// Directory the dumped WAVs land in: `TMP_E2E_VALIDATE_WAV_DIR`, else a
/// `level-validate-wavs` sibling of the log itself, so a caller that sets only the log
/// path still gets self-describing artifacts next to it.
fn wav_dir(log: &str) -> String {
    if let Ok(dir) = std::env::var("TMP_E2E_VALIDATE_WAV_DIR") {
        if !dir.is_empty() {
            return dir;
        }
    }
    let parent = std::path::Path::new(log)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .unwrap_or(".");
    format!("{parent}/level-validate-wavs")
}

/// `[A-Za-z0-9._-]`-only filename stem — labels carry `:` separators, which the shell
/// side would have to quote everywhere. The shell never RECONSTRUCTS this: it reads the
/// emitted `wav` path verbatim. This only keeps a dump directory readable.
fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// One external-validation expectation: WHICH sound was re-measured, and what the
/// leveling run promised for it. Built by the measuring caller (which owns the
/// identity) — never derived by zipping a result vec against a request.
#[derive(Debug, Clone)]
pub struct ValidationRow {
    /// Human-readable identity, e.g. `base:slot404` / `scene:slot404:scene2` /
    /// `footswitch:slot404:switch11`. Only a display/filename handle — the machine
    /// identity is the `slot`/`scene_slot`/`switch` triple below.
    pub label: String,
    /// 0-based list index (the app's own slot space, NOT the 1-based device slot).
    pub slot: u32,
    /// 0-based `scenes[]` wire index for a scene sound; `None` for base/footswitch.
    pub scene_slot: Option<u32>,
    /// Footswitch handle for a footswitch sound; `None` otherwise.
    pub switch: Option<u32>,
    /// What the run promised this sound would render at. The consumer compares the
    /// independent read against THIS — never against the run's own
    /// `predicted_lufs`/`verify_lufs`, which would make the check self-referential.
    pub target_lufs: f64,
    /// True when the solve could not reach `target_lufs` (unreachable, clamped at the
    /// control's limit). The consumer reports these as SKIP: asserting a clamped row
    /// against a target it was never able to hit is a guaranteed false red.
    pub clamped: bool,
    /// Forwarded from the run's post-save re-read: `Some(true)` = the saved preset does
    /// NOT hold the value the result reports, so the number is untrustworthy and the
    /// consumer SKIPs. `None` = not checked.
    pub persist_mismatch: Option<bool>,
    /// Where this row's WAV goes, when the caller names it explicitly (probe's
    /// `--dump-wav <dir>`). `None` = the env/default resolution in [`wav_dir`]. Carried
    /// on the row rather than pushed into the environment: a probe arm mutating
    /// process env to steer a library seam is invisible at the call site and outlives it.
    pub wav_dir: Option<String>,
}

impl ValidationRow {
    /// The base sound of `slot`.
    pub fn base(slot: u32, target_lufs: f64) -> Self {
        Self {
            label: format!("base:slot{slot}"),
            slot,
            scene_slot: None,
            switch: None,
            target_lufs,
            clamped: false,
            persist_mismatch: None,
            wav_dir: None,
        }
    }

    /// One scene sound of `slot` (0-based `scenes[]` wire index).
    pub fn scene(slot: u32, scene_slot: u32, target_lufs: f64) -> Self {
        Self {
            label: format!("scene:slot{slot}:scene{scene_slot}"),
            slot,
            scene_slot: Some(scene_slot),
            switch: None,
            target_lufs,
            clamped: false,
            persist_mismatch: None,
            wav_dir: None,
        }
    }

    /// One footswitch's ENGAGED sound on `slot`.
    pub fn footswitch(slot: u32, switch: u32, target_lufs: f64) -> Self {
        Self {
            label: format!("footswitch:slot{slot}:switch{switch}"),
            slot,
            scene_slot: None,
            switch: Some(switch),
            target_lufs,
            clamped: false,
            persist_mismatch: None,
            wav_dir: None,
        }
    }

    /// ONE Doctor sound of `slot`, by the same identity triple every other row uses:
    /// `scene` for a scene sound, `switch` for a footswitch sound, neither for base.
    ///
    /// `target_lufs` is left at `0.0` here and OVERWRITTEN by [`emit_doctor`] with what THIS
    /// repo measured for the capture — a Doctor sound has no loudness target, so the field
    /// carries the cross-check number instead of an assertion, and it cannot be known until
    /// the capture exists. A Doctor log's consumer must never grade it as a target miss,
    /// which is precisely why Doctor rows live in their own file ([`doctor_log_path`]).
    pub fn doctor(slot: u32, scene_slot: Option<u32>, switch: Option<u32>) -> Self {
        let what = match (scene_slot, switch) {
            (_, Some(sw)) => format!("switch{sw}"),
            (Some(sc), None) => format!("scene{sc}"),
            (None, None) => "base".to_string(),
        };
        Self {
            label: format!("doctor:slot{slot}:{what}"),
            slot,
            scene_slot,
            switch,
            target_lufs: 0.0,
            clamped: false,
            persist_mismatch: None,
            wav_dir: None,
        }
    }

    pub fn with_flags(mut self, clamped: bool, persist_mismatch: Option<bool>) -> Self {
        self.clamped = clamped;
        self.persist_mismatch = persist_mismatch;
        self
    }

    /// Pin this row's WAV directory (probe's `--dump-wav <dir>`), overriding
    /// `TMP_E2E_VALIDATE_WAV_DIR` and the beside-the-log default.
    pub fn with_wav_dir(mut self, dir: &str) -> Self {
        self.wav_dir = Some(dir.to_string());
        self
    }
}

/// A finite integrated loudness at or below this is genuinely down at the device's
/// stationary output floor — no leveled sound lands anywhere near it (the shipped targets
/// are −23/−21/−19 LUFS, and even the deliberately-quiet fixture scene models ≈ −58). Above
/// it, a `floor` stamp is far more likely to be [`crate::leveller::is_engaged`]'s
/// spread criterion misfiring than a failed inject — see [`engaged_verdict`].
const FLOOR_SUSPECT_FLOOR_LUFS: f64 = -55.0;

/// The engage verdict the consumer keys its "failed inject, not a level miss" FAIL on.
///
/// `danger.md`: a silent/failed re-amp inject reads as the device's STATIONARY OUTPUT
/// FLOOR, and a floor capture's ebur128 read is a real number for the WRONG signal —
/// noise the shell cannot possibly filter on its own. Stamping the verdict here (from
/// the same criterion `probe --measure-current`'s FLOOR/SILENT headline uses) is what
/// keeps the shell from ever having to grep a probe log.
///
/// FOUR verdicts, not three, because the criterion has a KNOWN false positive.
/// `is_engaged`'s `spread > 0.5` arm (documented in `CLAUDE.md`) stamps FLOOR on perfectly
/// valid captures of the stationary stimulus through a COMPRESSED chain — the sound is
/// loud, on-target and real, but its short-term-max sits within 0.5 LU of its integrated
/// reading. Failing those hard is a FALSE RED that would sink a correct 45-minute online
/// run, and softening the criterion itself would re-open the failed-inject hole it exists
/// to close. So the two cases are SPLIT by absolute level:
///
/// * `floor` — flat AND down at the floor (≤ [`FLOOR_SUSPECT_FLOOR_LUFS`]). The genuine
///   failed-inject signature; still a hard FAIL downstream.
/// * `floor_suspect` — flat but LOUD. The consumer WARNs and still grades the row against
///   its target with ffmpeg, so the independent meter stays the authority on whether the
///   leveling was right; only the engage proof is downgraded to advisory.
///
/// ORDER IS LOAD-BEARING: the `is_finite` arm comes first, so a `-inf` integrated is
/// `silent` and never reaches the level comparison below (which would read `-inf <= -55`
/// as a floor and lose the distinction). Do not "simplify" the ordering.
fn engaged_verdict(loud: &lufs::Loudness) -> &'static str {
    if !loud.integrated_lufs.is_finite() {
        "silent"
    } else if crate::leveller::is_engaged(loud) {
        "engaged"
    } else if loud.integrated_lufs > FLOOR_SUSPECT_FLOOR_LUFS {
        "floor_suspect"
    } else {
        "floor"
    }
}

/// JSON-escape the few characters a label could carry. Labels are built from
/// `format!` templates over integers, so this is belt-and-braces rather than a general
/// escaper — but the log MUST stay one flat parseable object per line.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_u32(v: Option<u32>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "null".into())
}

fn opt_bool(v: Option<bool>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "null".into())
}

/// Dump `cap`'s processed pair and append one row to the validation log. NO-OP (and no
/// work at all) unless `TMP_E2E_VALIDATE_LOG` is set.
///
/// `loud` is the loudness this repo measured for the SAME capture — used ONLY for the
/// engage verdict, never written as the answer. The consumer's whole job is to derive
/// its own number from the WAV.
pub(crate) fn emit(row: &ValidationRow, cap: &audio::Capture, loud: &lufs::Loudness) {
    let Some(log) = log_path() else {
        return;
    };
    let dir = row.wav_dir.clone().unwrap_or_else(|| wav_dir(&log));
    emit_to(&log, &dir, row, cap, loud);
}

/// The DOCTOR twin of [`emit`]: dump this Doctor sound's processed pair and append one row to
/// the Doctor log. NO-OP (and no work at all) unless `TMP_E2E_DOCTOR_LOG` is set, so a
/// production Doctor run never allocates, never writes a WAV, never touches the filesystem.
///
/// SAME row shape and SAME dumper as the leveling seam — an external judge that can read one
/// log can read the other. What differs is only WHICH file it lands in and what the numbers
/// mean (see [`ValidationRow::doctor`]).
pub(crate) fn emit_doctor(row: &ValidationRow, cap: &audio::Capture, loud: &lufs::Loudness) {
    let Some(log) = doctor_log_path() else {
        return;
    };
    let dir = row.wav_dir.clone().unwrap_or_else(|| wav_dir(&log));
    // The measured loudness IS the row's number for a Doctor sound (see `ValidationRow::doctor`).
    let row = ValidationRow {
        target_lufs: loud.integrated_lufs,
        ..row.clone()
    };
    emit_to(&log, &dir, &row, cap, loud);
}

/// [`emit`] with the destinations resolved — the env-free seam the unit tests drive
/// (setting `TMP_E2E_VALIDATE_LOG` in a test would arm emission for every OTHER test
/// running in parallel, since env is process-global).
/// A `<dir>/<stem>.wav` that does not exist yet — `stem`, else `stem-2`, `stem-3`, …
///
/// `dump_processed_capture` deliberately OVERWRITES on a repeated label (its own doc says
/// so; the probe's `--dump-wav` A/B relies on it). That is wrong for the expectation log:
/// two rows CAN legitimately share a label within one run — an operator asking for the
/// same sound twice, a spec re-measuring after a retry — and the second dump would then
/// silently replace the audio the FIRST row's line already points at, so the judge would
/// grade one capture twice and never see the other. Rows are cheap; a silently-substituted
/// capture is the false-green this whole file exists to prevent.
///
/// The counter is bounded: past the cap the base stem is returned and the overwrite is
/// accepted rather than spinning (a run producing 999 same-label rows has a bigger
/// problem, and a dump is never worth an unbounded loop).
fn unique_wav_stem(dir: &str, stem: &str) -> String {
    const MAX_TRIES: u32 = 999;
    let taken = |s: &str| std::path::Path::new(dir).join(format!("{s}.wav")).exists();
    if !taken(stem) {
        return stem.to_string();
    }
    for n in 2..=MAX_TRIES {
        let candidate = format!("{stem}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    stem.to_string()
}

fn emit_to(log: &str, dir: &str, row: &ValidationRow, cap: &audio::Capture, loud: &lufs::Loudness) {
    let stem = unique_wav_stem(dir, &sanitize(&row.label));
    let (wav, wav_error) = match crate::probe_api::stimulus::dump_processed_capture(cap, dir, &stem)
    {
        Ok(path) => (json_str(&path), "null".to_string()),
        // A dump failure must still produce a ROW: a swallowed row is an empty log, and
        // an empty log reads to the consumer as "nothing was leveled", i.e. a pass.
        Err(e) => ("null".to_string(), json_str(&e)),
    };
    // `tol_lu: null` = "the consumer's own default applies". Emitted explicitly so the
    // field exists in every row and a future per-row override needs no shape change.
    let line = format!(
        "{{\"label\":{},\"slot\":{},\"scene_slot\":{},\"switch\":{},\"target_lufs\":{:.4},\
         \"tol_lu\":null,\"clamped\":{},\"persist_mismatch\":{},\"wav\":{},\"wav_error\":{},\
         \"engaged\":{}}}",
        json_str(&row.label),
        row.slot,
        opt_u32(row.scene_slot),
        opt_u32(row.switch),
        row.target_lufs,
        row.clamped,
        opt_bool(row.persist_mismatch),
        wav,
        wav_error,
        json_str(engaged_verdict(loud)),
    );
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                log::warn!("validate log: append to {log} failed ({e})");
            }
        }
        Err(e) => log::warn!("validate log: open {log} failed ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(channels: usize) -> audio::Capture {
        let n = 48_000usize;
        let mut interleaved = Vec::with_capacity(n * channels);
        for i in 0..n {
            let s = 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin();
            for _ in 0..channels {
                interleaved.push(s);
            }
        }
        audio::Capture {
            interleaved,
            channels,
            sample_rate: 48_000,
        }
    }

    fn loud(integrated: f64, short_term_max: f64) -> lufs::Loudness {
        lufs::Loudness {
            integrated_lufs: integrated,
            short_term_max_lufs: short_term_max,
            true_peak_dbtp: -6.0,
        }
    }

    #[test]
    fn labels_sanitize_to_a_filename_stem() {
        assert_eq!(sanitize("scene:slot404:scene2"), "scene_slot404_scene2");
        assert_eq!(sanitize("base.slot-1_x"), "base.slot-1_x");
    }

    #[test]
    fn engage_verdict_separates_silent_floor_floor_suspect_and_engaged() {
        assert_eq!(engaged_verdict(&loud(f64::NEG_INFINITY, 0.0)), "silent");
        // Finite but flat and DOWN AT THE FLOOR: the failed-inject signature, hard FAIL.
        assert_eq!(engaged_verdict(&loud(-70.0, -69.9)), "floor");
        assert_eq!(engaged_verdict(&loud(-17.0, -12.0)), "engaged");
        // Flat but LOUD — `is_engaged`'s spread>0.5 arm misfiring on a compressed capture
        // of the stationary stimulus. A real sound at a real level, so the consumer WARNs
        // and still grades it with ffmpeg rather than failing the run outright.
        assert_eq!(engaged_verdict(&loud(-17.0, -16.8)), "floor_suspect");
        // The boundary is exclusive on the floor side: exactly at the constant stays hard.
        assert_eq!(
            engaged_verdict(&loud(
                FLOOR_SUSPECT_FLOOR_LUFS,
                FLOOR_SUSPECT_FLOOR_LUFS + 0.1
            )),
            "floor"
        );
        assert_eq!(
            engaged_verdict(&loud(
                FLOOR_SUSPECT_FLOOR_LUFS + 0.5,
                FLOOR_SUSPECT_FLOOR_LUFS + 0.6
            )),
            "floor_suspect"
        );
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tmp-companion-validate-log-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    // A dump FAILURE (an unwritable directory) must still append a row — otherwise the
    // consumer sees an empty log and reports "nothing to re-measure", i.e. a green run
    // that checked nothing.
    #[test]
    fn a_failed_dump_still_emits_a_row_carrying_the_error() {
        let dir = scratch("dumpfail");
        let log = dir.join("expect.jsonl");
        // A FILE where the dump wants a DIRECTORY — `create_dir_all` then fails.
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"not a dir").expect("write blocker");
        emit_to(
            log.to_str().expect("utf8"),
            blocked.to_str().expect("utf8"),
            &ValidationRow::scene(404, 2, -17.0).with_flags(true, Some(false)),
            &cap(2),
            &loud(-17.2, -12.0),
        );
        let body = std::fs::read_to_string(&log).expect("log written");
        assert!(body.contains("\"wav\":null"), "row kept, no WAV: {body}");
        assert!(
            body.contains("\"wav_error\":\""),
            "names the reason: {body}"
        );
        assert!(body.contains("\"scene_slot\":2"), "identity kept: {body}");
        assert!(body.contains("\"clamped\":true"), "flags kept: {body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The happy path's full contract, field for field — this is the shape
    // `scripts/level-validate.sh` parses, so a silent shape change must fail here.
    #[test]
    fn a_dumped_row_carries_the_wav_path_and_the_engage_verdict() {
        let dir = scratch("dumpok");
        let log = dir.join("expect.jsonl");
        let wavs = dir.join("wavs");
        emit_to(
            log.to_str().expect("utf8"),
            wavs.to_str().expect("utf8"),
            &ValidationRow::footswitch(404, 11, -17.0),
            &cap(2),
            &loud(-17.05, -11.0),
        );
        let body = std::fs::read_to_string(&log).expect("log written");
        assert!(
            body.contains("\"label\":\"footswitch:slot404:switch11\""),
            "{body}"
        );
        assert!(body.contains("\"slot\":404"), "{body}");
        assert!(body.contains("\"scene_slot\":null"), "{body}");
        assert!(body.contains("\"switch\":11"), "{body}");
        assert!(body.contains("\"target_lufs\":-17.0000"), "{body}");
        assert!(body.contains("\"tol_lu\":null"), "{body}");
        assert!(body.contains("\"clamped\":false"), "{body}");
        assert!(body.contains("\"persist_mismatch\":null"), "{body}");
        assert!(body.contains("\"wav_error\":null"), "{body}");
        assert!(body.contains("\"engaged\":\"engaged\""), "{body}");
        let wav = wavs.join("footswitch_slot404_switch11.wav");
        assert!(wav.is_file(), "the dumped WAV exists at {wav:?}");
        assert!(
            body.contains(&format!("\"wav\":\"{}\"", wav.to_str().expect("utf8"))),
            "the row carries the dumper's OWN path verbatim: {body}"
        );
        // One row per emit, newline-terminated: the consumer reads this line by line.
        assert_eq!(body.lines().count(), 1, "{body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Two rows sharing a label must NOT share a WAV: the second dump would overwrite the
    // audio the first row's line names, so ffmpeg would grade one capture twice.
    #[test]
    fn a_repeated_label_gets_its_own_wav_rather_than_overwriting_the_first() {
        let dir = scratch("dupelabel");
        let log = dir.join("expect.jsonl");
        let wavs = dir.join("wavs");
        for _ in 0..2 {
            emit_to(
                log.to_str().expect("utf8"),
                wavs.to_str().expect("utf8"),
                &ValidationRow::base(400, -23.0),
                &cap(2),
                &loud(-23.1, -18.0),
            );
        }
        let body = std::fs::read_to_string(&log).expect("log written");
        assert_eq!(body.lines().count(), 2, "one row per emit: {body}");
        assert!(wavs.join("base_slot400.wav").is_file(), "the first take");
        assert!(
            wavs.join("base_slot400-2.wav").is_file(),
            "the second take got its OWN file, not the first one's"
        );
        assert!(
            body.contains("base_slot400-2.wav"),
            "the second row points at its own capture: {body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ITEM 7 — THE DOCTOR PREMISE-CHECK SEAM. A Doctor sound is captured so an independent
    // tool can re-judge the audio a diagnosis rests on. Three things must hold:
    //
    // 1. the row carries IDENTITY (slot + scene/switch), never a position;
    // 2. `target_lufs` is OVERWRITTEN with what this repo measured — a Doctor sound has no
    //    loudness target, and the number is a cross-check, not an assertion;
    // 3. the row shape is the SAME one the leveling consumer parses, so one judge reads both.
    #[test]
    fn a_doctor_row_carries_identity_and_the_measured_loudness() {
        let dir = scratch("doctorrow");
        let log = dir.join("doctor.jsonl");
        let wavs = dir.join("wavs");
        emit_to(
            log.to_str().expect("utf8"),
            wavs.to_str().expect("utf8"),
            &ValidationRow::doctor(404, None, Some(11)),
            &cap(2),
            &loud(-17.05, -11.0),
        );
        let body = std::fs::read_to_string(&log).expect("log written");
        assert!(
            body.contains("\"label\":\"doctor:slot404:switch11\""),
            "{body}"
        );
        assert!(body.contains("\"slot\":404"), "{body}");
        assert!(body.contains("\"switch\":11"), "{body}");
        assert!(body.contains("\"scene_slot\":null"), "{body}");
        assert!(
            body.contains("\"clamped\":false"),
            "a Doctor row is never a clamp: {body}"
        );
        assert!(
            wavs.join("doctor_slot404_switch11.wav").is_file(),
            "the capture the diagnosis was made from is on disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The three Doctor sound kinds label distinctly — a base sound, a scene sound and a
    // footswitch sound of the same preset must never collide on one WAV.
    #[test]
    fn the_three_doctor_sound_kinds_get_distinct_identities() {
        assert_eq!(
            ValidationRow::doctor(400, None, None).label,
            "doctor:slot400:base"
        );
        assert_eq!(
            ValidationRow::doctor(400, Some(2), None).label,
            "doctor:slot400:scene2"
        );
        assert_eq!(
            ValidationRow::doctor(400, Some(2), Some(5)).label,
            "doctor:slot400:switch5",
            "a footswitch sound is named by its switch even when it sits in a scene"
        );
        let r = ValidationRow::doctor(400, Some(2), Some(5));
        assert_eq!(
            (r.scene_slot, r.switch),
            (Some(2), Some(5)),
            "the MACHINE identity keeps both — only the label picks one"
        );
    }

    // INERT BY DEFAULT, and on its OWN file. `emit_doctor` must do nothing at all unless
    // `TMP_E2E_DOCTOR_LOG` is set: a production Doctor run never writes a WAV. (Arming it
    // here would arm it for every other test in the process, so the check is on the path
    // resolver — the single gate `emit_doctor` returns on.)
    #[test]
    fn the_doctor_log_is_armed_separately_from_the_leveling_one() {
        assert!(
            doctor_log_path().is_none(),
            "unset by default — a production run writes nothing"
        );
    }

    #[test]
    fn the_wav_dir_defaults_beside_the_log() {
        assert_eq!(
            wav_dir("/tmp/run/expect.jsonl"),
            "/tmp/run/level-validate-wavs"
        );
    }
}
