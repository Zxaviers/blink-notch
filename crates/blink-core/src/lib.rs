//! Blink core: the logic of the notch with no UI and no windows.
//!
//! Ported from the JavaScript in `src/lib` (the WebView2 app). Until the JS is
//! retired, `tests/` runs both implementations on the same inputs and requires
//! identical output.

pub mod feed_url;
pub mod feeds;
pub mod hotkey;
pub mod ics;
pub mod jsval;
pub mod settings;
pub mod thresholds;