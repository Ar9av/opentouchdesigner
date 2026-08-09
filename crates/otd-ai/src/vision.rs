//! The reference image: loaded, shrunk, and turned into something four
//! different providers will each accept.
//!
//! Point a still at the assistant and ask for the look back as operators. The
//! hard part is not the asking, it is that the same picture has to go out as
//! an Anthropic content block, an OpenAI data URI, a line of Claude Code's
//! stream-json, and a file on disk for Codex — so it is decoded once, here,
//! and handed out in whichever shape is wanted.
//!
//! **Everything is shrunk on the way in.** A 4K screenshot is eight megabytes
//! and about nine thousand tokens of image, and no provider does anything
//! useful with the extra pixels: past roughly 1568 on the long edge they all
//! resize it themselves, having already charged for it. Shrinking first is the
//! difference between a reference image costing a fifth of the request and
//! costing four times the rest of it.
//!
//! **The re-encode is chosen, not fixed.** A photograph or a screen grab goes
//! out as JPEG, which is where the size actually lives; anything with real
//! transparency stays PNG, because flattening an alpha channel onto white
//! changes what the model is being asked about.
//!
//! No `base64` crate: it is thirty lines and this crate has one dependency
//! that is not `serde_json` already.

use std::path::{Path, PathBuf};

/// The longest edge we send. Anthropic, OpenAI and both CLIs resize anything
/// larger server-side, so pixels past this are billed and then discarded.
const MAX_EDGE: u32 = 1568;

/// The preview in the prompt bar. Small enough to keep in a struct that gets
/// cloned onto a worker thread without anybody thinking about it.
const THUMB_EDGE: u32 = 96;

/// Refuse anything past this *after* shrinking. Nothing legitimate gets here —
/// it means an image that is enormous in a way resizing did not fix — and the
/// providers reject it less politely than we can.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// A reference image, ready to send.
#[derive(Clone, PartialEq)]
pub struct Image {
    /// Re-encoded bytes, already shrunk. Not the file on disk.
    bytes: Vec<u8>,
    /// `image/png` or `image/jpeg` — what `bytes` actually is now, which is
    /// not necessarily what the file was.
    media_type: &'static str,
    /// Where it came from, for the label in the UI. A pasted image has none.
    source: Option<PathBuf>,
    /// Dimensions after shrinking, for the label.
    pub width: u32,
    pub height: u32,
    /// A small RGBA preview for the bar, so the UI never decodes anything.
    pub thumb: Thumb,
}

/// Straight RGBA8, the one thing every UI toolkit takes without an argument.
#[derive(Clone, PartialEq)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Read, decode, shrink and re-encode a file.
    ///
    /// Errors are for showing a user: an unreadable file and a file that is
    /// not an image are different problems and say so.
    pub fn load(path: impl AsRef<Path>) -> Result<Image, String> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut image = Image::decode(&bytes)?;
        image.source = Some(path.to_path_buf());
        Ok(image)
    }

    /// The same, for bytes that never had a file — a paste, or a frame
    /// grabbed out of the renderer.
    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let decoded = image::load_from_memory(bytes)
            .map_err(|e| format!("not an image this build can read: {e}"))?;

        // `thumbnail` rather than `resize`: it is a box filter and about ten
        // times quicker, and nothing here is being printed.
        let (w, h) = (decoded.width(), decoded.height());
        let shrunk = match w.max(h) > MAX_EDGE {
            true => {
                let scale = MAX_EDGE as f32 / w.max(h) as f32;
                let (nw, nh) = (
                    ((w as f32 * scale).round() as u32).max(1),
                    ((h as f32 * scale).round() as u32).max(1),
                );
                decoded.thumbnail(nw, nh)
            }
            false => decoded,
        };

        // Transparency is information — a reference with a cut-out subject
        // means something different flattened onto white — so anything using
        // its alpha channel stays PNG and pays for it.
        let media_type = match uses_alpha(&shrunk) {
            true => "image/png",
            false => "image/jpeg",
        };
        let bytes = encode(&shrunk, media_type)?;
        if bytes.len() > MAX_BYTES {
            return Err(format!(
                "that image is {} even after shrinking — save it smaller and try again",
                human(bytes.len())
            ));
        }

        let thumb = make_thumb(&shrunk);
        Ok(Image {
            bytes,
            media_type,
            source: None,
            width: shrunk.width(),
            height: shrunk.height(),
            thumb,
        })
    }

    pub fn media_type(&self) -> &'static str {
        self.media_type
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// What the file was called, for the chip in the bar.
    pub fn name(&self) -> String {
        self.source
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "pasted image".into())
    }

    /// `{width}×{height}, 240 KB` — the whole of what the UI says about it.
    pub fn detail(&self) -> String {
        format!(
            "{}×{}, {}",
            self.width,
            self.height,
            human(self.bytes.len())
        )
    }

    /// Base64, for the two wire formats that inline it.
    pub fn base64(&self) -> String {
        base64(&self.bytes)
    }

    /// A `data:` URI, which is how OpenAI and OpenRouter take an image.
    pub fn data_uri(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.base64())
    }

    /// Write the *processed* bytes somewhere Codex can read them, since its
    /// `-i` takes a path rather than data.
    ///
    /// Deliberately not the original file: this way a pasted image works, and
    /// the shrink applies either way rather than only when we inline it.
    pub fn write_temp(&self) -> Result<PathBuf, String> {
        let extension = match self.media_type {
            "image/png" => "png",
            _ => "jpg",
        };
        // Content-addressed-ish: same image, same path, so a repeated ask does
        // not litter, and two different images never collide.
        let name = format!(
            "otd-reference-{:016x}.{extension}",
            fingerprint(&self.bytes)
        );
        let path = std::env::temp_dir().join(name);
        if !path.exists() {
            std::fs::write(&path, &self.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(path)
    }
}

/// Deliberately not printing the bytes. A `{:?}` on a request should not
/// produce four megabytes of base64 in a log.
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}, {})", self.name(), self.detail())
    }
}

