//! WAV encoding for the Doctor's A/B clips: a rendered capture as WAV bytes and a
//! base64 `data:` URL the webview can play.

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
    wav_bytes_channels(samples, 1, rate)
}

/// Encode `channels`-interleaved f32 samples as a 32-bit-float WAV (in memory).
pub(crate) fn wav_bytes_channels(
    interleaved: &[f32],
    channels: u16,
    rate: u32,
) -> Result<Vec<u8>, String> {
    let samples = interleaved;
    let spec = hound::WavSpec {
        channels,
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
