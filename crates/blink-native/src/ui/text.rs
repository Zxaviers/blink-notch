//! Fonts and text layout.
//!
//! Font files are memory-mapped (shared, reclaimable pages, not private
//! memory) and a glyph is rasterized once, on first use, into a small coverage
//! bitmap. Memory therefore follows the glyphs actually shown, not the font
//! size (the fontdue crate parsed every glyph up front and cost about 28 MB).

use std::collections::HashMap;
use std::fs::File;
use std::mem::ManuallyDrop;

use windows::core::PCWSTR;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontFace, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FACE_TYPE_TRUETYPE, DWRITE_FONT_SIMULATIONS_NONE, DWRITE_GLYPH_RUN,
    DWRITE_MEASURING_MODE, DWRITE_MEASURING_MODE_GDI_CLASSIC, DWRITE_MEASURING_MODE_NATURAL, DWRITE_RENDERING_MODE, DWRITE_RENDERING_MODE_GDI_CLASSIC,
    DWRITE_RENDERING_MODE_NATURAL, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_TEXTURE_CLEARTYPE_3x1,
};

use memmap2::Mmap;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Sans,
    SansSemibold,
    /// Thin, for big numbers.
    SansLight,
    /// Bold, for titles.
    SansBold,
    Mono,
}

impl Face {
    fn file(self) -> &'static str {
        match self {
            Face::Sans => "segoeui.ttf",
            Face::SansSemibold => "seguisb.ttf",
            Face::SansLight => "segoeuil.ttf",
            Face::SansBold => "segoeuib.ttf",
            Face::Mono => "consola.ttf",
        }
    }
}

/// A glyph rasterized to coverage (0..=255), positioned relative to the pen.
pub struct Glyph {
    pub left: i32,
    /// Pixels from the baseline up to the top row of the bitmap.
    pub top: i32,
    pub width: usize,
    pub height: usize,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

struct Loaded {
    face: ttf_parser::Face<'static>,
}

pub struct Fonts {
    loaded: HashMap<Face, Loaded>,
    cache: HashMap<(Face, char, u32), Glyph>,
    /// Windows' own glyph rasterizer, which hints (snaps outlines to the pixel grid) so small text is
    /// crisp; None when it could not be set up, and the plain outline rasterizer is used.
    dw: Option<DirectWrite>,
}

/// DirectWrite, used only to turn one glyph into a coverage bitmap.
struct DirectWrite {
    factory: IDWriteFactory,
    faces: HashMap<Face, IDWriteFontFace>,
    rendering: DWRITE_RENDERING_MODE,
    measuring: DWRITE_MEASURING_MODE,
}

impl DirectWrite {
    fn new(dir: &std::path::Path, faces: &[Face]) -> Option<DirectWrite> {
        // GDI-classic hinting snaps stems to whole pixels both ways: the crispest at small sizes
        // (compared side by side with the outline rasterizer, natural, and symmetric).
        // BLINK_TEXT can pick another for comparing looks: off, natural, sym or gdi.
        let (rendering, measuring) = match std::env::var("BLINK_TEXT").as_deref() {
            Ok("off") => return None,
            Ok("natural") => (DWRITE_RENDERING_MODE_NATURAL, DWRITE_MEASURING_MODE_NATURAL),
            Ok("sym") => (DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_MEASURING_MODE_NATURAL),
            _ => (DWRITE_RENDERING_MODE_GDI_CLASSIC, DWRITE_MEASURING_MODE_GDI_CLASSIC),
        };
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let mut map = HashMap::new();
            for face in faces {
                let wide: Vec<u16> = dir.join(face.file()).to_string_lossy().encode_utf16().chain(Some(0)).collect();
                let file = factory.CreateFontFileReference(PCWSTR(wide.as_ptr()), None).ok()?;
                let font_face = factory.CreateFontFace(DWRITE_FONT_FACE_TYPE_TRUETYPE, &[Some(file)], 0, DWRITE_FONT_SIMULATIONS_NONE).ok()?;
                map.insert(*face, font_face);
            }
            Some(DirectWrite { factory, faces: map, rendering, measuring })
        }
    }

