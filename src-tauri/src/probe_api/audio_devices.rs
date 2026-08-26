//! Probe entry point: enumerate host audio devices (`probe --audio-devices`).
//!
//! Read-only, no device open, no HID, no re-amp — safe to run any time, with or
//! without the TMP plugged in. This is the instrument `CONTRIBUTING.md`'s
//! "Developing on Linux" section calls for: it prints what `cpal` actually sees
//! on the host (device names, the exact ALSA PCM id behind each name on Linux,
//! channel counts, sample rates, and — the thing `audio::pick_config` cares
//! about — sample formats), so the Linux re-amp port can be designed from
//! measurement rather than a CoreAudio-shaped guess. See `audio.rs::enumerate`
//! and `audio.rs::find_tmp`, which this is the first caller of.

use crate::audio::{self, AudioDevice};

/// `probe --audio-devices`: dump every host audio device cpal enumerates, which
/// one `find_tmp` would select for re-amp, and (Linux only) the `/proc/asound`
/// USB-audio card table so a Fender-VID card can be cross-checked even if the
/// name match misses it.
pub fn probe_audio_devices() -> Result<String, String> {
    let devices = audio::enumerate();
    let picked = audio::find_tmp(&devices).map(|d| d.name.clone());

    let mut report = String::new();
    report.push_str(&format!(
        "[probe --audio-devices] {} device(s) enumerated\n\n",
        devices.len()
    ));
    for d in &devices {
        report.push_str(&format_device(d, picked.as_deref() == Some(&d.name)));
    }
    match &picked {
        Some(name) => report.push_str(&format!("\nfind_tmp() picked: {name:?}\n")),
        None => report.push_str(
            "\nfind_tmp() picked: NONE — no name match and no >=4in/>=4out fallback candidate\n",
        ),
    }
    report.push_str(&format!(
        "(find_tmp() is this report's own diagnostic picker, not what re-amp actually uses — \
         see audio::find_device for the production seam{})\n",
        production_pick_note()
    ));

    report.push_str(&linux_asound_report());
    report.push_str(
        "\nNOTE: read-only; no device opened, no HID, no re-amp. Safe with or without the \
         unit plugged in.\n",
    );
    Ok(report)
}

#[cfg(target_os = "linux")]
fn production_pick_note() -> String {
    match audio::linux_audio_card_id() {
        Some(id) => format!(" — resolves to hw:CARD={id},DEV=0"),
        None => " — resolves to NONE (no Fender-VID card with PCM)".to_string(),
    }
}

#[cfg(not(target_os = "linux"))]
fn production_pick_note() -> String {
    String::new()
}

fn format_device(d: &AudioDevice, picked: bool) -> String {
    let marker = if picked { " ← find_tmp() pick" } else { "" };
    format!(
        "  {:?}{marker}\n    driver:  {}\n    in/out:  {}/{} channels\n    rates:   {:?}\n    formats: {:?}\n",
        d.name,
        d.driver.as_deref().unwrap_or("(same as name)"),
        d.input_channels,
        d.output_channels,
        d.sample_rates,
        d.sample_formats,
    )
}

#[cfg(target_os = "linux")]
fn linux_asound_report() -> String {
    // Fender's vendor id. HW-observed (fw 1.8.58): the TMP enumerates on TWO USB
    // product ids — 0x0044 carries HID control and is MIDI-only with no PCM, while
    // 0x0047 is the 4-in/4-out USB-audio card (see notes/gotchas.md, "The TMP's audio
    // interface is USB id 1ed8:0047"). Walk every card the vendor id owns rather than
    // assuming which product id carries PCM.
    const FENDER_VID: u16 = 0x1ED8;

    let mut out = String::from("\n/proc/asound (Linux): USB-audio cards by vendor id\n");
    let Ok(entries) = std::fs::read_dir("/proc/asound") else {
        out.push_str("  (no /proc/asound — ALSA not present?)\n");
        return out;
    };
    let mut cards: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("card"))
        .collect();
    cards.sort();

    let mut found_any_fender = false;
    for card in cards {
        let dir = format!("/proc/asound/{card}");
        let Ok(usbid) = std::fs::read_to_string(format!("{dir}/usbid")) else {
            continue; // not a USB card
        };
        let Some((vendor, product)) = audio::parse_asound_usbid(&usbid) else {
            out.push_str(&format!("  {card}: usbid {usbid:?} (unparseable)\n"));
            continue;
        };
        if vendor != FENDER_VID {
            continue;
        }
        found_any_fender = true;
        let id = std::fs::read_to_string(format!("{dir}/id"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let has_pcm = std::fs::read_dir(&dir)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().starts_with("pcm"))
            })
            .unwrap_or(false);
        out.push_str(&format!(
            "  {card} (ALSA id {id:?}): usbid {vendor:04x}:{product:04x}, {}\n",
            if has_pcm {
                "HAS PCM (audio)"
            } else {
                "no PCM (e.g. MIDI-only)"
            }
        ));
    }
    if !found_any_fender {
        out.push_str("  (no card with vendor id 1ed8 found — is the unit plugged in?)\n");
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn linux_asound_report() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_device_marks_the_find_tmp_pick() {
        let d = AudioDevice {
            name: "Tone Master Pro, USB Audio".to_string(),
            driver: Some("plughw:CARD=4,DEV=0".to_string()),
            input_channels: 4,
            output_channels: 4,
            sample_rates: vec![48_000],
            sample_formats: vec!["F32".to_string()],
        };
        let picked = format_device(&d, true);
        assert!(picked.contains("← find_tmp() pick"));
        let unpicked = format_device(&d, false);
        assert!(!unpicked.contains("← find_tmp() pick"));
    }
}
