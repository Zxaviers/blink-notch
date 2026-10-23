//! The eyes the notch shows when it has been quiet for a while, as a pure function of how long
//! it has been quiet. They open (a quick fade and grow), look around now and then, blink at
//! uneven intervals, and finally grow sleepy. Nothing here draws or keeps time; the app asks
//! "what do the eyes look like after this long?" and "when does that next change?".

/// Quiet for this long and the clock gives way to the eyes.
pub const EYES_AFTER_MS: u64 = 45_000;
/// The eyes are fully drowsy from here on, and nothing wakes the notch for them any more.
pub const DOZE_AT_MS: u64 = 150_000;
const APPEAR_MS: u64 = 450;
const FIRST_CYCLE_AT_MS: u64 = APPEAR_MS + 700;
const DOZE_RAMP_MS: u64 = 2_200;
const BLINK_MS: u64 = 240;
/// A blink ends a cycle; it starts this long before the end.
const BLINK_LEAD_MS: u64 = 300;
const GLANCE_MS: u64 = 1_100;
/// How open the eyes are when dozing (1 = wide open).
const DOZE_OPEN: f32 = 0.16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeFrame {
    /// 0 = just starting to show, 1 = fully there.
    pub appear: f32,
    /// How open the eyelids are: 1 wide, near 0 shut.
    pub open: f32,
    /// Which way they look, -1 (left) to 1 (right).
    pub look: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub frame: EyeFrame,
    /// Something is moving right now: the app should keep drawing frames.
    pub animating: bool,
    /// How long after the start of the quiet the picture next changes by itself (None: never again).
    pub next_change_ms: Option<u64>,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Length of the n-th stretch between blinks: uneven, between 3.4 and 6 seconds.
fn cycle_ms(n: u64) -> u64 {
    3_400 + (n.wrapping_mul(2_654_435_761) >> 7) % 2_600
}

/// 0 when open, 1 at the instant they are shut: a fast close and a slower open.
fn blink_depth(since_start_ms: u64) -> f32 {
    const CLOSE: f32 = 80.0;
    const HOLD: f32 = 30.0;
    let t = since_start_ms as f32;
    if t < CLOSE {
        ease_in_out(t / CLOSE)
    } else if t < CLOSE + HOLD {
        1.0
    } else {
        1.0 - ease_out((t - CLOSE - HOLD) / (BLINK_MS as f32 - CLOSE - HOLD))
    }
}

/// How the eyes look after `idle_ms` of quiet; None before they show at all.
pub fn at(idle_ms: u64) -> Option<Step> {
    if idle_ms < EYES_AFTER_MS {
        return None;
    }
    let t = idle_ms - EYES_AFTER_MS;
    let appear = ease_out(t as f32 / APPEAR_MS as f32);
    let doze_start = DOZE_AT_MS - EYES_AFTER_MS - DOZE_RAMP_MS;
    let doze_end = DOZE_AT_MS - EYES_AFTER_MS;

    // Dozing: the lids sink slowly, then stay down.
    if t >= doze_start {
        let k = ease_in_out((t - doze_start) as f32 / DOZE_RAMP_MS as f32);
        let open = 1.0 + (DOZE_OPEN - 1.0) * k;
        let animating = t < doze_end;
        return Some(Step { frame: EyeFrame { appear: 1.0, open, look: 0.0 }, animating, next_change_ms: if animating { Some(idle_ms + 1) } else { None } });
    }

    // Find the stretch between blinks that holds `t`.
    let mut start = FIRST_CYCLE_AT_MS;
    let mut n = 0;
    while start + cycle_ms(n) <= t {
        start += cycle_ms(n);
        n += 1;
    }
    let len = cycle_ms(n);
    let end = start + len;
    let mut open = 1.0;
    let mut look = 0.0;
    let mut animating = appear < 1.0;
    let mut next = if appear < 1.0 { APPEAR_MS } else { end };
    let mut consider = |at: u64| {
        if at > t {
            next = next.min(at);
        }
    };
    consider(FIRST_CYCLE_AT_MS);

    // A glance sideways partway through most stretches, alternating sides.
    let glance_at = start + len * 35 / 100;
    if n % 4 != 3 {
        let side = if n % 2 == 0 { 1.0 } else { -1.0 };
        if t >= glance_at && t < glance_at + GLANCE_MS {
            let into = (t - glance_at) as f32;
            let k = if into < 180.0 {
                ease_in_out(into / 180.0)
            } else if into < GLANCE_MS as f32 - 180.0 {
                1.0
            } else {
                1.0 - ease_in_out((into - (GLANCE_MS as f32 - 180.0)) / 180.0)
            };
            look = side * k;
            animating = true;
            consider(glance_at + GLANCE_MS);
        } else {
            consider(glance_at);
        }
    }

    // The blink that ends the stretch.
    let blink_at = end - BLINK_LEAD_MS;
    if t >= blink_at && t < blink_at + BLINK_MS {
        open = 1.0 - blink_depth(t - blink_at) * 0.92;
        animating = true;
        consider(blink_at + BLINK_MS);
    } else {
        consider(blink_at);
    }
    consider(end);
    consider(doze_start);
    let next_change_ms = if animating { Some(idle_ms + 1) } else { Some(EYES_AFTER_MS + next) };
    Some(Step { frame: EyeFrame { appear, open, look }, animating, next_change_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_before_the_quiet_is_long_enough() {
        assert!(at(0).is_none() && at(EYES_AFTER_MS - 1).is_none());
        assert!(at(EYES_AFTER_MS).is_some());
    }

    #[test]
    fn the_eyes_fade_in_and_then_hold_still_between_blinks() {
        let first = at(EYES_AFTER_MS).unwrap();
        assert!(first.frame.appear < 0.05 && first.animating);
        let settled = at(EYES_AFTER_MS + APPEAR_MS + 100).unwrap();
        assert_eq!(settled.frame.appear, 1.0);
        assert!(!settled.animating && settled.frame.open == 1.0 && settled.frame.look == 0.0);
        // A quiet moment names the next time something moves, in the future.
        assert!(settled.next_change_ms.unwrap() > EYES_AFTER_MS + APPEAR_MS + 100);
    }

    #[test]
    fn a_blink_closes_smoothly_and_opens_again() {
        // Find a blink: sample every 5 ms through the first cycle and watch the lids.
        let mut opens = Vec::new();
        for ms in (0..9_000).step_by(5) {
            if let Some(s) = at(EYES_AFTER_MS + ms) {
                opens.push(s.frame.open);
            }
        }
        let lowest = opens.iter().cloned().fold(1.0_f32, f32::min);
        assert!(lowest < 0.15, "the lids shut: {lowest}");
        // No jumps: neighbouring samples differ by a small step (smooth, not a flicker).
        let biggest_jump = opens.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
        assert!(biggest_jump < 0.2, "{biggest_jump}");
    }

    #[test]
    fn the_eyes_look_aside_now_and_then() {
        let mut seen: Vec<f32> = Vec::new();
        for ms in (0..20_000).step_by(20) {
            if let Some(s) = at(EYES_AFTER_MS + ms) {
                seen.push(s.frame.look);
            }
        }
        assert!(seen.iter().any(|l| *l > 0.9) && seen.iter().any(|l| *l < -0.9), "glances to both sides");
    }

    #[test]
    fn they_grow_sleepy_and_then_everything_goes_quiet() {
        let ramp = at(DOZE_AT_MS - 1_000).unwrap();
        assert!(ramp.animating && ramp.frame.open < 1.0 && ramp.frame.open > DOZE_OPEN);
        let asleep = at(DOZE_AT_MS + 10).unwrap();
        assert!(!asleep.animating && asleep.next_change_ms.is_none());
        assert!((asleep.frame.open - DOZE_OPEN).abs() < 0.01);
    }
}