    /// (left, top, width, height, coverage): left is from the pen, top from the baseline up.
    fn glyph(&self, face: Face, gid: u16, px: f32) -> Option<(i32, i32, usize, usize, Vec<u8>)> {
        let font_face = self.faces.get(&face)?;
        let gids = [gid];
        let mut run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(font_face.clone())),
            fontEmSize: px,
            glyphCount: 1,
            glyphIndices: gids.as_ptr(),
            glyphAdvances: std::ptr::null(),
            glyphOffsets: std::ptr::null(),
            isSideways: false.into(),
            bidiLevel: 0,
        };
        let result = unsafe {
            (|| {
                let analysis = self.factory.CreateGlyphRunAnalysis(&run, 1.0, None, self.rendering, self.measuring, 0.0, 0.0).ok()?;
                let b = analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1).ok()?;
                let (w, h) = ((b.right - b.left).max(0) as usize, (b.bottom - b.top).max(0) as usize);
                if w == 0 || h == 0 {
                    return Some((0, 0, 0, 0, Vec::new()));
                }
                let mut rgb = vec![0u8; w * h * 3];
                analysis.CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &b, &mut rgb).ok()?;
                // The window has one alpha per pixel (no colour fringes): average the three sub-pixels.
                let coverage = rgb.chunks_exact(3).map(|t| ((u16::from(t[0]) + u16::from(t[1]) + u16::from(t[2])) / 3) as u8).collect();
                Some((b.left, -b.top, w, h, coverage))
            })()
        };
        unsafe { ManuallyDrop::drop(&mut run.fontFace) };
        result
    }
}

