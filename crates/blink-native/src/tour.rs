//! The first-run tour: four short cards the notch shows in itself the first time it starts.
//!
//! The host (`app.rs`) owns when it starts and ends; this file owns what the cards say and how they look.

use blink_core::i18n::Lang;

use crate::modules::{CONTENT_H, PAGE_SIDE};
use crate::ui::canvas::{Canvas, Icon};
use crate::ui::text::{Face, Fonts};
use crate::ui::theme::{Rgb, Theme};

const ACCENT: Rgb = Rgb(0x4e, 0xa8, 0xff);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TourButton {
    Next,
    Back,
    Skip,
}

pub struct Card {
    pub title: &'static str,
    pub body: &'static str,
    pub icon: Icon,
}

pub const STEPS: usize = 4;

pub fn card(step: usize, lang: Lang) -> Card {
    let id = lang == Lang::Id;
    match step {
        0 => Card {
            title: if id { "Selamat datang di Blink" } else { "Welcome to Blink" },
            body: if id {
                "Aku tinggal di tepi atas layarmu. Arahkan mouse ke atas dan aku terbuka. Ganti halaman dengan panah, atau tombol kiri dan kanan saat keyboard aktif."
            } else {
                "I live at the top edge of your screen. Move the mouse up to me and I open. Turn pages with the arrows, or Left and Right when the keyboard is on."
            },
            icon: Icon::Bolt,
        },
        1 => Card {
            title: if id { "Dua tombol pengaturan" } else { "Two settings buttons" },
            body: if id {
                "Gerigi di halaman Deadline untuk urusan deadline: feed kalender, pengingat, pil. Tiga titik di kanan atas untuk seluruh Blink: tampilan, halaman, cuaca, musik."
            } else {
                "The gear on the Deadlines page is for your deadlines: feeds, reminders, the pill. The three dots at the top right are for all of Blink: display, pages, weather, music."
            },
            icon: Icon::Gear,
        },
        2 => Card {
            title: if id { "Musik, file, dan lainnya" } else { "Music, files and more" },
            body: if id {
                "Lagu yang diputar muncul di sini beserta liriknya; pin menahan notch tetap terbuka. Seret file ke notch: lepas di Rak untuk menyimpannya sebentar lalu seret keluar ke aplikasi lain, atau di Proyek. Baterai, Wi-Fi, dan volume muncul sebagai intip singkat."
            } else {
                "A song that plays shows up with its lyrics; the pin keeps the notch open. Drag a file onto me: drop it on the Shelf to keep it for a moment and drag it out to another app, or on Projects. Battery, Wi-Fi and volume show up as short peeks."
            },
            icon: Icon::Music,
        },
        _ => Card {
            title: if id { "Asisten AI" } else { "AI assistants" },
            body: if id {
                "Pakai Claude Code, Gemini, Cursor atau asisten AI lain? Buka Pengaturan Blink (tiga titik), lalu Agen, dan tekan Hubungkan. Setelah itu aku memberi tahu saat ia bekerja, butuh kamu, atau selesai."
            } else {
                "Using Claude Code, Gemini, Cursor or another AI assistant? Open Blink settings (the three dots), then Agents, and press Connect. After that I tell you when it is working, needs you, or has finished."
            },
            icon: Icon::Message,
        },
    }
}

pub fn title_parts(lang: Lang) -> (&'static str, &'static str) {
    if lang == Lang::Id { ("Sela", "mat datang") } else { ("Wel", "come") }
}

fn button_words(lang: Lang) -> (&'static str, &'static str, &'static str, &'static str) {
    if lang == Lang::Id { ("Lewati", "Kembali", "Lanjut", "Selesai") } else { ("Skip", "Back", "Next", "Done") }
}

