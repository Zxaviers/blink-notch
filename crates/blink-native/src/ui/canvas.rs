//! A canvas on a CPU pixmap. Everything is addressed in logical pixels and
//! scaled to the display here, so the layout code never thinks about DPI.

use tiny_skia::{Color, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, PathBuilder, Pattern, Pixmap, Point, RadialGradient, SpreadMode, Stroke, Transform};

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
    Speaker,
    SpeakerMuted,
    Sun,
    Wifi,
    Bluetooth,
    Mic,
    MicMuted,
    Cast,
    Monitor,
    Lock,
    Crop,
    ChevronRight,
    ChevronLeft,
    Bolt,
    Headphones,
    Plus,
    Close,
    Folder,
    File,
    Download,
    ArrowUp,
    Battery,
    Music,
    Cloud,
    CloudSun,
    Rain,
    Snow,
    Fog,
    Moon,
    Storm,
    Calendar,
    Play,
    Pause,
    SkipBack,
    SkipForward,
    Message,
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

    /// How many rows from the top hold any non-transparent pixel (0 when the canvas is empty).
    /// Exact, so the window layer never has to guess how far a frame reached.
    pub fn content_rows(&self) -> usize {
        let row_bytes = self.pm.width() as usize * 4;
        let data = self.pm.data();
        (0..self.pm.height() as usize)
            .rev()
            .find(|y| data[y * row_bytes..(y + 1) * row_bytes].chunks_exact(4).any(|px| px[3] != 0))
            .map_or(0, |y| y + 1)
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

    // ---------------------------------------------------- gradients and glow

    fn rrect_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<tiny_skia::Path> {
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let r = radius.min(w / 2.0).min(h / 2.0);
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
        pb.finish()
    }

    fn color_of(c: Rgb, alpha: f32) -> Color {
        Color::from_rgba8(c.0, c.1, c.2, (255.0 * alpha.clamp(0.0, 1.0)).round() as u8)
    }

    /// A rounded rectangle filled with a gradient from `a` (left or top) to `b` (right or bottom).
    #[allow(clippy::too_many_arguments)]
    pub fn gradient_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, a: Rgb, b: Rgb, vertical: bool, alpha: f32) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        let Some(path) = Self::rrect_path(x, y, w, h, radius * s) else { return };
        let (p0, p1) = if vertical { (Point::from_xy(x, y), Point::from_xy(x, y + h)) } else { (Point::from_xy(x, y), Point::from_xy(x + w, y)) };
        let stops = vec![GradientStop::new(0.0, Self::color_of(a, alpha)), GradientStop::new(1.0, Self::color_of(b, alpha))];
        let Some(shader) = LinearGradient::new(p0, p1, stops, SpreadMode::Pad, Transform::identity()) else { return };
        let mut paint = Paint::default();
        paint.shader = shader;
        paint.anti_alias = true;
        self.pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), self.mask.as_ref());
    }

    /// A rounded rectangle whose colour and opacity change from one end to the other.
    #[allow(clippy::too_many_arguments)]
    pub fn gradient_rrect_alpha(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, a: (Rgb, f32), b: (Rgb, f32), vertical: bool) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        let Some(path) = Self::rrect_path(x, y, w, h, radius * s) else { return };
        let (p0, p1) = if vertical { (Point::from_xy(x, y), Point::from_xy(x, y + h)) } else { (Point::from_xy(x, y), Point::from_xy(x + w, y)) };
        let stops = vec![GradientStop::new(0.0, Self::color_of(a.0, a.1)), GradientStop::new(1.0, Self::color_of(b.0, b.1))];
        let Some(shader) = LinearGradient::new(p0, p1, stops, SpreadMode::Pad, Transform::identity()) else { return };
        let mut paint = Paint::default();
        paint.shader = shader;
        paint.anti_alias = true;
        self.pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), self.mask.as_ref());
    }

    /// A picture (album art) scaled into a rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn image_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, image: &Pixmap, alpha: f32) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        if image.width() == 0 || image.height() == 0 {
            return;
        }
        let Some(path) = Self::rrect_path(x, y, w, h, radius * s) else { return };
        let (sx, sy) = (w / image.width() as f32, h / image.height() as f32);
        let shader = Pattern::new(image.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, alpha.clamp(0.0, 1.0), Transform::from_row(sx, 0.0, 0.0, sy, x, y));
        let mut paint = Paint::default();
        paint.shader = shader;
        paint.anti_alias = true;
        self.pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), self.mask.as_ref());
    }

    /// A circle that fades from `color` at the centre to nothing at the edge: a soft glow.
    pub fn glow(&mut self, cx: f32, cy: f32, radius: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let (cx, cy, r) = (cx * s, cy * s, radius * s);
        if r <= 0.5 {
            return;
        }
        let stops = vec![GradientStop::new(0.0, Self::color_of(color, alpha)), GradientStop::new(1.0, Self::color_of(color, 0.0))];
        let Some(shader) = RadialGradient::new(Point::from_xy(cx, cy), Point::from_xy(cx, cy), r, stops, SpreadMode::Pad, Transform::identity()) else { return };
        let mut paint = Paint::default();
        paint.shader = shader;
        paint.anti_alias = true;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r);
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), self.mask.as_ref());
        }
    }

    /// A glow spread along a rounded rectangle: soft light around a pill or a bar.
    #[allow(clippy::too_many_arguments)]
    pub fn glow_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, spread: f32, color: Rgb, alpha: f32) {
        const LAYERS: usize = 6;
        for i in 0..LAYERS {
            let t = (i + 1) as f32 / LAYERS as f32;
            let grow = spread * t;
            self.rrect(x - grow, y - grow, w + 2.0 * grow, h + 2.0 * grow, radius + grow, color, alpha * (1.0 - t) * 0.35);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx * s, cy * s, (radius * s).max(0.1));
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &Self::paint(color, alpha), FillRule::Winding, Transform::identity(), self.mask.as_ref());
        }
    }

    /// A round button face, lit from the upper left: `light` fading to `base`.
    pub fn orb(&mut self, cx: f32, cy: f32, radius: f32, light: Rgb, base: Rgb, alpha: f32) {
        let s = self.scale;
        let (cx, cy, r) = (cx * s, cy * s, radius * s);
        let focus = Point::from_xy(cx - r * 0.35, cy - r * 0.45);
        let stops = vec![GradientStop::new(0.0, Self::color_of(light, alpha)), GradientStop::new(1.0, Self::color_of(base, alpha))];
        let Some(shader) = RadialGradient::new(focus, Point::from_xy(cx, cy), r * 1.25, stops, SpreadMode::Pad, Transform::identity()) else { return };
        let mut paint = Paint::default();
        paint.shader = shader;
        paint.anti_alias = true;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r);
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), self.mask.as_ref());
        }
    }

    /// An arc stroke (a progress ring): from `start_deg` (0 = right, clockwise, -90 = top) sweeping `sweep_deg`.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, thickness: f32, start_deg: f32, sweep_deg: f32, color: Rgb, alpha: f32) {
        let s = self.scale;
        let sweep = sweep_deg.clamp(-360.0, 360.0);
        if sweep.abs() < 0.5 {
            return;
        }
        let steps = ((sweep.abs() / 6.0).ceil() as usize).max(2);
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let a = (start_deg + sweep * i as f32 / steps as f32).to_radians();
            let (x, y) = ((cx + radius * a.cos()) * s, (cy + radius * a.sin()) * s);
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: thickness * s, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
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
        // A cloud: four round bumps over a flat bottom, shifted by (ox, oy).
        let cloud = |pb: &mut PathBuilder, ox: f32, oy: f32| {
            let (lx, ly) = (-4.6 + ox, 3.6 + oy);
            let (rx, ry) = (4.6 + ox, 3.6 + oy);
            let (a, b) = (p(lx, ly), p(rx, ry));
            pb.move_to(a.0, a.1);
            pb.line_to(b.0, b.1);
            arc(pb, (4.6 + ox, 1.4 + oy), 2.2, -90.0, 90.0);
            arc(pb, (1.6 + ox, -0.6 + oy), 3.4, -10.0, -200.0);
            arc(pb, (-2.2 + ox, 0.6 + oy), 2.2, -60.0, -240.0);
            arc(pb, (-4.6 + ox, 1.4 + oy), 2.2, 90.0, 270.0);
        };
        let polygon = |pb: &mut PathBuilder, pts: &[(f32, f32)]| {
            for (i, pt) in pts.iter().enumerate() {
                let (x, y) = p(pt.0, pt.1);
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
            pb.close();
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
            Icon::Speaker | Icon::SpeakerMuted => {
                // The cone, then waves (or a cross when muted).
                line(&mut pb, (-5.5, -1.6), (-2.8, -1.6));
                line(&mut pb, (-2.8, -1.6), (0.4, -4.2));
                line(&mut pb, (0.4, -4.2), (0.4, 4.2));
                line(&mut pb, (0.4, 4.2), (-2.8, 1.6));
                line(&mut pb, (-2.8, 1.6), (-5.5, 1.6));
                line(&mut pb, (-5.5, 1.6), (-5.5, -1.6));
                if icon == Icon::Speaker {
                    arc(&mut pb, (0.4, 0.0), 3.0, -45.0, 45.0);
                    arc(&mut pb, (0.4, 0.0), 5.6, -45.0, 45.0);
                } else {
                    line(&mut pb, (2.6, -2.4), (6.0, 2.4));
                    line(&mut pb, (6.0, -2.4), (2.6, 2.4));
                }
            }
            Icon::Sun => {
                arc(&mut pb, (0.0, 0.0), 2.6, 0.0, 360.0);
                for i in 0..8 {
                    let a = (i as f32 * 45.0).to_radians();
                    line(&mut pb, (4.2 * a.cos(), 4.2 * a.sin()), (6.0 * a.cos(), 6.0 * a.sin()));
                }
            }
            Icon::Wifi => {
                for r in [3.0, 5.6, 8.2] {
                    arc(&mut pb, (0.0, 4.4), r, -135.0, -45.0);
                }
                arc(&mut pb, (0.0, 4.4), 0.35, 0.0, 360.0);
            }
            Icon::Bluetooth => {
                line(&mut pb, (-3.2, -3.0), (3.2, 3.0));
                line(&mut pb, (3.2, 3.0), (0.0, 6.0));
                line(&mut pb, (0.0, 6.0), (0.0, -6.0));
                line(&mut pb, (0.0, -6.0), (3.2, -3.0));
                line(&mut pb, (3.2, -3.0), (-3.2, 3.0));
            }
            Icon::Mic | Icon::MicMuted => {
                line(&mut pb, (-2.2, -4.2), (-2.2, 0.6));
                line(&mut pb, (2.2, -4.2), (2.2, 0.6));
                arc(&mut pb, (0.0, -4.2), 2.2, 180.0, 360.0);
                arc(&mut pb, (0.0, 0.6), 2.2, 0.0, 180.0);
                arc(&mut pb, (0.0, 0.6), 4.6, 0.0, 180.0);
                line(&mut pb, (0.0, 5.2), (0.0, 6.6));
                if icon == Icon::MicMuted {
                    line(&mut pb, (-5.2, -6.0), (5.2, 6.2));
                }
            }
            Icon::Cast => {
                line(&mut pb, (-6.0, 4.0), (-6.0, -4.0));
                line(&mut pb, (-6.0, -4.0), (6.0, -4.0));
                line(&mut pb, (6.0, -4.0), (6.0, 4.0));
                line(&mut pb, (6.0, 4.0), (2.4, 4.0));
                for r in [2.4, 4.8] {
                    arc(&mut pb, (-6.0, 4.0), r, -90.0, 0.0);
                }
                arc(&mut pb, (-6.0, 4.0), 0.35, 0.0, 360.0);
            }
            Icon::Monitor => {
                line(&mut pb, (-6.0, -5.0), (6.0, -5.0));
                line(&mut pb, (6.0, -5.0), (6.0, 2.0));
                line(&mut pb, (6.0, 2.0), (-6.0, 2.0));
                line(&mut pb, (-6.0, 2.0), (-6.0, -5.0));
                line(&mut pb, (0.0, 2.0), (0.0, 5.4));
                line(&mut pb, (-3.0, 5.4), (3.0, 5.4));
            }
            Icon::Lock => {
                line(&mut pb, (-4.4, -0.4), (4.4, -0.4));
                line(&mut pb, (4.4, -0.4), (4.4, 5.6));
                line(&mut pb, (4.4, 5.6), (-4.4, 5.6));
                line(&mut pb, (-4.4, 5.6), (-4.4, -0.4));
                line(&mut pb, (-2.8, -0.4), (-2.8, -2.4));
                line(&mut pb, (2.8, -0.4), (2.8, -2.4));
                arc(&mut pb, (0.0, -2.4), 2.8, 180.0, 360.0);
            }
            Icon::Crop => {
                line(&mut pb, (-3.0, -6.0), (-3.0, 3.0));
                line(&mut pb, (-3.0, 3.0), (6.0, 3.0));
                line(&mut pb, (3.0, 6.0), (3.0, -3.0));
                line(&mut pb, (3.0, -3.0), (-6.0, -3.0));
            }
            Icon::ChevronRight => {
                line(&mut pb, (-1.8, -4.0), (2.2, 0.0));
                line(&mut pb, (2.2, 0.0), (-1.8, 4.0));
            }
            Icon::ChevronLeft => {
                line(&mut pb, (1.8, -4.0), (-2.2, 0.0));
                line(&mut pb, (-2.2, 0.0), (1.8, 4.0));
            }
            Icon::Bolt => {
                line(&mut pb, (1.2, -6.0), (-3.4, 1.0));
                line(&mut pb, (-3.4, 1.0), (0.2, 1.0));
                line(&mut pb, (0.2, 1.0), (-1.2, 6.0));
                line(&mut pb, (-1.2, 6.0), (3.4, -1.2));
                line(&mut pb, (3.4, -1.2), (-0.2, -1.2));
                line(&mut pb, (-0.2, -1.2), (1.2, -6.0));
            }
            Icon::Headphones => {
                arc(&mut pb, (0.0, 0.8), 5.2, 180.0, 360.0);
                line(&mut pb, (-5.2, 0.8), (-5.2, 3.4));
                line(&mut pb, (5.2, 0.8), (5.2, 3.4));
                line(&mut pb, (-5.2, 2.0), (-3.0, 2.0));
                line(&mut pb, (-3.0, 2.0), (-3.0, 5.6));
                line(&mut pb, (-3.0, 5.6), (-5.2, 5.6));
                line(&mut pb, (5.2, 2.0), (3.0, 2.0));
                line(&mut pb, (3.0, 2.0), (3.0, 5.6));
                line(&mut pb, (3.0, 5.6), (5.2, 5.6));
            }
            Icon::Plus => {
                line(&mut pb, (-5.0, 0.0), (5.0, 0.0));
                line(&mut pb, (0.0, -5.0), (0.0, 5.0));
            }
            Icon::Close => {
                line(&mut pb, (-4.0, -4.0), (4.0, 4.0));
                line(&mut pb, (4.0, -4.0), (-4.0, 4.0));
            }
            Icon::Folder => {
                line(&mut pb, (-6.0, -4.0), (-2.0, -4.0));
                line(&mut pb, (-2.0, -4.0), (-0.6, -2.4));
                line(&mut pb, (-0.6, -2.4), (6.0, -2.4));
                line(&mut pb, (6.0, -2.4), (6.0, 4.6));
                line(&mut pb, (6.0, 4.6), (-6.0, 4.6));
                line(&mut pb, (-6.0, 4.6), (-6.0, -4.0));
            }
            Icon::File => {
                line(&mut pb, (-4.0, -6.0), (1.2, -6.0));
                line(&mut pb, (1.2, -6.0), (4.0, -3.2));
                line(&mut pb, (4.0, -3.2), (4.0, 6.0));
                line(&mut pb, (4.0, 6.0), (-4.0, 6.0));
                line(&mut pb, (-4.0, 6.0), (-4.0, -6.0));
                line(&mut pb, (1.2, -6.0), (1.2, -3.2));
                line(&mut pb, (1.2, -3.2), (4.0, -3.2));
            }
            Icon::Download => {
                line(&mut pb, (0.0, -5.6), (0.0, 3.0));
                line(&mut pb, (-3.4, -0.4), (0.0, 3.0));
                line(&mut pb, (3.4, -0.4), (0.0, 3.0));
                line(&mut pb, (-5.4, 5.8), (5.4, 5.8));
            }
            Icon::ArrowUp => {
                line(&mut pb, (0.0, 5.6), (0.0, -5.6));
                line(&mut pb, (-4.2, -1.4), (0.0, -5.6));
                line(&mut pb, (4.2, -1.4), (0.0, -5.6));
            }
            Icon::Battery => {
                line(&mut pb, (-6.0, -3.2), (4.4, -3.2));
                line(&mut pb, (4.4, -3.2), (4.4, 3.2));
                line(&mut pb, (4.4, 3.2), (-6.0, 3.2));
                line(&mut pb, (-6.0, 3.2), (-6.0, -3.2));
                line(&mut pb, (6.2, -1.2), (6.2, 1.2));
            }
            Icon::Music => {
                line(&mut pb, (-1.0, 4.2), (-1.0, -5.2));
                line(&mut pb, (-1.0, -5.2), (5.0, -6.2));
                line(&mut pb, (5.0, -6.2), (5.0, 2.6));
                arc(&mut pb, (-3.0, 4.2), 2.0, 0.0, 360.0);
                arc(&mut pb, (3.0, 2.6), 2.0, 0.0, 360.0);
            }
            Icon::Cloud | Icon::CloudSun | Icon::Rain | Icon::Snow | Icon::Storm => {
                // The sun peeks out from behind a cloud; the others hang something under it.
                let (ox, oy) = if icon == Icon::CloudSun { (0.0, 1.6) } else { (0.0, -1.8) };
                cloud(&mut pb, ox, oy);
                match icon {
                    Icon::CloudSun => {
                        arc(&mut pb, (-3.2, -3.4), 1.6, 0.0, 360.0);
                        for i in 0..6 {
                            let a = (i as f32 * 60.0 - 30.0).to_radians();
                            line(&mut pb, (-3.2 + 2.4 * a.cos(), -3.4 + 2.4 * a.sin()), (-3.2 + 3.2 * a.cos(), -3.4 + 3.2 * a.sin()));
                        }
                    }
                    Icon::Rain => {
                        for x in [-3.0, 0.0, 3.0] {
                            line(&mut pb, (x + 0.8, 3.0), (x - 0.4, 6.0));
                        }
                    }
                    Icon::Snow => {
                        for x in [-3.0, 0.0, 3.0] {
                            arc(&mut pb, (x, 4.6), 0.35, 0.0, 360.0);
                        }
                    }
                    Icon::Storm => {
                        polygon(&mut pb, &[(1.0, 2.2), (-1.8, 5.2), (0.2, 5.2), (-0.6, 7.2), (2.2, 4.0), (0.2, 4.0)]);
                    }
                    _ => {}
                }
            }
            Icon::Fog => {
                line(&mut pb, (-5.0, -3.0), (5.0, -3.0));
                line(&mut pb, (-3.0, -0.4), (6.0, -0.4));
                line(&mut pb, (-6.0, 2.2), (3.0, 2.2));
                line(&mut pb, (-4.0, 4.8), (4.0, 4.8));
            }
            Icon::Moon => {
                // A crescent: a big arc and a smaller one cutting into it.
                arc(&mut pb, (0.0, 0.0), 5.2, 45.0, 315.0);
                arc(&mut pb, (2.4, -1.4), 3.9, 332.0, 112.0);
            }
            Icon::Calendar => {
                line(&mut pb, (-5.6, -3.4), (5.6, -3.4));
                line(&mut pb, (5.6, -3.4), (5.6, 5.4));
                line(&mut pb, (5.6, 5.4), (-5.6, 5.4));
                line(&mut pb, (-5.6, 5.4), (-5.6, -3.4));
                line(&mut pb, (-5.6, -0.4), (5.6, -0.4));
                line(&mut pb, (-3.0, -5.4), (-3.0, -2.4));
                line(&mut pb, (3.0, -5.4), (3.0, -2.4));
            }
            Icon::Play => {
                polygon(&mut pb, &[(-3.2, -5.2), (5.2, 0.0), (-3.2, 5.2)]);
            }
            Icon::Pause => {
                polygon(&mut pb, &[(-4.4, -5.0), (-1.4, -5.0), (-1.4, 5.0), (-4.4, 5.0)]);
                polygon(&mut pb, &[(1.4, -5.0), (4.4, -5.0), (4.4, 5.0), (1.4, 5.0)]);
            }
            Icon::SkipBack => {
                polygon(&mut pb, &[(-5.6, -5.0), (-3.6, -5.0), (-3.6, 5.0), (-5.6, 5.0)]);
                polygon(&mut pb, &[(5.6, -5.0), (-2.4, 0.0), (5.6, 5.0)]);
            }
            Icon::SkipForward => {
                polygon(&mut pb, &[(5.6, -5.0), (3.6, -5.0), (3.6, 5.0), (5.6, 5.0)]);
                polygon(&mut pb, &[(-5.6, -5.0), (2.4, 0.0), (-5.6, 5.0)]);
            }
            Icon::Message => {
                line(&mut pb, (-6.0, -4.6), (6.0, -4.6));
                line(&mut pb, (6.0, -4.6), (6.0, 3.0));
                line(&mut pb, (6.0, 3.0), (0.8, 3.0));
                line(&mut pb, (0.8, 3.0), (-2.4, 6.0));
                line(&mut pb, (-2.4, 6.0), (-2.4, 3.0));
                line(&mut pb, (-2.4, 3.0), (-6.0, 3.0));
                line(&mut pb, (-6.0, 3.0), (-6.0, -4.6));
                line(&mut pb, (-3.0, -1.0), (3.0, -1.0));
            }
            Icon::Undo => {
                arc(&mut pb, (0.5, 0.5), 4.2, 160.0, 400.0);
                line(&mut pb, (-4.6, -2.3), (-3.7, -4.7));
                line(&mut pb, (-4.6, -2.3), (-2.2, -1.6));
            }
        }
        if let Some(path) = pb.finish() {
            // The transport symbols are solid shapes; the rest are strokes.
            let solid = matches!(icon, Icon::Play | Icon::Pause | Icon::SkipBack | Icon::SkipForward);
            if solid {
                self.pm.fill_path(&path, &Self::paint(color, alpha), FillRule::Winding, Transform::identity(), self.mask.as_ref());
            }
            let stroke = Stroke { width: if solid { 1.2 } else { 1.5 } * u, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
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
    fn content_rows_reports_exactly_how_far_the_drawing_reaches() {
        let mut c = Canvas::new(640, 360, 1.0);
        assert_eq!(c.content_rows(), 0);
        c.fill_notch(Shape { w: 232.0, h: 32.0, r: 16.0 }, Rgb(0, 0, 0), 1.0);
        let small = c.content_rows();
        assert!((32..=33).contains(&small), "a 32 px notch reaches row {small}");
        c.clear();
        c.fill_notch(Shape { w: 420.0, h: 340.0, r: 24.0 }, Rgb(0, 0, 0), 1.0);
        assert!((340..=341).contains(&c.content_rows()));
        c.clear();
        assert_eq!(c.content_rows(), 0);
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
        for icon in [
            Icon::Check, Icon::ArrowUpRight, Icon::Alarm, Icon::Refresh, Icon::Gear, Icon::Undo, Icon::Speaker, Icon::SpeakerMuted, Icon::Sun, Icon::Wifi,
            Icon::Bluetooth, Icon::Mic, Icon::MicMuted, Icon::Cast, Icon::Monitor, Icon::Lock, Icon::Crop, Icon::ChevronRight, Icon::ChevronLeft,
            Icon::Bolt, Icon::Headphones, Icon::Plus, Icon::Close, Icon::Folder, Icon::File, Icon::Download, Icon::ArrowUp, Icon::Battery, Icon::Music,
            Icon::Play, Icon::Pause, Icon::SkipBack, Icon::SkipForward, Icon::Message, Icon::Cloud, Icon::CloudSun, Icon::Rain, Icon::Snow, Icon::Fog,
            Icon::Moon, Icon::Storm, Icon::Calendar,
        ] {
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
