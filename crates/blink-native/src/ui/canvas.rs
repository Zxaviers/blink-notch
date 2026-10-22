//! A canvas on a CPU pixmap. Everything is addressed in logical pixels and
//! scaled to the display here, so the layout code never thinks about DPI.

use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::text::{Face, Fonts};
use super::theme::Rgb;
use crate::shell::{Shape, WINDOW_W};

/// Simple vector icons, drawn as strokes so no icon font is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Check,
    ArrowUpRight,
    Alarm,
    Refresh,
    Gear,
    Undo,
}

type ClipState = (Option<(i32, i32, i32, i32)>, Option<Mask>);

pub struct Canvas {
    pub pm: Pixmap,
    pub scale: f32,
    /// Physical clip rectangle (x0, y0, x1, y1) for text and masked shapes.
    clip: Option<(i32, i32, i32, i32)>,
    /// Soft clip (a rounded shape, or nested rectangles) applied to everything drawn.
    mask: Option<Mask>,
    stack: Vec<ClipState>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, scale: f32) -> Canvas {
        Canvas { pm: Pixmap::new(width.max(1), height.max(1)).expect("pixmap"), scale, clip: None, mask: None, stack: Vec::new() }
    }

    pub fn width(&self) -> u32 {
        self.pm.width()
    }

    pub fn height(&self) -> u32 {
        self.pm.height()
    }

    pub fn clear(&mut self) {
        self.pm.fill(tiny_skia::Color::TRANSPARENT);
    }

    // ------------------------------------------------------------ clipping

    /// Restrict drawing to a logical rectangle (inside any clip already set) until `pop_clip`.
    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let s = self.scale;
        let (x0, y0, x1, y1) = ((x * s).round() as i32, (y * s).round() as i32, ((x + w) * s).round() as i32, ((y + h) * s).round() as i32);
        self.stack.push((self.clip, self.mask.clone()));
        let merged = match self.clip {
            Some((a0, b0, a1, b1)) => (x0.max(a0), y0.max(b0), x1.min(a1), y1.min(b1)),
            None => (x0, y0, x1, y1),
        };
        self.clip = Some(merged);
        if let Some(rect) = tiny_skia::Rect::from_ltrb(x0 as f32, y0 as f32, x1.max(x0) as f32, y1.max(y0) as f32) {
            let path = PathBuilder::from_rect(rect);
            match self.mask.as_mut() {
                Some(m) => m.intersect_path(&path, FillRule::Winding, false, Transform::identity()),
                None => {
                    if let Some(mut m) = Mask::new(self.pm.width(), self.pm.height()) {
                        m.fill_path(&path, FillRule::Winding, false, Transform::identity());
                        self.mask = Some(m);
                    }
                }
            }
        }
    }

    /// Restrict drawing to the notch's own outline (so contents revealed during the
    /// open animation never spill onto the see-through area), until `pop_clip`.
    pub fn push_notch_clip(&mut self, shape: Shape) {
        let s = self.scale;
        let (w, h, r) = (shape.w * s, shape.h * s, shape.r.min(shape.w / 2.0).min(shape.h) * s);
        let x = (WINDOW_W * s - w) / 2.0;
        self.stack.push((self.clip, self.mask.clone()));
        self.clip = Some((x.floor() as i32, 0, (x + w).ceil() as i32, h.ceil() as i32));
        let k = 0.5523 * r;
        let mut pb = PathBuilder::new();
        pb.move_to(x, 0.0);
        pb.line_to(x + w, 0.0);
        pb.line_to(x + w, h - r);
        pb.cubic_to(x + w, h - r + k, x + w - r + k, h, x + w - r, h);
        pb.line_to(x + r, h);
        pb.cubic_to(x + r - k, h, x, h - r + k, x, h - r);
        pb.close();
        if let (Some(path), Some(mut m)) = (pb.finish(), Mask::new(self.pm.width(), self.pm.height())) {
            m.fill_path(&path, FillRule::Winding, true, Transform::identity());
            self.mask = Some(m);
        }
    }

    pub fn pop_clip(&mut self) {
        let (clip, mask) = self.stack.pop().unwrap_or((None, None));
        self.clip = clip;
        self.mask = mask;
    }
    // -------------------------------------------------------------- shapes

    fn paint(color: Rgb, alpha: f32) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(color.0, color.1, color.2, (255.0 * alpha.clamp(0.0, 1.0)).round() as u8);
        p.anti_alias = true;
        p
    }

    /// The notch body: square top corners (it hangs from the screen edge), rounded bottom.
    pub fn fill_notch(&mut self, shape: Shape, color: Rgb, alpha: f32) {
        let s = self.scale;
        let (w, h, r) = (shape.w * s, shape.h * s, shape.r.min(shape.w / 2.0).min(shape.h) * s);
        let x = (WINDOW_W * s - w) / 2.0;
        let k = 0.5523 * r;
        let mut pb = PathBuilder::new();
        pb.move_to(x, 0.0);
        pb.line_to(x + w, 0.0);
        pb.line_to(x + w, h - r);
        pb.cubic_to(x + w, h - r + k, x + w - r + k, h, x + w - r, h);
        pb.line_to(x + r, h);
        pb.cubic_to(x + r - k, h, x, h - r + k, x, h - r);
        pb.close();
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &Self::paint(color, alpha), FillRule::Winding, Transform::identity(), None);
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb, alpha: f32) {
        self.rrect(x, y, w, h, 0.0, color, alpha);
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, (w * s).max(0.0), (h * s).max(0.0));
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = (radius * s).min(w / 2.0).min(h / 2.0);
        let mut pb = PathBuilder::new();
        if r <= 0.01 {
            pb.move_to(x, y);
            pb.line_to(x + w, y);
            pb.line_to(x + w, y + h);
            pb.line_to(x, y + h);
        } else {
            let k = 0.5523 * r;
            pb.move_to(x + r, y);
            pb.line_to(x + w - r, y);
            pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
            pb.line_to(x + w, y + h - r);
            pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
            pb.line_to(x + r, y + h);
            pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
            pb.line_to(x, y + r);
            pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &Self::paint(color, alpha), FillRule::Winding, Transform::identity(), self.mask.as_ref());
        }
    }

    /// Filled bar with round ends, from `x0` to `x1` at height `y`.
    pub fn bar(&mut self, x0: f32, x1: f32, y: f32, thickness: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        pb.move_to(x0 * s, y * s);
        pb.line_to((x1 * s).max(x0 * s + 0.1), y * s);
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: thickness * s, line_cap: LineCap::Round, ..Stroke::default() };
            self.pm.stroke_path(&path, &Self::paint(color, alpha), &stroke, Transform::identity(), self.mask.as_ref());
        }
    }

    // --------------------------------------------------------------- icons

    pub fn icon(&mut self, icon: Icon, cx: f32, cy: f32, size: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let u = size / 14.0 * s; // designed on a 14-unit box
        let (cx, cy) = (cx * s, cy * s);
        let p = |x: f32, y: f32| (cx + x * u, cy + y * u);
        let mut pb = PathBuilder::new();
        let line = |pb: &mut PathBuilder, a: (f32, f32), b: (f32, f32)| {
            let (ax, ay) = p(a.0, a.1);
            let (bx, by) = p(b.0, b.1);
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
        };
        let arc = |pb: &mut PathBuilder, center: (f32, f32), r: f32, from_deg: f32, to_deg: f32| {
            let steps = (((to_deg - from_deg).abs() / 15.0).ceil() as usize).max(2);
            for i in 0..=steps {
                let a = (from_deg + (to_deg - from_deg) * i as f32 / steps as f32).to_radians();
                let (x, y) = p(center.0 + r * a.cos(), center.1 + r * a.sin());
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
        };
        match icon {
            Icon::Check => {
                line(&mut pb, (-4.0, 0.5), (-1.2, 3.3));
                line(&mut pb, (-1.2, 3.3), (4.2, -3.3));
            }
            Icon::ArrowUpRight => {
                line(&mut pb, (-3.5, 3.5), (3.5, -3.5));
                line(&mut pb, (-0.5, -3.5), (3.5, -3.5));
                line(&mut pb, (3.5, -3.5), (3.5, 0.5));
            }
            Icon::Alarm => {
                arc(&mut pb, (0.0, 0.8), 4.4, 0.0, 360.0);
                line(&mut pb, (0.0, 0.8), (0.0, -1.7));
                line(&mut pb, (0.0, 0.8), (1.8, 1.8));
                line(&mut pb, (-4.8, -3.2), (-3.0, -4.6));
                line(&mut pb, (4.8, -3.2), (3.0, -4.6));
            }
            Icon::Refresh => {
                arc(&mut pb, (0.0, 0.0), 4.2, -60.0, 240.0);
                line(&mut pb, (2.2, -4.6), (4.4, -4.8));
                line(&mut pb, (4.4, -4.8), (4.2, -2.6));
            }
            Icon::Gear => {
                arc(&mut pb, (0.0, 0.0), 2.0, 0.0, 360.0);
                arc(&mut pb, (0.0, 0.0), 4.3, 0.0, 360.0);
                for i in 0..8 {
                    let a = (i as f32 * 45.0).to_radians();
                    line(&mut pb, (4.3 * a.cos(), 4.3 * a.sin()), (6.2 * a.cos(), 6.2 * a.sin()));
                }
            }
            Icon::Undo => {
                arc(&mut pb, (0.5, 0.5), 4.2, 160.0, 400.0);
                line(&mut pb, (-4.6, -2.3), (-3.7, -4.7));
                line(&mut pb, (-4.6, -2.3), (-2.2, -1.6));
            }
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: 1.5 * u, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            self.pm.stroke_path(&path, &Self::paint(color, alpha), &stroke, Transform::identity(), self.mask.as_ref());
        }
    }

    // ---------------------------------------------------------------- text

    /// Draw `text` with its baseline at `baseline`, starting at `x` (logical
    /// pixels). Returns the advance in logical pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        fonts: &mut Fonts,
        face: Face,
        size: f32,
        x: f32,
        baseline: f32,
        text: &str,
        color: Rgb,
        alpha: f32,
        spacing: f32,
    ) -> f32 {
        let s = self.scale;
        let px = size * s;
        let (pw, ph) = (self.width() as i32, self.height() as i32);
        let (cx0, cy0, cx1, cy1) = self.clip.unwrap_or((0, 0, pw, ph));
        let (cx0, cy0, cx1, cy1) = (cx0.max(0), cy0.max(0), cx1.min(pw), cy1.min(ph));
        let base = (baseline * s).round() as i32;
        let mut pen = x * s;
        let alpha = alpha.clamp(0.0, 1.0);
        let stride = self.width() as usize;
        let mask = self.mask.as_ref().map(|m| m.data());

        for ch in text.chars() {
            let g = fonts.glyph(face, ch, px);
            let (gx0, gy0) = (pen.round() as i32 + g.left, base - g.top);
            let data = self.pm.data_mut();
            for row in 0..g.height {
                let y = gy0 + row as i32;
                if y < cy0 || y >= cy1 {
                    continue;
                }
                for col in 0..g.width {
                    let xx = gx0 + col as i32;
                    if xx < cx0 || xx >= cx1 {
                        continue;
                    }
                    let cov = g.coverage[row * g.width + col];
                    if cov == 0 {
                        continue;
                    }
                    let mut a = cov as f32 / 255.0 * alpha;
                    let i = (y as usize * stride + xx as usize) * 4;
                    if let Some(m) = &mask {
                        a *= f32::from(m[y as usize * stride + xx as usize]) / 255.0;
                        if a <= 0.0 {
                            continue;
                        }
                    }
                    let inv = 1.0 - a;
                    data[i] = (color.0 as f32 * a + data[i] as f32 * inv) as u8;
                    data[i + 1] = (color.1 as f32 * a + data[i + 1] as f32 * inv) as u8;
                    data[i + 2] = (color.2 as f32 * a + data[i + 2] as f32 * inv) as u8;
                    data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv) as u8;
                }
            }
            pen += g.advance + spacing * s;
        }
        (pen - x * s) / s
    }

    /// Width of `text` in logical pixels.
    pub fn measure(&self, fonts: &mut Fonts, face: Face, size: f32, text: &str, spacing: f32) -> f32 {
        fonts.measure(face, text, size * self.scale, spacing * self.scale) / self.scale
    }

    pub fn save_png(&self, path: &str) -> Result<(), String> {
        self.pm.save_png(path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(c: &Canvas, x: u32, y: u32) -> u8 {
        c.pm.pixel(x, y).map_or(0, |p| p.alpha())
    }

    #[test]
    fn the_notch_hangs_from_the_top_centre_and_leaves_the_rest_transparent() {
        let mut c = Canvas::new(640, 360, 1.0);
        c.fill_notch(Shape { w: 232.0, h: 32.0, r: 16.0 }, Rgb(0, 0, 0), 1.0);
        assert_eq!(alpha_at(&c, 320, 2), 255, "centre, just below the top edge");
        assert_eq!(alpha_at(&c, 204, 0), 255, "top-left corner is square: (640-232)/2 = 204");
        assert_eq!(alpha_at(&c, 100, 10), 0, "outside the notch is see-through");
        assert_eq!(alpha_at(&c, 320, 40), 0, "below the notch is see-through");
        assert!(alpha_at(&c, 205, 31) < 255, "the bottom corners are rounded");
    }

    #[test]
    fn drawing_scales_with_the_display() {
        let mut c = Canvas::new(960, 540, 1.5);
        c.fill_notch(Shape { w: 232.0, h: 32.0, r: 16.0 }, Rgb(0, 0, 0), 1.0);
        assert_eq!(alpha_at(&c, 480, 40), 255, "32 logical px = 48 physical");
        assert_eq!(alpha_at(&c, 480, 55), 0);
    }

    #[test]
    fn clipping_keeps_text_and_shapes_inside_the_rectangle() {
        let mut fonts = Fonts::load().unwrap();
        let mut c = Canvas::new(640, 360, 1.0);
        c.push_clip(100.0, 100.0, 50.0, 30.0);
        c.rect(0.0, 0.0, 640.0, 360.0, Rgb(255, 255, 255), 1.0);
        c.text(&mut fonts, Face::Sans, 40.0, 90.0, 125.0, "MMMMMM", Rgb(255, 0, 0), 1.0, 0.0);
        c.pop_clip();
        assert_eq!(alpha_at(&c, 99, 110), 0);
        assert_eq!(alpha_at(&c, 150, 110), 0);
        assert_eq!(alpha_at(&c, 120, 99), 0);
        assert_eq!(alpha_at(&c, 120, 130), 0);
        assert!(alpha_at(&c, 120, 110) > 0);
    }

    #[test]
    fn text_draws_pixels_and_reports_its_advance() {
        let mut fonts = Fonts::load().unwrap();
        let mut c = Canvas::new(320, 80, 1.0);
        let adv = c.text(&mut fonts, Face::Mono, 22.0, 10.0, 40.0, "3h 13m", Rgb(224, 178, 90), 1.0, 0.0);
        assert!((adv - c.measure(&mut fonts, Face::Mono, 22.0, "3h 13m", 0.0)).abs() < 0.01);
        let lit = c.pm.data().chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(lit > 100, "{lit}");
    }

    #[test]
    fn every_icon_draws_something_inside_its_box() {
        for icon in [Icon::Check, Icon::ArrowUpRight, Icon::Alarm, Icon::Refresh, Icon::Gear, Icon::Undo] {
            let mut c = Canvas::new(40, 40, 2.0);
            c.icon(icon, 10.0, 10.0, 14.0, Rgb(255, 255, 255), 1.0);
            let lit: Vec<usize> = c.pm.data().chunks_exact(4).enumerate().filter(|(_, p)| p[3] > 0).map(|(i, _)| i).collect();
            assert!(lit.len() > 20, "{icon:?} drew {} pixels", lit.len());
            // Centered at (10,10) logical = (20,20) physical, box 14 logical = 28 physical.
            assert!(lit.iter().all(|i| { let (x, y) = (i % 40, i / 40); (5..=35).contains(&x) && (5..=35).contains(&y) }), "{icon:?} strays outside its box");
        }
    }

    #[test]
    fn nested_clips_intersect_and_pop_restores() {
        let mut fonts = Fonts::load().unwrap();
        let mut c = Canvas::new(200, 100, 1.0);
        c.push_clip(10.0, 10.0, 100.0, 60.0);
        c.push_clip(60.0, 0.0, 100.0, 40.0); // overlap: x 60..110, y 10..40
        c.rect(0.0, 0.0, 200.0, 100.0, Rgb(255, 255, 255), 1.0);
        c.text(&mut fonts, Face::Sans, 30.0, 55.0, 35.0, "MM", Rgb(255, 0, 0), 1.0, 0.0);
        c.pop_clip();
        assert_eq!(alpha_at(&c, 70, 20), 255);
        assert_eq!(alpha_at(&c, 55, 20), 0, "outside the inner clip");
        assert_eq!(alpha_at(&c, 70, 50), 0, "outside the inner clip (below)");
        c.rect(0.0, 80.0, 200.0, 10.0, Rgb(0, 255, 0), 1.0);
        assert_eq!(alpha_at(&c, 20, 50), 0);
        c.rect(15.0, 15.0, 10.0, 10.0, Rgb(0, 0, 255), 1.0);
        assert_eq!(alpha_at(&c, 20, 20), 255, "the outer clip is back after the pop");
        c.pop_clip();
        c.rect(150.0, 90.0, 10.0, 5.0, Rgb(9, 9, 9), 1.0);
        assert_eq!(alpha_at(&c, 155, 92), 255, "no clip left");
    }

    #[test]
    fn the_notch_clip_follows_the_rounded_outline() {
        let mut c = Canvas::new(640, 360, 1.0);
        let shape = Shape { w: 232.0, h: 32.0, r: 16.0 };
        c.push_notch_clip(shape);
        c.rect(0.0, 0.0, 640.0, 360.0, Rgb(255, 255, 255), 1.0);
        c.pop_clip();
        assert_eq!(alpha_at(&c, 320, 20), 255);
        assert_eq!(alpha_at(&c, 100, 20), 0, "left of the notch");
        assert_eq!(alpha_at(&c, 320, 40), 0, "below the notch");
        assert!(alpha_at(&c, 206, 31) < 128, "the rounded corner cuts the fill");
    }}
