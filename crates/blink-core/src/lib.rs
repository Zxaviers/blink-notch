//! Blink core: the logic of the notch with no UI and no windows.
//!
//! Ported from the JavaScript in `src/lib` (the WebView2 app). Until the JS is
//! retired, `tests/` runs both implementations on the same inputs and requires
//! identical output.

pub mod countdown;
pub mod done;
pub mod feed;
pub mod feed_url;
pub mod feeds;
pub mod format;
pub mod hotkey;
pub mod i18n;
mod i18n_data;
pub mod ics;
pub mod jsval;
pub mod local_events;
pub mod merge;
pub mod planner;
pub mod secret;
pub mod settings;
pub mod store;
pub mod thresholds;
pub mod view;