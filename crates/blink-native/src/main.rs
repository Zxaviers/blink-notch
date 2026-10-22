//! `blink`: the native notch.
//!
//!   blink                 run
//!   blink --pin-open      start with the panel open (screenshots, tests)
//!   blink --seconds N     quit after N seconds
//!   blink --dump FILE     write a PNG of the last frame on exit
//!
//! Environment (for tests): BLINK_CONTROL=<name> opens the control pipe,
//! BLINK_MANUAL_CLOCK=1 freezes time until told to advance, BLINK_DATA_DIR
//! replaces the data folder, BLINK_SCALE forces the display scale, BLINK_TZ the zone.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    use blink_native::platform::window::{run, Options};

    let mut opts = Options {
        control: std::env::var("BLINK_CONTROL").ok().filter(|s| !s.is_empty()),
        manual_clock: std::env::var_os("BLINK_MANUAL_CLOCK").is_some(),
        data_dir: std::env::var_os("BLINK_DATA_DIR").map(Into::into),
        scale: std::env::var("BLINK_SCALE").ok().and_then(|s| s.parse().ok()),
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pin-open" => opts.pin_open = true,
            "--seconds" => opts.seconds = args.next().and_then(|s| s.parse().ok()),
            "--dump" => opts.dump = args.next(),
            _ => {}
        }
    }
    if let Err(e) = run(opts) {
        eprintln!("blink: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("blink runs on Windows only");
}
