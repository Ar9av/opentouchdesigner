//! A WAV reader, written here rather than pulled in.
//!
//! Phase 2 lists Audio File In, and the honest way to ship it without
//! GStreamer is the one container that needs no codec: RIFF/WAVE PCM. That
//! covers what a show file actually is — a stem bounced from a DAW — and it
//! is small enough that owning the parser is cheaper than owning a
//! dependency. Compressed formats stay out of scope until the video layer
//! brings a real media stack with it.
//!
//! Everything is downmixed to mono on load, for the same reason the audio
//! input does it in its callback: one channel of samples is what the rest of
//! the CHOP world speaks.

/// A decoded file: mono samples and the rate they were recorded at.
#[derive(Debug)]
pub struct Wav {
    pub sample_rate: f64,
    pub samples: Vec<f32>,
}

impl Wav {
    /// Duration in seconds.
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate.max(1.0)
    }
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// Parse a RIFF/WAVE file. PCM in 8, 16, 24 or 32 bits, or 32-bit float —
/// the formats DAWs actually bounce. Anything else is refused by name so the
/// node can say *why* rather than produce silence.
pub fn parse(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }

    let mut format: Option<(u16, u16, u32, u16)> = None; // tag, channels, rate, bits
    let mut data: Option<&[u8]> = None;

    // Walk the chunks; `fmt ` and `data` can arrive in either order and
    // other chunks (LIST, cue, bext from broadcast WAVs) are skipped.
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32le(bytes, at + 4).ok_or("truncated chunk header")? as usize;
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| format!("truncated `{}` chunk", String::from_utf8_lossy(id)))?;
        match id {
            b"fmt " => {
                let mut tag = u16le(body, 0).ok_or("short fmt chunk")?;
                let channels = u16le(body, 2).ok_or("short fmt chunk")?;
                let rate = u32le(body, 4).ok_or("short fmt chunk")?;
                let bits = u16le(body, 14).ok_or("short fmt chunk")?;
                // WAVE_FORMAT_EXTENSIBLE wraps the real tag in a GUID whose
                // first two bytes are the classic code.
                if tag == 0xfffe {
                    tag = u16le(body, 24).ok_or("short extensible fmt chunk")?;
                }
                format = Some((tag, channels, rate, bits));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned; an odd length is padded by one byte.
        at += 8 + len + (len & 1);
    }

    let (tag, channels, rate, bits) = format.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    let channels = channels.max(1) as usize;

    let frames: Vec<f32> = match (tag, bits) {
        // PCM.
        (1, 8) => data.iter().map(|b| (*b as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .chunks_exact(3)
            .map(|c| {
                let v = i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8;
                v as f32 / 8_388_608.0
            })
            .collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
            .collect(),
        // IEEE float.
        (3, 32) => data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        _ => {
            return Err(format!(
                "unsupported WAV format (tag {tag}, {bits}-bit) — PCM or 32-bit float only"
            ));
        }
    };

    let samples = if channels == 1 {
        frames
    } else {
        frames
            .chunks_exact(channels)
            .map(|f| f.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    Ok(Wav {
        sample_rate: rate as f64,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a WAV in memory — the tests own both sides of the format.
    fn wav_bytes(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        let block = channels as u32 * bits as u32 / 8;
        out.extend_from_slice(&(rate * block).to_le_bytes());
        out.extend_from_slice(&(block as u16).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn sixteen_bit_stereo_is_decoded_and_downmixed() {
        // One frame: left = +max, right = -max — the downmix is ~0. A second
        // frame with both at half scale downmixes to half scale.
        let mut data = Vec::new();
        for v in [i16::MAX, i16::MIN, 16384, 16384] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let wav = parse(&wav_bytes(1, 2, 48000, 16, &data)).unwrap();
        assert_eq!(wav.sample_rate, 48000.0);
        assert_eq!(wav.samples.len(), 2);
        assert!(wav.samples[0].abs() < 1e-4);
        assert!((wav.samples[1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn float_mono_survives_exactly() {
        let mut data = Vec::new();
        for v in [0.25f32, -0.75, 1.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let wav = parse(&wav_bytes(3, 1, 44100, 32, &data)).unwrap();
        assert_eq!(wav.samples, vec![0.25, -0.75, 1.0]);
        assert!((wav.duration() - 3.0 / 44100.0).abs() < 1e-9);
    }

    #[test]
    fn twenty_four_bit_reaches_full_scale() {
        // 0x7fffff is +max in 24-bit; 0x800000 is -max.
        let data = [0xff, 0xff, 0x7f, 0x00, 0x00, 0x80];
        let wav = parse(&wav_bytes(1, 1, 44100, 24, &data)).unwrap();
        assert!(wav.samples[0] > 0.999);
        assert!(wav.samples[1] < -0.999);
    }

    #[test]
    fn refusals_name_the_problem() {
        assert!(parse(b"not audio").unwrap_err().contains("not a WAV"));
        let mp3ish = wav_bytes(85, 1, 44100, 0, &[]);
        assert!(parse(&mp3ish).unwrap_err().contains("unsupported"));
        // A data chunk that claims more bytes than exist must not panic.
        let mut truncated = wav_bytes(1, 1, 44100, 16, &[0, 0, 0, 0]);
        let len = truncated.len();
        truncated.truncate(len - 2);
        assert!(parse(&truncated).unwrap_err().contains("truncated"));
    }
}
