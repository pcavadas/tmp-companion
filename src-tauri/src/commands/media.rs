//! Audition render + spectrum/EQ-match measurement Tauri commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ─── Audition (MEASURE — re-amp render → WAV data URL for playback) ──────────────

/// Standard-alphabet base64 (no padding omitted) — small + dependency-free, for
/// embedding a rendered WAV as a `data:` URL the webview can play.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Encode mono f32 samples as a 32-bit-float WAV (in memory).
pub(crate) fn wav_bytes(samples: &[f32], rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut w =
            hound::WavWriter::new(&mut cursor, spec).map_err(|e| format!("wav writer: {e}"))?;
        for &s in samples {
            w.write_sample(s).map_err(|e| format!("wav write: {e}"))?;
        }
        w.finalize().map_err(|e| format!("wav finalize: {e}"))?;
    }
    Ok(cursor.into_inner())
}

/// Re-amp a preset and return its processed audio as a `data:audio/wav;base64,…` URL
/// the frontend `<audio>` element can play (MEASURE — drives the device,
/// HW-pending). A/B and before/after are a later refinement (render two + compare).
#[tauri::command]
pub(crate) async fn audition_render(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Cache hit → return the already-rendered clip, skipping the re-amp pass.
    let cache_key = audition::clip_key(slot, topology_id.as_deref().unwrap_or("default"));
    if let Some(url) = lock_ok(&state.clip_cache).get(&cache_key) {
        return Ok(url);
    }
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    let url = with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let wav = wav_bytes(&samples, rate)?;
        Ok(format!("data:audio/wav;base64,{}", base64_encode(&wav)))
    })
    .await?;
    lock_ok(&state.clip_cache).insert(&cache_key, url.clone());
    Ok(url)
}

// ─── Spectrum report (MEASURE — re-amp capture + band analysis) ──────────────────

/// Per-band energies + tonal flags for one preset.
#[derive(serde::Serialize)]
pub(crate) struct SpectrumResult {
    bands: Vec<f64>,
    flags: Vec<String>,
}

/// Re-amp a preset and analyze its captured spectrum (MEASURE — drives the device;
/// HW-validation pending). Reuses the leveller's validated capture sequence.
#[tauri::command]
pub(crate) async fn spectrum_scan(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<SpectrumResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let bands = spectrum::band_energies(&samples, rate as f32, &spectrum::default_bands());
        let flags = spectrum::tonal_flags(&bands);
        Ok(SpectrumResult { bands, flags })
    })
    .await
}

/// EQ-match: source vs reference spectra + the per-band gain deltas that move
/// source toward reference, with a preview of the matched spectrum.
#[derive(serde::Serialize)]
pub(crate) struct EqMatchResult {
    source_bands: Vec<f64>,
    reference_bands: Vec<f64>,
    distance: f64,
    deltas: Vec<f64>,
    matched_bands: Vec<f64>,
}

/// Re-amp two presets and compute the EQ-match from `source` toward `reference`
/// (MEASURE — drives the device; HW-validation pending).
#[tauri::command]
pub(crate) async fn eq_match(
    app: tauri::AppHandle,
    source_slot: u32,
    reference_slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<EqMatchResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (s, sr) = leveller::capture_samples(source_slot, &stim, 0.5)?;
        let source_bands = spectrum::band_energies(&s, sr as f32, &cfg);
        let (r, rr) = leveller::capture_samples(reference_slot, &stim, 0.5)?;
        let reference_bands = spectrum::band_energies(&r, rr as f32, &cfg);
        let distance = spectrum::spectral_distance(&source_bands, &reference_bands);
        let deltas = spectrum::eq_match_deltas(&source_bands, &reference_bands);
        let matched_bands = spectrum::apply_deltas(&source_bands, &deltas);
        Ok(EqMatchResult {
            source_bands,
            reference_bands,
            distance,
            deltas,
            matched_bands,
        })
    })
    .await
}

/// Re-amp a target + candidate presets and rank the candidates by spectral distance
/// to the target, nearest first ("best match" — MEASURE; HW-pending).
#[tauri::command]
pub(crate) async fn rank_candidates(
    app: tauri::AppHandle,
    target_slot: u32,
    candidate_slots: Vec<u32>,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<spectrum::SicRank>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (t, tr) = leveller::capture_samples(target_slot, &stim, 0.5)?;
        let target = spectrum::band_energies(&t, tr as f32, &cfg);
        let mut cands = Vec::with_capacity(candidate_slots.len());
        for slot in candidate_slots {
            let (c, cr) = leveller::capture_samples(slot, &stim, 0.5)?;
            cands.push((
                format!("slot {slot}"),
                spectrum::band_energies(&c, cr as f32, &cfg),
            ));
        }
        Ok(spectrum::rank_sics(&target, &cands))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn wav_bytes_has_riff_header_and_round_trips() {
        let samples = [0.0f32, 0.5, -0.5, 1.0];
        let bytes = wav_bytes(&samples, 48000).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // Decodes back to the same samples via hound.
        let mut rdr = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let got: Vec<f32> = rdr.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(got, samples);
    }
}
