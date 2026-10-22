//! Time. Production uses the real clock. Tests use a manual clock that only
//! moves when told to, so animations and timers are deterministic.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct Clock {
    start: Instant,
    /// Some(ms) = manual mode: the app's monotonic time is this value.
    manual_ms: Option<u64>,
    /// Wall clock at app time 0 in manual mode (what "now" means for deadlines).
    manual_wall_base_ms: i64,
}

impl Clock {
    pub fn real() -> Self {
        Clock { start: Instant::now(), manual_ms: None, manual_wall_base_ms: 0 }
    }

    /// `wall_base_ms`: the wall-clock instant (Unix ms) at manual time 0.
    pub fn manual(wall_base_ms: i64) -> Self {
        Clock { start: Instant::now(), manual_ms: Some(0), manual_wall_base_ms: wall_base_ms }
    }

    pub fn is_manual(&self) -> bool {
        self.manual_ms.is_some()
    }

    /// Monotonic milliseconds since the app started; drives the shell and animation.
    pub fn now_ms(&self) -> u64 {
        self.manual_ms.unwrap_or_else(|| self.start.elapsed().as_millis() as u64)
    }

    /// Unix milliseconds: what deadlines are compared against.
    pub fn wall_ms(&self) -> i64 {
        match self.manual_ms {
            Some(m) => self.manual_wall_base_ms + m as i64,
            None => SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
        }
    }

    /// Move a manual clock forward. No effect on the real clock.
    pub fn set_manual_ms(&mut self, ms: u64) {
        if let Some(m) = self.manual_ms.as_mut() {
            *m = ms;
        }
    }
}