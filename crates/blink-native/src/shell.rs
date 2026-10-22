//! The notch's behavior as a pure state machine: rest, peek and open, hover
//! intent and grace, animation targets. No Windows, no drawing, no real time:
//! every method takes `now` in milliseconds, so tests drive it exactly.
//!
//! Same numbers as the WebView2 app: 150 ms hover intent, 300 ms grace, 4 s
//! peek, 280 ms ease-out, 232x32 pill, 420-wide panel up to 340 high.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Rest,
    Peek,
    Open,
}

/// How the resting pill looks (from the "when resting" setting and the data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestStyle {
    /// Course code and countdown.
    Full,
    /// A small plain pill.
    Mini,
    /// An invisible strip along the top edge that still reacts to the mouse.
    Hot,
}

/// Logical size of the visible notch (before display scaling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub w: f32,
    pub h: f32,
    pub r: f32,
}

pub const REST_FULL: Shape = Shape { w: 232.0, h: 32.0, r: 16.0 };
pub const REST_MINI: Shape = Shape { w: 84.0, h: 10.0, r: 6.0 };
pub const REST_HOT: Shape = Shape { w: 160.0, h: 6.0, r: 3.0 };
pub const PEEK: Shape = Shape { w: 400.0, h: 56.0, r: 18.0 };
pub const OPEN_W: f32 = 420.0;
pub const OPEN_MAX_H: f32 = 340.0;
pub const OPEN_R: f32 = 24.0;
/// The window the shape lives in, in logical pixels.
pub const WINDOW_W: f32 = 640.0;
pub const WINDOW_H: f32 = 360.0;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Ease-out cubic, close to the CSS cubic-bezier(0.2, 0.8, 0.2, 1) of the web app.
pub fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

#[derive(Clone, Copy, Debug)]
pub struct ShellConfig {
    pub hover_delay_ms: u64,
    pub grace_ms: u64,
    pub peek_ms: u64,
    pub anim_ms: u64,
    /// Windows "show animations" is off: shapes jump instead of easing.
    pub reduced_motion: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig { hover_delay_ms: 150, grace_ms: 300, peek_ms: 4000, anim_ms: 280, reduced_motion: false }
    }
}

#[derive(Clone, Copy, Debug)]
struct Anim {
    from: Shape,
    to: Shape,
    start_ms: u64,
    duration_ms: u64,
}

#[derive(Debug)]
pub struct Shell {
    cfg: ShellConfig,
    mode: Mode,
    rest_style: RestStyle,
    open_height: f32,
    settled: Shape,
    anim: Option<Anim>,
    hovering: bool,
    intent_at: Option<u64>,
    grace_at: Option<u64>,
    peek_until: Option<u64>,
    keyboard: bool,
    pinned: bool,
    mode_changed_at: u64,
}

impl Shell {
    pub fn new(cfg: ShellConfig) -> Self {
        Shell {
            cfg,
            mode: Mode::Rest,
            rest_style: RestStyle::Full,
            open_height: 120.0,
            settled: REST_FULL,
            anim: None,
            hovering: false,
            intent_at: None,
            grace_at: None,
            peek_until: None,
            keyboard: false,
            pinned: false,
            mode_changed_at: 0,
        }
    }

    // ------------------------------------------------------------- queries

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn rest_style(&self) -> RestStyle {
        self.rest_style
    }

    pub fn keyboard(&self) -> bool {
        self.keyboard
    }

    pub fn hovering(&self) -> bool {
        self.hovering
    }