/// Whether any pixel is actually see-through. An RGBA image whose alpha is
/// 255 everywhere — which most screenshots are — is a JPEG that has not
/// noticed yet.
fn uses_alpha(image: &image::DynamicImage) -> bool {
    use image::GenericImageView;
    if !image.color().has_alpha() {
        return false;
    }
    image.pixels().any(|(_, _, pixel)| pixel.0[3] < 255)
}

fn encode(image: &image::DynamicImage, media_type: &str) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    let result = match media_type {
        "image/png" => image.write_to(&mut out, image::ImageFormat::Png),
        _ => {
            // Quality 85: the point where a screen grab stops getting smaller
            // and starts getting worse.
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
            image.to_rgb8().write_with_encoder(encoder)
        }
    };
    result.map_err(|e| format!("could not re-encode the image: {e}"))?;
    Ok(out.into_inner())
}

fn make_thumb(image: &image::DynamicImage) -> Thumb {
    let small = image.thumbnail(THUMB_EDGE, THUMB_EDGE).to_rgba8();
    Thumb {
        width: small.width(),
        height: small.height(),
        rgba: small.into_raw(),
    }
}

/// Enough to tell two images apart in a filename. Not a hash anybody should
/// rely on for anything else.
fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a, over the length and the bytes.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes.len().to_le_bytes().iter().chain(bytes.iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn human(bytes: usize) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded. Thirty lines against a dependency and a supply
/// chain, for something with one right answer that has not moved since 1987.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(match chunk.len() > 1 {
            true => ALPHABET[(triple >> 6) as usize & 63] as char,
            false => '=',
        });
        out.push(match chunk.len() > 2 {
            true => ALPHABET[triple as usize & 63] as char,
            false => '=',
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid-colour image of a given size, as PNG bytes.
    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let buffer = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn base64_matches_the_examples_everybody_tests_against() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Bytes above 127 are where a hand-rolled encoder usually goes wrong.
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn a_large_image_is_shrunk_to_the_long_edge() {
        // 4000×2000 is an ordinary screenshot on an ordinary monitor, and the
        // whole point is that it does not go out at that size.
        let image = Image::decode(&png(4000, 2000, [10, 20, 30, 255])).unwrap();
        assert_eq!(image.width, MAX_EDGE);
        assert_eq!(image.height, MAX_EDGE / 2);
        // Aspect ratio survives, which is the bit that breaks silently.
        assert!((image.width as f32 / image.height as f32 - 2.0).abs() < 0.01);
    }

    #[test]
    fn a_small_image_is_left_alone() {
        let image = Image::decode(&png(320, 240, [10, 20, 30, 255])).unwrap();
        assert_eq!((image.width, image.height), (320, 240));
    }

    #[test]
    fn transparency_survives_and_everything_else_becomes_a_jpeg() {
        // Opaque: JPEG, because that is where the size is.
        let opaque = Image::decode(&png(64, 64, [200, 100, 50, 255])).unwrap();
        assert_eq!(opaque.media_type(), "image/jpeg");

        // Actually transparent: PNG, because flattening it changes the
        // question being asked.
        let clear = Image::decode(&png(64, 64, [200, 100, 50, 128])).unwrap();
        assert_eq!(clear.media_type(), "image/png");
        assert!(clear.data_uri().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn a_file_that_is_not_an_image_says_so_rather_than_panicking() {
        let e = Image::decode(b"this is not a png").unwrap_err();
        assert!(e.contains("not an image"), "{e}");
    }

    #[test]
    fn a_thumbnail_is_made_once_and_fits_in_the_bar() {
        let image = Image::decode(&png(800, 400, [1, 2, 3, 255])).unwrap();
        assert!(image.thumb.width <= THUMB_EDGE && image.thumb.height <= THUMB_EDGE);
        // RGBA8, so four bytes a pixel and no surprises for the UI.
        assert_eq!(
            image.thumb.rgba.len(),
            (image.thumb.width * image.thumb.height * 4) as usize
        );
    }

    #[test]
    fn the_temp_file_is_the_processed_image_not_the_original() {
        let image = Image::decode(&png(3000, 3000, [9, 9, 9, 255])).unwrap();
        let path = image.write_temp().unwrap();
        let written = std::fs::read(&path).unwrap();
        assert_eq!(written, image.bytes());
        // Same image, same path — a repeated ask does not litter the temp dir.
        assert_eq!(image.write_temp().unwrap(), path);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn nothing_prints_the_bytes() {
        // A `{:?}` on a request must not produce megabytes of base64.
        let image = Image::decode(&png(64, 64, [1, 2, 3, 255])).unwrap();
        let debugged = format!("{image:?}");
        assert!(debugged.len() < 100, "{debugged}");
        assert!(debugged.contains("64×64"), "{debugged}");
    }
}
