//! Rasterising text for the Text TOP.
//!
//! `fontdue` is pure Rust, so this costs nothing at build time and keeps the
//! promise the README makes: `cargo run` and nothing else, on all three
//! platforms.
//!
//! **No font is embedded.** Every font good enough to be a sensible default is
//! either large or encumbered, and shipping one inside the binary makes a
//! licensing decision on behalf of everyone who redistributes a build. Instead
//! the operator takes a Font path, and with none set it looks through the
//! places each platform actually keeps its system faces. If it finds nothing it
//! says so, by name, on the node — the same thing a Movie File In does when
//! ffmpeg is missing, and for the same reason: a missing resource must be a
//! message, not a mystery.

use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

/// How a line sits against the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Centre,
    End,
}

impl Align {
    pub fn from_index(i: usize) -> Align {
        match i {
            1 => Align::Centre,
            2 => Align::End,
            _ => Align::Start,
        }
    }
}

pub struct Layout {
    pub text: String,
    pub size: f32,
    pub line_spacing: f32,
    pub horizontal: Align,
    pub vertical: Align,
    pub width: u32,
    pub height: u32,
    /// Break lines that do not fit the width.
    pub wrap: bool,
}

/// Somewhere a system font might be. First hit wins.
///
/// Deliberately a short list of the faces that are actually always present
/// rather than an attempt at font enumeration: the point is to have *a*
/// readable default so the operator does something the moment it is dropped,
/// not to be a font manager.
#[cfg(target_os = "macos")]
const SYSTEM_FONTS: &[&str] = &[
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/SFNS.ttf",
    "/Library/Fonts/Arial.ttf",
];

#[cfg(target_os = "windows")]
const SYSTEM_FONTS: &[&str] = &[
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\tahoma.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SYSTEM_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

/// The system font, loaded once. `None` if this machine has none of them.
fn default_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        for path in SYSTEM_FONTS {
            if let Ok(font) = load(std::path::Path::new(path)) {
                return Some(font);
            }
        }
        None
    })
    .as_ref()
}

fn load(path: &std::path::Path) -> Result<Font, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // A `.ttc` collection holds several faces; index 0 is the regular one.
    Font::from_bytes(bytes.as_slice(), FontSettings::default())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// What a Text TOP produced: RGBA8 coverage, and why it could not.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A cache of one font per path, so retyping a caption does not re-parse the
/// face every frame.
#[derive(Default)]
pub struct FontCache {
    entries: Vec<(String, Result<Font, String>)>,
}

impl FontCache {
    pub fn get(&mut self, path: &str) -> Result<&Font, String> {
        let path = path.trim();
        if path.is_empty() {
            return default_font().ok_or_else(|| {
                format!(
                    "no system font found — set Font to a .ttf or .otf. Looked in: {}",
                    SYSTEM_FONTS.join(", ")
                )
            });
        }
        if !self.entries.iter().any(|(p, _)| p == path) {
            let loaded = load(std::path::Path::new(path));
            self.entries.push((path.to_string(), loaded));
            // A caption whose font is being retyped would otherwise grow this
            // without bound.
            if self.entries.len() > 8 {
                let _ = self.entries.remove(0);
            }
        }
        self.entries
            .iter()
            .rev()
            .find(|(p, _)| p == path)
            .map(|(_, f)| f.as_ref().map_err(|e| e.clone()))
            .unwrap()
    }
}