/// Feeds a glyph outline (font units, y up) into a tiny-skia path (pixels, y down).
struct Outline<'a> {
    pb: &'a mut PathBuilder,
    x: f32,
    y: f32,
    u: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x * self.u, self.y - y * self.u);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x * self.u, self.y - y * self.u);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(self.x + x1 * self.u, self.y - y1 * self.u, self.x + x * self.u, self.y - y * self.u);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + x1 * self.u, self.y - y1 * self.u,
            self.x + x2 * self.u, self.y - y2 * self.u,
            self.x + x * self.u, self.y - y * self.u,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Fonts {
    /// Maps the three system fonts. Fails if one is missing.
    pub fn load() -> Result<Fonts, String> {
        let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from).join("Fonts");
        let mut loaded = HashMap::new();
        for face in [Face::Sans, Face::SansSemibold, Face::Mono] {
            let path = dir.join(face.file());
            let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            // SAFETY: the file is only read. If another process truncated it while mapped
            // we could fault, but system fonts are read-only.
            let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("{}: {e}", path.display()))?;
            // Fonts live as long as the process, so the mapping is leaked to get a 'static borrow.
            let bytes: &'static [u8] = Box::leak(Box::new(map));
            let face_parsed = ttf_parser::Face::parse(bytes, 0).map_err(|e| format!("{}: {e}", path.display()))?;
            loaded.insert(face, Loaded { face: face_parsed });
        }
        // The light and bold weights are nice to have: a computer without them uses the regular
        // and semibold ones in their place.
        for (face, fallback) in [(Face::SansLight, Face::Sans), (Face::SansBold, Face::SansSemibold)] {
            let path = dir.join(face.file());
            let parsed = File::open(&path)
                .ok()
                .and_then(|file| unsafe { Mmap::map(&file) }.ok())
                .and_then(|map| {
                    let bytes: &'static [u8] = Box::leak(Box::new(map));
                    ttf_parser::Face::parse(bytes, 0).ok()
                });
            let face_parsed = parsed.unwrap_or_else(|| loaded[&fallback].face.clone());
            loaded.insert(face, Loaded { face: face_parsed });
        }
        let dw = DirectWrite::new(&dir, &[Face::Sans, Face::SansSemibold, Face::SansLight, Face::SansBold, Face::Mono]);
        Ok(Fonts { loaded, cache: HashMap::new(), dw })
    }

    fn rasterize(&self, face: Face, ch: char, px: f32) -> Glyph {
        let font = &self.loaded[&face].face;
        let u = px / font.units_per_em() as f32;
        // Unknown characters take the replacement look of '?'.
        let gid = font.glyph_index(ch).or_else(|| font.glyph_index('?')).unwrap_or_default();
        let advance = font.glyph_hor_advance(gid).unwrap_or(0) as f32 * u;
        if let Some((left, top, width, height, coverage)) = self.dw.as_ref().and_then(|dw| dw.glyph(face, gid.0, px)) {
            return Glyph { left, top, width, height, advance, coverage };
        }
        let Some(bb) = font.glyph_bounding_box(gid) else {
            return Glyph { left: 0, top: 0, width: 0, height: 0, advance, coverage: Vec::new() };
        };
        let left = (bb.x_min as f32 * u).floor() as i32 - 1;
        let top = (bb.y_max as f32 * u).ceil() as i32 + 1;
        let right = (bb.x_max as f32 * u).ceil() as i32 + 1;
        let bottom = (bb.y_min as f32 * u).floor() as i32 - 1;
        let (w, h) = ((right - left).max(1) as usize, (top - bottom).max(1) as usize);

        let mut pb = PathBuilder::new();
        font.outline_glyph(gid, &mut Outline { pb: &mut pb, x: -(left as f32), y: top as f32, u });
        let mut coverage = vec![0u8; w * h];
        if let (Some(path), Some(mut pm)) = (pb.finish(), Pixmap::new(w as u32, h as u32)) {
            let mut paint = Paint::default();
            paint.set_color_rgba8(255, 255, 255, 255);
            paint.anti_alias = true;
            pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            for (c, px4) in coverage.iter_mut().zip(pm.data().chunks_exact(4)) {
                *c = px4[3];
            }
        }
        Glyph { left, top, width: w, height: h, advance, coverage }
    }

    pub fn glyph(&mut self, face: Face, ch: char, px: f32) -> &Glyph {
        let key = (face, ch, px.round() as u32);
        if !self.cache.contains_key(&key) {
            let g = self.rasterize(face, ch, px);
            self.cache.insert(key, g);
        }
        &self.cache[&key]
    }

    /// Ascent and descent in pixels (both positive) at px.
    pub fn metrics(&self, face: Face, px: f32) -> (f32, f32) {
        let font = &self.loaded[&face].face;
        let u = px / font.units_per_em() as f32;
        (font.ascender() as f32 * u, -(font.descender() as f32) * u)
    }

    /// Baseline offset from the top of a line box of height line_h, text centered in it.
    pub fn baseline(&self, face: Face, size: f32, line_h: f32) -> f32 {
        let (asc, desc) = self.metrics(face, size);
        (line_h - (asc + desc)) / 2.0 + asc
    }

    /// How many glyph bitmaps are cached (memory diagnostics).
    pub fn cached_glyphs(&self) -> usize {
        self.cache.len()
    }

    pub fn advance(&mut self, face: Face, ch: char, px: f32) -> f32 {
        self.glyph(face, ch, px).advance
    }

    /// Width of `text` in pixels at `px`, with `spacing` extra pixels per glyph.
    pub fn measure(&mut self, face: Face, text: &str, px: f32, spacing: f32) -> f32 {
        text.chars().map(|c| self.advance(face, c, px) + spacing).sum()
    }

    /// `text` shortened with an ellipsis so it fits `max_w` (unchanged if it already does).
    pub fn ellipsize(&mut self, face: Face, text: &str, px: f32, spacing: f32, max_w: f32) -> String {
        if self.measure(face, text, px, spacing) <= max_w {
            return text.to_string();
        }
        let ell = '\u{2026}';
        let ell_w = self.advance(face, ell, px) + spacing;
        let mut out = String::new();
        let mut w = 0.0;
        for c in text.chars() {
            let cw = self.advance(face, c, px) + spacing;
            if w + cw + ell_w > max_w {
                break;
            }
            w += cw;
            out.push(c);
        }
        let trimmed = out.trim_end().to_string();
        format!("{trimmed}{ell}")
    }

    /// Word-wrap into at most `max_lines` lines of width `max_w`; the last line
    /// is ellipsized if text remains. A word wider than the line is split.
    pub fn wrap(&mut self, face: Face, text: &str, px: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        let space_w = self.advance(face, ' ', px);
        let mut line_w = 0.0;
        let mut words = text.split_whitespace().peekable();
        let mut overflow = false;

        while let Some(word) = words.next() {
            let ww = self.measure(face, word, px, 0.0);
            let needed = if line.is_empty() { ww } else { line_w + space_w + ww };
            if needed <= max_w {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
                line_w = needed;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                if lines.len() == max_lines {
                    overflow = true;
                    break;
                }
            }
            if ww <= max_w {
                line.push_str(word);
                line_w = ww;
            } else {
                // Split a long word across lines.
                let mut chunk = String::new();
                let mut cw = 0.0;
                for c in word.chars() {
                    let a = self.advance(face, c, px);
                    if cw + a > max_w && !chunk.is_empty() {
                        lines.push(std::mem::take(&mut chunk));
                        cw = 0.0;
                        if lines.len() == max_lines {
                            overflow = true;
                            break;
                        }
                    }
                    chunk.push(c);
                    cw += a;
                }
                if overflow {
                    break;
                }
                line = chunk;
                line_w = cw;
            }
        }
        if !overflow && !line.is_empty() {
            if lines.len() < max_lines {
                lines.push(line);
            } else {
                overflow = true;
            }
        }
        if overflow || words.peek().is_some() {
            if let Some(last) = lines.last_mut() {
                let joined = format!("{last}\u{2026}");
                *last = self.ellipsize(face, &joined, px, 0.0, max_w);
                if !last.ends_with('\u{2026}') {
                    *last = self.ellipsize(face, &format!("{last}\u{2026}\u{2026}"), px, 0.0, max_w);
                }
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::load().expect("system fonts")
    }

    #[test]
    fn measuring_is_proportional_and_monospace_is_even() {
        let mut f = fonts();
        let a = f.measure(Face::Sans, "Quiz 3", 13.0, 0.0);
        let b = f.measure(Face::Sans, "Quiz 3", 26.0, 0.0);
        assert!((b / a - 2.0).abs() < 0.1, "{a} {b}");
        let m1 = f.measure(Face::Mono, "iiii", 13.0, 0.0);
        let m2 = f.measure(Face::Mono, "MMMM", 13.0, 0.0);
        assert!((m1 - m2).abs() < 0.01, "monospace digits and letters share a width");
        assert_eq!(f.measure(Face::Sans, "", 13.0, 0.0), 0.0);
        assert!(f.measure(Face::Sans, "ab", 13.0, 2.0) > f.measure(Face::Sans, "ab", 13.0, 0.0) + 3.9, "letter spacing adds per glyph");
    }

    #[test]
    fn digits_keep_one_width_in_the_mono_face_so_countdowns_do_not_jump() {
        let mut f = fonts();
        let w: Vec<f32> = "0123456789".chars().map(|c| f.advance(Face::Mono, c, 22.0)).collect();
        assert!(w.iter().all(|x| (x - w[0]).abs() < 0.01), "{w:?}");
    }

    #[test]
    fn ellipsize_fits_or_leaves_text_alone() {
        let mut f = fonts();
        let full = "Project proposal draft";
        let w = f.measure(Face::Sans, full, 13.0, 0.0);
        assert_eq!(f.ellipsize(Face::Sans, full, 13.0, 0.0, w + 1.0), full);
        let short = f.ellipsize(Face::Sans, full, 13.0, 0.0, w / 2.0);
        assert!(short.ends_with('\u{2026}') && short.chars().count() < full.chars().count());
        assert!(f.measure(Face::Sans, &short, 13.0, 0.0) <= w / 2.0);
        assert_eq!(f.ellipsize(Face::Sans, full, 13.0, 0.0, 1.0), "\u{2026}");
    }

    #[test]
    fn wrap_breaks_on_words_and_clamps_with_an_ellipsis() {
        let mut f = fonts();
        let text = "Laporan praktikum basis data normalisasi tabel";
        let one = f.wrap(Face::Sans, text, 13.0, 1000.0, 2);
        assert_eq!(one, vec![text.to_string()]);

        let w = f.measure(Face::Sans, text, 13.0, 0.0);
        let two = f.wrap(Face::Sans, text, 13.0, w * 0.6, 2);
        assert_eq!(two.len(), 2);
        assert!(two.iter().all(|l| f.measure(Face::Sans, l, 13.0, 0.0) <= w * 0.6 + 0.5), "{two:?}");
        assert!(!two[1].ends_with('\u{2026}'), "fits in two lines: {two:?}");

        let clamped = f.wrap(Face::Sans, text, 13.0, w * 0.3, 2);
        assert_eq!(clamped.len(), 2);
        assert!(clamped[1].ends_with('\u{2026}'), "{clamped:?}");
        assert!(clamped.iter().all(|l| f.measure(Face::Sans, l, 13.0, 0.0) <= w * 0.3 + 0.5), "{clamped:?}");
    }

    #[test]
    fn a_single_very_long_word_is_split_rather_than_overflowing() {
        let mut f = fonts();
        let lines = f.wrap(Face::Sans, "Supercalifragilisticexpialidocious", 13.0, 60.0, 3);
        assert!(lines.len() >= 2 && lines.len() <= 3);
        assert!(lines.iter().all(|l| f.measure(Face::Sans, l, 13.0, 0.0) <= 60.5), "{lines:?}");
    }

    #[test]
    fn unknown_characters_and_emoji_do_not_panic() {
        let mut f = fonts();
        let w = f.measure(Face::Sans, "A \u{1F600} B \u{4E2D}", 13.0, 0.0);
        assert!(w > 20.0);
        let g = f.glyph(Face::Sans, '\u{1F600}', 13.0);
        assert!(g.advance > 0.0);
    }

    #[test]
    fn glyphs_are_cached_per_size() {
        let mut f = fonts();
        f.glyph(Face::Sans, 'A', 13.0);
        let n = f.cached_glyphs();
        f.glyph(Face::Sans, 'A', 13.0);
        assert_eq!(f.cached_glyphs(), n);
        f.glyph(Face::Sans, 'A', 15.0);
        assert_eq!(f.cached_glyphs(), n + 1);
    }
}