/// Draw one card over the page area. Returns where the buttons are, in window coordinates.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    alpha: f32,
    step: usize,
    lang: Lang,
    pointer: Option<(f32, f32)>,
    accent: Option<Rgb>,
    origin_x: f32,
    origin_y: f32,
    width: f32,
) -> Vec<(f32, f32, f32, f32, TourButton)> {
    let mut hits = Vec::new();
    let accent_color = accent.unwrap_or(ACCENT);
    let c = card(step, lang);
    let x = origin_x + PAGE_SIDE;
    let w = width - 2.0 * PAGE_SIDE;
    let over = |rx: f32, ry: f32, rw: f32, rh: f32| pointer.is_some_and(|(px, py)| px >= rx && px < rx + rw && py >= ry && py < ry + rh);
    let scale = canvas.scale;

    // The picture, then the words.
    let (ix, iy) = (x + 20.0, origin_y + 26.0);
    canvas.glow(ix, iy, 34.0, accent_color, alpha * 0.25);
    canvas.circle(ix, iy, 19.0, accent_color, alpha * 0.18);
    canvas.icon(c.icon, ix, iy, 20.0, accent_color, alpha);
    let tx = x + 50.0;
    let tw = w - 50.0;
    let base = origin_y + 4.0 + fonts.baseline(Face::SansBold, 15.0, 22.0);
    canvas.text(fonts, Face::SansBold, 15.0, tx, base, c.title, theme.fg, alpha, 0.0);
    for (i, line) in fonts.wrap(Face::Sans, c.body, 12.0 * scale, tw * scale, 4).iter().enumerate() {
        let base = origin_y + 28.0 + i as f32 * 16.0 + fonts.baseline(Face::Sans, 12.0, 16.0);
        canvas.text(fonts, Face::Sans, 12.0, tx, base, line, theme.dim, alpha, 0.0);
    }

    // The buttons along the bottom, with the step dots between.
    let (skip, back, next, done) = button_words(lang);
    let by = origin_y + CONTENT_H - 28.0;
    let skip_w = canvas.measure(fonts, Face::Sans, 12.0, skip, 0.0) + 20.0;
    let hover = over(x, by, skip_w, 26.0);
    let base = by + fonts.baseline(Face::Sans, 12.0, 26.0);
    canvas.text(fonts, Face::Sans, 12.0, x + 10.0, base, skip, if hover { theme.fg } else { theme.dim }, alpha, 0.0);
    hits.push((x, by, skip_w, 26.0, TourButton::Skip));

    let last = step + 1 >= STEPS;
    let next_label = if last { done } else { next };
    let nw = canvas.measure(fonts, Face::SansSemibold, 12.0, next_label, 0.0) + 28.0;
    let nx = x + w - nw;
    let hover = over(nx, by, nw, 26.0);
    canvas.rrect(nx, by, nw, 26.0, 13.0, accent_color, alpha * if hover { 1.0 } else { 0.94 });
    let base = by + fonts.baseline(Face::SansSemibold, 12.0, 26.0);
    canvas.text(fonts, Face::SansSemibold, 12.0, nx + 14.0, base, next_label, Rgb(0x06, 0x12, 0x20), alpha, 0.0);
    hits.push((nx, by, nw, 26.0, TourButton::Next));

    if step > 0 {
        let bw = canvas.measure(fonts, Face::Sans, 12.0, back, 0.0) + 20.0;
        let bx = nx - 8.0 - bw;
        let hover = over(bx, by, bw, 26.0);
        canvas.rrect(bx, by, bw, 26.0, 13.0, theme.fg, alpha * if hover { 0.22 } else { 0.1 });
        let base = by + fonts.baseline(Face::Sans, 12.0, 26.0);
        canvas.text(fonts, Face::Sans, 12.0, bx + 10.0, base, back, theme.fg, alpha, 0.0);
        hits.push((bx, by, bw, 26.0, TourButton::Back));
    }

    let dots_x = origin_x + width / 2.0 - (STEPS as f32 - 1.0) * 6.0 - 40.0;
    for i in 0..STEPS {
        let lit = i == step;
        canvas.circle(dots_x + i as f32 * 12.0, by + 13.0, if lit { 3.2 } else { 2.4 }, if lit { accent_color } else { theme.fg }, alpha * if lit { 1.0 } else { 0.3 });
    }
    hits
}
