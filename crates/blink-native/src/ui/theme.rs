//! Colors. Same palette as the WebView2 app: a black cutout for the pill, a dark
//! or light surface for the open panel, amber for "soon" and red for "overdue".
//! Urgency is never color alone: the text carries a word or a number too.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub surface: Surface,
    pub bg: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    /// A faint overlay for hover rows, borders and tracks: (color, alpha 0..=255).
    pub line: (Rgb, u8),
    pub soon: Rgb,
    pub late: Rgb,
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            surface: Surface::Dark,
            bg: Rgb(0, 0, 0),
            fg: Rgb(0xf2, 0xf2, 0xf2),
            dim: Rgb(0xa3, 0xa3, 0xa8),
            line: (Rgb(255, 255, 255), 26),
            soon: Rgb(0xe0, 0xb2, 0x5a),
            late: Rgb(0xec, 0x7f, 0x72),
        }
    }

    pub fn light() -> Theme {
        Theme {
            surface: Surface::Light,
            bg: Rgb(0xf5, 0xf4, 0xf1),
            fg: Rgb(0x14, 0x15, 0x1a),
            dim: Rgb(0x5c, 0x5e, 0x66),
            line: (Rgb(0, 0, 0), 26),
            soon: Rgb(0x8a, 0x5a, 0x00),
            late: Rgb(0xa3, 0x2a, 0x1e),
        }
    }

    /// The urgency accent: soon and overdue are colored, "later" stays neutral.
    pub fn urgency(&self, urgency: &str) -> Rgb {
        match urgency {
            "soon" => self.soon,
            "overdue" => self.late,
            _ => self.fg,
        }
    }
}

/// Which surface the panel uses: the setting, plus Windows' app theme for "auto".
pub fn panel_surface(theme_setting: &str, windows_apps_use_light: bool) -> Surface {
    match theme_setting {
        "light" => Surface::Light,
        "auto" if windows_apps_use_light => Surface::Light,
        _ => Surface::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_windows_and_dark_is_the_default() {
        assert_eq!(panel_surface("dark", true), Surface::Dark);
        assert_eq!(panel_surface("light", false), Surface::Light);
        assert_eq!(panel_surface("auto", true), Surface::Light);
        assert_eq!(panel_surface("auto", false), Surface::Dark);
        assert_eq!(panel_surface("weird", true), Surface::Dark);
    }

    #[test]
    fn only_soon_and_overdue_are_colored() {
        let t = Theme::dark();
        assert_eq!(t.urgency("later"), t.fg);
        assert_ne!(t.urgency("soon"), t.fg);
        assert_ne!(t.urgency("overdue"), t.urgency("soon"));
    }
}