    pub fn config(&self) -> &ShellConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: ShellConfig) {
        self.cfg = cfg;
    }

    fn rest_shape(&self) -> Shape {
        match self.rest_style {
            RestStyle::Full => REST_FULL,
            RestStyle::Mini => REST_MINI,
            RestStyle::Hot => REST_HOT,
        }
    }

    /// Where the notch is heading (or resting).
    pub fn target(&self) -> Shape {
        match self.mode {
            Mode::Rest => self.rest_shape(),
            Mode::Peek => PEEK,
            Mode::Open => Shape { w: OPEN_W, h: self.open_height, r: OPEN_R },
        }
    }

    /// The shape to draw at `now`.
    pub fn shape(&self, now: u64) -> Shape {
        match self.anim {
            Some(a) if now < a.start_ms + a.duration_ms => {
                let t = ease((now - a.start_ms) as f32 / a.duration_ms as f32);
                Shape { w: lerp(a.from.w, a.to.w, t), h: lerp(a.from.h, a.to.h, t), r: lerp(a.from.r, a.to.r, t) }
            }
            Some(a) => a.to,
            None => self.settled,
        }
    }

    pub fn is_animating(&self, now: u64) -> bool {
        self.anim.is_some_and(|a| now < a.start_ms + a.duration_ms)
    }

    /// How open the notch is, 0 (resting pill) to 1 (the panel at its own height). Contents fade on it.
    pub fn openness(&self, now: u64) -> f32 {
        let s = self.shape(now);
        ((s.h - REST_FULL.h) / (self.open_height - REST_FULL.h).max(1.0)).clamp(0.0, 1.0)
    }

    /// Fade of contents after a mode change, for reduced motion (shapes jump, text fades).
    pub fn fade_in(&self, now: u64) -> f32 {
        const FADE_MS: u64 = 160;
        (now.saturating_sub(self.mode_changed_at) as f32 / FADE_MS as f32).clamp(0.0, 1.0)
    }

    /// The earliest moment something needs to happen without an input event.
    pub fn next_wake(&self) -> Option<u64> {
        [self.intent_at, self.grace_at, self.peek_until].into_iter().flatten().min()
    }

    // ------------------------------------------------------------- changes

    fn retarget(&mut self, now: u64) {
        let to = self.target();
        let from = self.shape(now);
        if from == to {
            self.anim = None;
            self.settled = to;
            return;
        }
        if self.cfg.reduced_motion || self.cfg.anim_ms == 0 {
            self.anim = None;
            self.settled = to;
            return;
        }
        self.settled = from;
        self.anim = Some(Anim { from, to, start_ms: now, duration_ms: self.cfg.anim_ms });
    }

    fn set_mode(&mut self, mode: Mode, now: u64) {
        if self.mode != mode {
            self.mode = mode;
            self.mode_changed_at = now;
        }
        self.retarget(now);
    }

    pub fn set_rest_style(&mut self, style: RestStyle, now: u64) {
        if self.rest_style != style {
            self.rest_style = style;
            if self.mode == Mode::Rest {
                self.retarget(now);
            }
        }
    }

    /// The panel's content height changed (clamped to the maximum).
    pub fn set_open_height(&mut self, h: f32, now: u64) {
        let h = h.clamp(REST_FULL.h, OPEN_MAX_H);
        if (h - self.open_height).abs() > f32::EPSILON {
            self.open_height = h;
            if self.mode == Mode::Open {
                self.retarget(now);
            }
        }
    }

    /// Start expanded and stay expanded (debugging, tests).
    pub fn pin_open(&mut self, now: u64) {
        self.pinned = true;
        self.open(now);
    }

    pub fn pointer_enter(&mut self, now: u64) {
        self.hovering = true;
        self.grace_at = None;
        if self.mode != Mode::Open && self.intent_at.is_none() {
            self.intent_at = Some(now + self.cfg.hover_delay_ms);
        }
    }

    pub fn pointer_leave(&mut self, now: u64) {
        self.hovering = false;
        self.intent_at = None;
        if self.mode == Mode::Open && self.grace_at.is_none() && !self.keyboard && !self.pinned {
            self.grace_at = Some(now + self.cfg.grace_ms);
        }
    }

    /// A click on the pill opens it straight away.
    pub fn click(&mut self, now: u64) {
        if self.mode != Mode::Open {
            self.open(now);
        }
    }

    pub fn open(&mut self, now: u64) {
        self.intent_at = None;
        self.peek_until = None;
        self.set_mode(Mode::Open, now);
    }

    /// Back to the resting pill and out of keyboard mode.
    pub fn close(&mut self, now: u64) {
        self.keyboard = false;
        self.intent_at = None;
        self.grace_at = None;
        self.peek_until = None;
        self.set_mode(Mode::Rest, now);
    }

    /// A short automatic expansion. Ignored while open.
    pub fn peek(&mut self, now: u64) {
        if self.mode == Mode::Open {
            return;
        }
        self.peek_until = Some(now + self.cfg.peek_ms);
        self.set_mode(Mode::Peek, now);
    }

    /// Hotkey: open and hold keyboard focus. The platform layer takes real focus.
    pub fn enter_keyboard(&mut self, now: u64) {
        self.keyboard = true;
        self.grace_at = None;
        self.open(now);
    }

    pub fn exit_keyboard(&mut self, now: u64) {
        if self.keyboard {
            self.close(now);
        }
    }

    /// Run whatever has become due. Returns true if the mode changed.
    pub fn tick(&mut self, now: u64) -> bool {
        let before = self.mode;
        if self.intent_at.is_some_and(|t| t <= now) {
            self.intent_at = None;
            self.open(now);
        }
        if self.grace_at.is_some_and(|t| t <= now) {
            self.grace_at = None;
            if !self.hovering {
                self.close(now);
            }
        }
        if self.peek_until.is_some_and(|t| t <= now) {
            self.peek_until = None;
            if self.mode == Mode::Peek {
                self.set_mode(Mode::Rest, now);
            }
        }
        if self.anim.is_some_and(|a| now >= a.start_ms + a.duration_ms) {
            self.settled = self.anim.take().map_or(self.settled, |a| a.to);
        }
        self.mode != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Shell {
        Shell::new(ShellConfig::default())
    }

    #[test]
    fn hovering_opens_after_the_intent_delay_not_before() {
        let mut s = shell();
        s.pointer_enter(1000);
        assert_eq!(s.next_wake(), Some(1150));
        assert!(!s.tick(1149));
        assert_eq!(s.mode(), Mode::Rest);
        assert!(s.tick(1150));
        assert_eq!(s.mode(), Mode::Open);
    }

    #[test]
    fn leaving_before_the_delay_cancels_the_open() {
        let mut s = shell();
        s.pointer_enter(0);
        s.pointer_leave(100);
        assert_eq!(s.next_wake(), None);
        s.tick(1000);
        assert_eq!(s.mode(), Mode::Rest);
    }

    #[test]
    fn closing_waits_the_grace_period_and_reentering_cancels_it() {
        let mut s = shell();
        s.open(0);
        s.pointer_enter(10);
        s.pointer_leave(1000);
        assert_eq!(s.next_wake(), Some(1300));
        s.pointer_enter(1200);
        assert_eq!(s.next_wake(), None);
        s.tick(5000);
        assert_eq!(s.mode(), Mode::Open);

        s.pointer_leave(6000);
        assert!(s.tick(6300));
        assert_eq!(s.mode(), Mode::Rest);
    }

    #[test]
    fn a_pointer_that_came_back_at_the_deadline_keeps_it_open() {
        let mut s = shell();
        s.open(0);
        s.pointer_leave(100); // grace due at 400
        s.hovering = true; // came back without an enter event reaching us first
        assert!(!s.tick(400));
        assert_eq!(s.mode(), Mode::Open);
    }

    #[test]
    fn a_peek_expires_after_four_seconds_and_hover_turns_it_into_open() {
        let mut s = shell();
        s.peek(0);
        assert_eq!(s.mode(), Mode::Peek);
        assert_eq!(s.next_wake(), Some(4000));
        s.tick(4000);
        assert_eq!(s.mode(), Mode::Rest);

        s.peek(10_000);
        s.pointer_enter(10_500);
        s.tick(10_650);
        assert_eq!(s.mode(), Mode::Open);
        assert_eq!(s.next_wake(), None, "the peek timer is gone once open");
    }

    #[test]
    fn a_peek_is_ignored_while_open() {
        let mut s = shell();
        s.open(0);
        s.peek(100);
        assert_eq!(s.mode(), Mode::Open);
    }

    #[test]
    fn clicking_the_pill_opens_immediately() {
        let mut s = shell();
        s.pointer_enter(0);
        s.click(10);
        assert_eq!(s.mode(), Mode::Open);
        assert_eq!(s.next_wake(), None);
    }

    #[test]
    fn keyboard_mode_never_closes_on_leave() {
        let mut s = shell();
        s.enter_keyboard(0);
        assert!(s.keyboard() && s.mode() == Mode::Open);
        s.pointer_enter(10);
        s.pointer_leave(20);
        assert_eq!(s.next_wake(), None);
        s.tick(10_000);
        assert_eq!(s.mode(), Mode::Open);
        s.exit_keyboard(10_001);
        assert_eq!((s.mode(), s.keyboard()), (Mode::Rest, false));
    }

    #[test]
    fn a_pinned_notch_stays_open() {
        let mut s = shell();
        s.pin_open(0);
        s.pointer_enter(1);
        s.pointer_leave(2);
        s.tick(10_000);
        assert_eq!(s.mode(), Mode::Open);
    }

    #[test]
    fn the_shape_eases_from_the_pill_to_the_panel_over_280_ms() {
        let mut s = shell();
        s.set_open_height(200.0, 0);
        s.open(1000);
        assert_eq!(s.shape(1000), REST_FULL);
        let mid = s.shape(1140);
        assert!(mid.h > REST_FULL.h && mid.h < 200.0 && mid.w > REST_FULL.w && mid.w < OPEN_W);
        assert!(mid.h > (REST_FULL.h + 200.0) / 2.0, "ease-out is past halfway at the midpoint");
        assert!(s.is_animating(1279) && !s.is_animating(1280));
        assert_eq!(s.shape(1280), Shape { w: OPEN_W, h: 200.0, r: OPEN_R });
        s.tick(1300);
        assert_eq!(s.shape(2000), Shape { w: OPEN_W, h: 200.0, r: OPEN_R });
    }

    #[test]
    fn reversing_mid_animation_starts_from_the_current_shape() {
        let mut s = shell();
        s.set_open_height(300.0, 0);
        s.open(0);
        let at = s.shape(100);
        s.close(100);
        let start = s.shape(100);
        assert_eq!(at, start, "no jump when the direction flips");
        assert!(s.shape(380).h < at.h);
        assert_eq!(s.shape(380), REST_FULL);
    }

    #[test]
    fn reduced_motion_jumps_straight_to_the_target() {
        let mut s = Shell::new(ShellConfig { reduced_motion: true, ..ShellConfig::default() });
        s.set_open_height(200.0, 0);
        s.open(50);
        assert_eq!(s.shape(50), Shape { w: OPEN_W, h: 200.0, r: OPEN_R });
        assert!(!s.is_animating(60));
        assert!(s.fade_in(50) < 0.1 && s.fade_in(250) == 1.0, "contents still fade");
    }

    #[test]
    fn the_open_height_follows_content_and_is_clamped() {
        let mut s = shell();
        s.open(0);
        s.tick(300);
        s.set_open_height(500.0, 400);
        assert_eq!(s.target().h, OPEN_MAX_H);
        s.set_open_height(10.0, 500);
        assert_eq!(s.target().h, REST_FULL.h);
    }

    #[test]
    fn the_rest_style_changes_the_resting_shape_only() {
        let mut s = shell();
        s.set_rest_style(RestStyle::Mini, 0);
        assert_eq!(s.target(), REST_MINI);
        s.open(10);
        s.set_rest_style(RestStyle::Hot, 20);
        assert_eq!(s.target().w, OPEN_W);
        s.close(30);
        assert_eq!(s.target(), REST_HOT);
    }

    #[test]
    fn zero_hover_delay_opens_on_the_next_tick() {
        let mut s = Shell::new(ShellConfig { hover_delay_ms: 0, ..ShellConfig::default() });
        s.pointer_enter(500);
        assert_eq!(s.next_wake(), Some(500));
        assert!(s.tick(500));
    }

    #[test]
    fn a_short_panel_still_reaches_full_openness() {
        let mut s = shell();
        s.set_open_height(100.0, 0); // an empty state, much shorter than the maximum
        s.open(0);
        assert_eq!(s.openness(280), 1.0);
        assert!(s.openness(100) > 0.0 && s.openness(100) < 1.0);
    }

    #[test]
    fn openness_tracks_the_height() {
        let mut s = shell();
        s.set_open_height(OPEN_MAX_H, 0);
        assert_eq!(s.openness(0), 0.0);
        s.open(0);
        assert_eq!(s.openness(280), 1.0);
        let mid = s.openness(100);
        assert!(mid > 0.0 && mid < 1.0);
    }
}