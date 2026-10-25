//! What the notch shows while files are dragged over it: two places to drop them, the shelf at the left and Projects at the right,
//! with the one under the pointer lit. (The host decides which is lit and what happens on the drop; this only draws.)

use blink_core::i18n::Lang;

use crate::app::DropZone;
use crate::modules::{CONTENT_H, PAGE_SIDE};
use crate::ui::canvas::{Canvas, Icon};
use crate::ui::text::{Face, Fonts};
use crate::ui::theme::{Rgb, Theme};

const CYAN: Rgb = Rgb(0x38, 0xd6, 0xe8);
const VIOLET: Rgb = Rgb(0xa7, 0x8b, 0xfa);

pub fn title_parts(lang: Lang) -> (&'static str, &'static str) {
    if lang == Lang::Id { ("Lepas ", "di sini") } else { ("Drop ", "here") }
}

fn words(lang: Lang) -> [(&'static str, &'static str); 2] {
    if lang == Lang::Id {
        [("Rak", "Simpan sebentar, seret keluar nanti"), ("Proyek", "Tambahkan ke proyek ini")]
    } else {
        [("Shelf", "Keep it for a moment, drag it out later"), ("Projects", "Add it to the current project")]
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    alpha: f32,
    lit: DropZone,
    shelf: bool,
    projects: bool,
    lang: Lang,
    accent: Option<Rgb>,
    origin_x: f32,
    origin_y: f32,
    width: f32,
) {
    let x = origin_x + PAGE_SIDE - 14.0;
    let w = width - 2.0 * (PAGE_SIDE - 14.0);
    let both = shelf && projects;
    let gap = 12.0;
    let zone_w = if both { (w - gap) / 2.0 } else { w };
    let h = CONTENT_H - 14.0;
    let y = origin_y + 2.0;
    let text = words(lang);
    let mut at = x;
    for (zone, on, color, icon, (title, hint)) in [
        (DropZone::Shelf, shelf, accent.unwrap_or(CYAN), Icon::Download, text[0]),
        (DropZone::Projects, projects, accent.unwrap_or(VIOLET), Icon::Folder, text[1]),
    ] {
        if !on {
            continue;
        }
        let is_lit = lit == zone;
        canvas.rrect(at, y, zone_w, h, 16.0, color, alpha * if is_lit { 0.2 } else { 0.06 });
        // A dashed outline: the zone is a place to drop.
        dashed(canvas, at, y, zone_w, h, if is_lit { color } else { theme.dim }, alpha * if is_lit { 1.0 } else { 0.55 });
        let cx = at + zone_w / 2.0;
        if is_lit {
            canvas.glow(cx, y + 34.0, 40.0, color, alpha * 0.35);
        }
        canvas.circle(cx, y + 34.0, 19.0, color, alpha * if is_lit { 0.35 } else { 0.16 });
        canvas.icon(icon, cx, y + 34.0, 19.0, if is_lit { theme.fg } else { color }, alpha);
        let base = y + 62.0 + fonts.baseline(Face::SansBold, 14.0, 20.0);
        let tw = canvas.measure(fonts, Face::SansBold, 14.0, title, 0.0);
        canvas.text(fonts, Face::SansBold, 14.0, cx - tw / 2.0, base, title, theme.fg, alpha, 0.0);
        let max = zone_w - 16.0;
        let cut = fonts.ellipsize(Face::Sans, hint, 11.0 * canvas.scale, 0.0, max * canvas.scale);
        let hw = canvas.measure(fonts, Face::Sans, 11.0, &cut, 0.0);
        let base = y + 84.0 + fonts.baseline(Face::Sans, 11.0, 16.0);
        canvas.text(fonts, Face::Sans, 11.0, cx - hw / 2.0, base, &cut, theme.dim, alpha, 0.0);
        at += zone_w + gap;
    }
}

/// A dashed outline made of short bars along the four sides.
fn dashed(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, color: Rgb, alpha: f32) {
    let (dash, gap) = (6.0_f32, 5.0_f32);
    let mut at = x + 14.0;
    while at < x + w - 14.0 {
        let len = dash.min(x + w - 14.0 - at);
        canvas.rrect(at, y, len, 1.6, 0.8, color, alpha);
        canvas.rrect(at, y + h - 1.6, len, 1.6, 0.8, color, alpha);
        at += dash + gap;
    }
    let mut at = y + 14.0;
    while at < y + h - 14.0 {
        let len = dash.min(y + h - 14.0 - at);
        canvas.rrect(x, at, 1.6, len, 0.8, color, alpha);
        canvas.rrect(x + w - 1.6, at, 1.6, len, 0.8, color, alpha);
        at += dash + gap;
    }
}
