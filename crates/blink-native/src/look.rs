//! How the notch looks beyond light and dark: a palette (the colour of the body and a soft light in its corner), an accent
//! colour for the chrome (titles, the page dots, the ring), and the shape of the resting pill.
//!
//! Chosen in Blink settings, Display, and kept in `host.json` under `look`: `{ "palette": "aurora", "accent": "pink", "pill": "wide" }`.
//! `accent` is a name from `ACCENTS` or a `#rrggbb` colour; leaving it out lets every page keep its own colour.

use serde_json::Value;

use crate::shell::Shape;
use crate::ui::theme::Rgb;

pub struct PaletteDef {
    pub id: &'static str,
    /// The body of the notch (and the panel, when the panel is dark).
    pub body: Rgb,
    /// A soft light in the top-left corner of the open panel; none for the plain black one.
    pub glow: Option<Rgb>,
}

pub const PALETTES: [PaletteDef; 6] = [
    PaletteDef { id: "midnight", body: Rgb(0, 0, 0), glow: None },
    PaletteDef { id: "graphite", body: Rgb(0x16, 0x17, 0x1c), glow: None },
    PaletteDef { id: "aurora", body: Rgb(0x0a, 0x0e, 0x20), glow: Some(Rgb(0x6d, 0x5d, 0xfc)) },
    PaletteDef { id: "sunset", body: Rgb(0x16, 0x0c, 0x11), glow: Some(Rgb(0xff, 0x7a, 0x59)) },
    PaletteDef { id: "forest", body: Rgb(0x07, 0x12, 0x0d), glow: Some(Rgb(0x34, 0xd3, 0x99)) },
    PaletteDef { id: "ocean", body: Rgb(0x05, 0x12, 0x18), glow: Some(Rgb(0x22, 0xd3, 0xee)) },
];

pub const ACCENTS: [(&str, Rgb); 8] = [
    ("blue", Rgb(0x4e, 0xa8, 0xff)),
    ("violet", Rgb(0xa7, 0x8b, 0xfa)),
    ("pink", Rgb(0xf4, 0x72, 0xb6)),
    ("orange", Rgb(0xfb, 0x92, 0x3c)),
    ("yellow", Rgb(0xfb, 0xbf, 0x24)),
    ("green", Rgb(0x34, 0xd3, 0x99)),
    ("teal", Rgb(0x2d, 0xd4, 0xbf)),
    ("red", Rgb(0xf8, 0x71, 0x71)),
];

/// The resting pill. `classic` is the one Blink always had.
pub const PILLS: [(&str, Shape); 4] = [
    ("classic", Shape { w: 232.0, h: 32.0, r: 16.0 }),
    ("compact", Shape { w: 184.0, h: 28.0, r: 14.0 }),
    ("wide", Shape { w: 300.0, h: 34.0, r: 17.0 }),
    ("square", Shape { w: 232.0, h: 32.0, r: 8.0 }),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub palette: usize,
    pub accent: Option<Rgb>,
    /// The accent follows the cover of the song that is playing (`"accent": "cover"`); a fixed accent is used when nothing plays.
    pub accent_cover: bool,
    pub pill: usize,
    /// The bars move with the sound itself (a loopback listen to the speakers, only while a song plays and the bars show).
    pub visualizer: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { palette: 0, accent: None, accent_cover: false, pill: 0, visualizer: true }
    }
}

impl Look {
    pub fn palette(&self) -> &'static PaletteDef {
        &PALETTES[self.palette]
    }

    pub fn pill(&self) -> Shape {
        PILLS[self.pill].1
    }

    pub fn pill_name(&self) -> &'static str {
        PILLS[self.pill].0
    }
}

fn hex(text: &str) -> Option<Rgb> {
    let t = text.strip_prefix('#')?;
    if t.len() != 6 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(t, 16).ok()?;
    Some(Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// Read `look` from `host.json`'s content; anything unknown means the default.
pub fn parse(host: Option<&Value>) -> Look {
    let look = host.and_then(|h| h.get("look"));
    let word = |key: &str| look.and_then(|l| l.get(key)).and_then(Value::as_str).map(str::trim);
    Look {
        palette: word("palette").and_then(|w| PALETTES.iter().position(|p| p.id == w)).unwrap_or(0),
        accent: word("accent").and_then(|w| ACCENTS.iter().find(|(n, _)| *n == w).map(|(_, c)| *c).or_else(|| hex(w))),
        accent_cover: word("accent") == Some("cover"),
        pill: word("pill").and_then(|w| PILLS.iter().position(|(n, _)| *n == w)).unwrap_or(0),
        visualizer: look.and_then(|l| l.get("visualizer")).and_then(Value::as_bool).unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_look_is_read_from_host_json_with_defaults_for_anything_unknown() {
        assert_eq!(parse(None), Look::default());
        let l = parse(Some(&json!({ "look": { "palette": "aurora", "accent": "pink", "pill": "wide" } })));
        assert_eq!((l.palette().id, l.accent, l.pill().w), ("aurora", Some(Rgb(0xf4, 0x72, 0xb6)), 300.0));
        let custom = parse(Some(&json!({ "look": { "accent": "#10ff80" } })));
        assert_eq!(custom.accent, Some(Rgb(0x10, 0xff, 0x80)));
        let bad = parse(Some(&json!({ "look": { "palette": "nope", "accent": "#12", "pill": 5 } })));
        assert_eq!(bad, Look::default());
        let cover = parse(Some(&json!({ "look": { "accent": "cover", "visualizer": false } })));
        assert!(cover.accent_cover && cover.accent.is_none() && !cover.visualizer);
        assert!(Look::default().visualizer && !Look::default().accent_cover);
    }

    #[test]
    fn every_pill_fits_the_window_and_a_page_title_never_sits_under_it() {
        for (name, shape) in PILLS {
            assert!(shape.w <= 320.0 && shape.h >= 24.0 && shape.h <= 40.0 && shape.r <= shape.h / 2.0 + 0.01, "{name}");
        }
        assert_eq!(PILLS[0].1, crate::shell::REST_FULL, "classic is what it always was");
        assert_eq!(PALETTES[0].body, Rgb(0, 0, 0), "the first palette is the plain black one");
    }
}