/// Break `text` into the lines that will actually be drawn.
fn lines(font: &Font, layout: &Layout) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in layout.text.split('\n') {
        if !layout.wrap {
            out.push(paragraph.to_string());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if advance(font, &candidate, layout.size) <= layout.width as f32 || current.is_empty() {
                current = candidate;
            } else {
                out.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        out.push(current);
    }
    out
}

/// How wide a line is, in pixels.
fn advance(font: &Font, line: &str, size: f32) -> f32 {
    line.chars()
        .map(|ch| font.metrics(ch, size).advance_width)
        .sum()
}

/// Draw `layout` into an RGBA8 buffer.
///
/// White, with the glyph coverage in the alpha channel and premultiplied into
/// the colour. Premultiplied because the operator's Colour parameter is applied
/// in the shader, and un-premultiplied coverage would fringe every edge with
/// black the moment it was composited over anything.
pub fn rasterise(font: &Font, layout: &Layout) -> Raster {
    let (w, h) = (layout.width.max(1), layout.height.max(1));
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];

    let lines = lines(font, layout);
    let line_height = layout.size * layout.line_spacing.max(0.1);
    let block = line_height * lines.len() as f32;

    // The baseline of the first line. Vertical alignment positions the whole
    // block, then each line steps down from there.
    let top = match layout.vertical {
        Align::Start => 0.0,
        Align::Centre => (h as f32 - block) * 0.5,
        Align::End => h as f32 - block,
    };

    for (index, line) in lines.iter().enumerate() {
        let line_width = advance(font, line, layout.size);
        let mut pen_x = match layout.horizontal {
            Align::Start => 0.0,
            Align::Centre => (w as f32 - line_width) * 0.5,
            Align::End => w as f32 - line_width,
        };
        // fontdue's `ymin` is measured up from the baseline, so the baseline
        // sits `size` below the line's top box — near enough to the ascent for
        // a caption, and it keeps the maths one line rather than a font-metrics
        // excursion.
        let baseline = top + line_height * index as f32 + layout.size;

        for ch in line.chars() {
            let (metrics, coverage) = font.rasterize(ch, layout.size);
            let x0 = (pen_x + metrics.xmin as f32).round() as i32;
            let y0 = (baseline - metrics.height as f32 - metrics.ymin as f32).round() as i32;

            for gy in 0..metrics.height {
                let y = y0 + gy as i32;
                if y < 0 || y >= h as i32 {
                    continue;
                }
                for gx in 0..metrics.width {
                    let x = x0 + gx as i32;
                    if x < 0 || x >= w as i32 {
                        continue;
                    }
                    let a = coverage[gy * metrics.width + gx];
                    if a == 0 {
                        continue;
                    }
                    let i = ((y as usize) * w as usize + x as usize) * 4;
                    // Glyphs overlap at their bounding boxes; keeping the
                    // larger coverage avoids a seam where two boxes meet.
                    let existing = pixels[i + 3];
                    let a = a.max(existing);
                    pixels[i] = a;
                    pixels[i + 1] = a;
                    pixels[i + 2] = a;
                    pixels[i + 3] = a;
                }
            }
            pen_x += metrics.advance_width;
        }
    }

    Raster {
        width: w,
        height: h,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any_font() -> Option<&'static Font> {
        default_font()
    }

    #[test]
    fn text_puts_ink_on_the_canvas() {
        let Some(font) = any_font() else {
            eprintln!("skipping: no system font on this machine");
            return;
        };
        let raster = rasterise(
            font,
            &Layout {
                text: "Hi".into(),
                size: 48.0,
                line_spacing: 1.2,
                horizontal: Align::Centre,
                vertical: Align::Centre,
                width: 200,
                height: 100,
                wrap: false,
            },
        );
        let ink = raster.pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(ink > 20, "expected glyphs, got {ink} covered pixels");
        // And nothing in the corner: centred text must not start at 0,0.
        assert_eq!(raster.pixels[3], 0);
    }

    #[test]
    fn empty_text_is_transparent_rather_than_black() {
        // It is the usual thing to composite over, so the default has to be
        // "nothing here", not "a black rectangle".
        let Some(font) = any_font() else { return };
        let raster = rasterise(
            font,
            &Layout {
                text: String::new(),
                size: 32.0,
                line_spacing: 1.2,
                horizontal: Align::Start,
                vertical: Align::Start,
                width: 64,
                height: 32,
                wrap: false,
            },
        );
        assert!(raster.pixels.iter().all(|b| *b == 0));
    }

    #[test]
    fn wrapping_splits_a_long_line_and_not_a_short_one() {
        let Some(font) = any_font() else { return };
        let layout = |wrap: bool, width: u32| Layout {
            text: "the quick brown fox jumps over the lazy dog".into(),
            size: 24.0,
            line_spacing: 1.2,
            horizontal: Align::Start,
            vertical: Align::Start,
            width,
            height: 400,
            wrap,
        };
        assert_eq!(lines(font, &layout(false, 100)).len(), 1);
        assert!(lines(font, &layout(true, 100)).len() > 3);
        assert_eq!(lines(font, &layout(true, 4000)).len(), 1);
    }

    #[test]
    fn a_newline_starts_a_new_line_even_without_wrapping() {
        let Some(font) = any_font() else { return };
        let got = lines(
            font,
            &Layout {
                text: "one\ntwo\nthree".into(),
                size: 24.0,
                line_spacing: 1.2,
                horizontal: Align::Start,
                vertical: Align::Start,
                width: 1000,
                height: 400,
                wrap: false,
            },
        );
        assert_eq!(got, vec!["one", "two", "three"]);
    }
}
