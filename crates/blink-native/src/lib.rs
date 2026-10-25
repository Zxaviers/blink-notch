//! Blink native notch.
//!
//! Pure pieces (clock, shell state machine, drawing kit, modules, the app) live
//! here and are unit tested without Windows; the window, tray and other Windows
//! glue is in `platform` and the `blink` binary.

pub mod access;
pub mod app;
pub mod clock;
pub mod events;
pub mod dropzones;
pub mod eyes;
pub mod host;
pub mod live;
pub mod look;
pub mod modules;
#[cfg(windows)]
pub mod platform;
pub mod shell;
pub mod tour;
pub mod ui;
