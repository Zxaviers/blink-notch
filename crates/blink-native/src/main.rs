//! `blink`: the native notch.
//!
//!   blink                 run
//!   blink --pin-open      start with the panel open (screenshots, tests)
//!   blink --demo          show sample deadlines instead of your feeds (or BLINK_DEMO=1)
//!   blink --seconds N     quit after N seconds
//!   blink --quit          ask a running notch to exit (the installer uses this)
//!   blink --purge-data    delete settings, caches and saved feed links (the uninstaller, on request)
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
        demo: std::env::var_os("BLINK_DEMO").is_some(),
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    // One-shot commands that do not start a notch.
    match std::env::args().nth(1).as_deref() {
        Some("--quit") => {
            let _ = blink_core::ipc::request(&serde_json::json!({ "cmd": "quit" }));
            return;
        }
        Some("--purge-data") => {
            purge_data();
            return;
        }
        _ => {}
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pin-open" => opts.pin_open = true,
            "--demo" => opts.demo = true,
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

/// Everything Blink keeps for a user: the data folder, the settings program's web profile and
/// the feed links in Credential Manager (their ids come from the settings file).
#[cfg(windows)]
fn purge_data() {
    use blink_core::secret::{CredentialManager, Secrets};
    use blink_core::store::{default_dir, Store};

    let Some(dir) = default_dir() else { return };
    if let Ok(Some(settings)) = Store::new(&dir).get_json("settings") {
        for feed in settings.get("feeds").and_then(|f| f.as_array()).into_iter().flatten() {
            if let Some(id) = feed.get("id").and_then(|i| i.as_str()) {
                CredentialManager.delete(id);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    // A scripted run points the data folder somewhere scratch; then the real web profile is not ours to delete.
    if std::env::var_os("BLINK_DATA_DIR").is_none() {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            // The web profile of the settings window: under its current name and under the one it had before 0.3.
            for name in ["dev.zxaviers.blink", "dev.riski.blink"] {
                let _ = std::fs::remove_dir_all(std::path::Path::new(&local).join(name));
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("blink runs on Windows only");
}
