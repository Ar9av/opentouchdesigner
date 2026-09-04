//! Reading an audio file.
//!
//! Two paths, chosen by what the file turns out to be — the same split the
//! movie node makes, and for the same reasons.
//!
//!  * **RIFF/WAVE PCM** is parsed here, in process. It is the one container
//!    that needs no codec, it is what a show file actually is — a stem
//!    bounced from a DAW — and owning the parser is cheaper than owning a
//!    dependency. No subprocess for the case that matters most.
//!  * **Everything else** — m4a, mp3, ogg, flac, the audio track of a movie
//!    — goes through `ffmpeg`, which the video layer already requires. It
//!    was always the plan to wait for "a real media stack"; the media stack
//!    arrived with Movie File In, and there is no reason for a podcast to be
//!    unopenable on a machine that plays video.
//!
//! Everything is downmixed to mono on load, for the same reason the audio
//! input does it in its callback: one channel of samples is what the rest of
//! the CHOP world speaks.

use std::path::Path;
use std::process::{Command, Stdio};

/// What ffmpeg is asked to resample to.
///
/// Fixing the rate is what lets the decode be one subprocess: asking for a
/// known format means the bytes coming back need no header and no probe.
/// Resampling preserves pitch and duration, so nothing about the sound
/// depends on this number.
pub const DECODED_RATE: f64 = 48_000.0;

/// A decoded file: mono samples and the rate they were recorded at.
#[derive(Debug)]
pub struct Wav {
    pub sample_rate: f64,
    pub samples: Vec<f32>,
}

/// Read an audio file, whatever it is.
///
/// WAV is tried first and costs a parse. Anything else costs an ffmpeg run,
/// which is why the order is this way round and not "ask ffmpeg what it is".
pub fn load(path: &Path) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match parse(&bytes) {
        Ok(wav) => Ok(wav),
        // Not WAV, or WAV this parser cannot read. Either way ffmpeg gets a
        // go: it decodes the compressed formats, and it also reads WAVs that
        // are unusual enough to defeat a parser this size.
        Err(why) => decode(path).map_err(|e| match e {
            // ffmpeg is not here, so `why` is the whole story.
            DecodeError::NoFfmpeg(missing) => format!("{why} — {missing}"),
            DecodeError::Failed(e) => e,
        }),
    }
}

enum DecodeError {
    NoFfmpeg(String),
    Failed(String),
}

/// Turn ffmpeg's complaint into one line worth showing on a node.
///
/// The *first* line is the specific one; the lines after it are the generic
/// "error opening output files" summary, which tells a user nothing they can
/// act on. Both are prefixed with a `[out#0/s16le @ 0x…]` tag that is noise
/// outside a terminal.
fn why_it_failed(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let detail = match first.starts_with('[') {
        true => first.split_once("] ").map(|(_, rest)| rest).unwrap_or(first),
        false => first,
    };
    // A picture, or a movie with no soundtrack: ffmpeg reports this as an
    // *output* problem, because `-vn` left it with nothing to write.
    if detail.contains("does not contain any stream") {
        return "no audio track in that file".to_string();
    }
    match detail.is_empty() {
        true => "ffmpeg could not decode it".to_string(),
        false => detail.to_string(),
    }
}

/// Hand the file to ffmpeg and take mono PCM back.
///
/// Raw `s16le` on stdout rather than a WAV: a WAV written to a pipe cannot
/// have its length patched in afterwards, so ffmpeg emits a placeholder size
/// that a strict reader is right to reject. Asking for headerless samples at
/// a rate we chose avoids the problem instead of coping with it.
fn decode(path: &Path) -> Result<Wav, DecodeError> {
    let Some(ffmpeg) = otd_core::tools::ffmpeg() else {
        return Err(DecodeError::NoFfmpeg(otd_core::tools::missing_ffmpeg()));
    };
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        // `-vn`: album art is a video stream, and decoding it produces
        // nothing anybody asked for.
        .args(["-vn", "-ac", "1"])
        .args(["-ar", &format!("{DECODED_RATE:.0}"), "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| DecodeError::Failed(format!("could not run ffmpeg: {e}")))?;

    if !out.status.success() {
        return Err(DecodeError::Failed(why_it_failed(&out.stderr)));
    }
    if out.stdout.is_empty() {
        return Err(DecodeError::Failed(
            "no audio track in that file".to_string(),
        ));
    }
    Ok(Wav {
        sample_rate: DECODED_RATE,
        samples: out
            .stdout
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
    })
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

    #[test]
    fn a_wav_is_read_without_reaching_for_a_subprocess() {
        // The format a show file is in must not depend on ffmpeg being
        // installed, and must not pay for a process to open.
        let bytes = wav_bytes(1, 1, 44100, 16, &[0, 0, 0, 0x40]);
        let dir = std::env::temp_dir().join("otd-wav-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tone.wav");
        std::fs::write(&path, &bytes).unwrap();

        let wav = load(&path).unwrap();
        // Its own rate, not the rate ffmpeg would have been asked for.
        assert_eq!(wav.sample_rate, 44100.0);
        assert_ne!(wav.sample_rate, DECODED_RATE);
        assert_eq!(wav.samples.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_says_so_rather_than_blaming_the_format() {
        let err = load(std::path::Path::new("/no/such/audio.wav")).unwrap_err();
        assert!(err.contains("No such file") || err.contains("cannot find"), "{err}");
    }

    #[test]
    fn ffmpegs_complaint_becomes_one_line_worth_reading() {
        // A picture has no audio, and ffmpeg reports that as an *output*
        // problem because `-vn` left it nothing to write. The user does not
        // care about output files; they care that there is no sound in it.
        let no_stream = b"[out#0/s16le @ 0x14f605e40] Output file does not contain any stream\n\
                          Error opening output file -.\n\
                          Error opening output files: Invalid argument\n";
        assert_eq!(why_it_failed(no_stream), "no audio track in that file");

        // Anything else keeps ffmpeg's words, minus the bracketed tag, and
        // takes the first line: the ones after it are the generic summary.
        let bad_data = b"[in#0 @ 0xad2c10000] Error opening input: Invalid data found\n\
                         Error opening input files: Invalid data found\n";
        assert_eq!(
            why_it_failed(bad_data),
            "Error opening input: Invalid data found"
        );

        assert_eq!(why_it_failed(b""), "ffmpeg could not decode it");
    }

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
