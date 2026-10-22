//! The notch's private pipe: how the settings program talks to the running
//! notch. One JSON object per line each way; the notch answers every request
//! with `{"ok":true,...}` or `{"ok":false,"error":"CODE"}`.
//!
//! Requests the notch understands (see `blink-native::platform::window`):
//! `ping`, `reload_settings`, `reload_local`, `set_hotkey {accelerator}`,
//! `refresh_now`, `diagnostics`.
//!
//! The pipe is owner-only, so no other account can drive the notch.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

use serde_json::Value;

/// `\\.\pipe\blink-ipc-main`, or another tag when `BLINK_IPC` is set (tests run side by side).
pub fn pipe_path() -> String {
    pipe_path_for(&std::env::var("BLINK_IPC").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| "main".to_string()))
}

pub fn pipe_path_for(tag: &str) -> String {
    format!(r"\\.\pipe\blink-ipc-{tag}")
}

#[derive(Debug, Clone, PartialEq)]
pub enum IpcError {
    /// No notch is listening.
    NotRunning,
    /// It answered `ok: false` with this code, or the conversation broke.
    Failed(String),
}

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;

/// Send one request and wait for the answer. A busy pipe (the notch is between two
/// connections) is retried until `timeout`.
pub fn request_at(path: &str, cmd: &Value, timeout: Duration) -> Result<Value, IpcError> {
    let started = Instant::now();
    let pipe = loop {
        match OpenOptions::new().read(true).write(true).open(path) {
            Ok(f) => break f,
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => return Err(IpcError::NotRunning),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(IpcError::Failed(format!("open: {e}"))),
        }
    };
    let mut writer = &pipe;
    let mut line = cmd.to_string();
    line.push('\n');
    writer.write_all(line.as_bytes()).map_err(|e| IpcError::Failed(format!("write: {e}")))?;
    let mut answer = String::new();
    BufReader::new(&pipe).read_line(&mut answer).map_err(|e| IpcError::Failed(format!("read: {e}")))?;
    let value: Value = serde_json::from_str(answer.trim()).map_err(|_| IpcError::Failed("bad answer".to_string()))?;
    if value["ok"].as_bool() == Some(true) {
        Ok(value)
    } else {
        Err(IpcError::Failed(value["error"].as_str().unwrap_or("FAILED").to_string()))
    }
}

pub fn request(cmd: &Value) -> Result<Value, IpcError> {
    request_at(&pipe_path(), cmd, Duration::from_secs(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipe_nobody_listens_on_means_the_notch_is_not_running() {
        let path = pipe_path_for("unit-test-nobody-home");
        assert_eq!(request_at(&path, &serde_json::json!({ "cmd": "ping" }), Duration::from_millis(100)), Err(IpcError::NotRunning));
    }

    #[test]
    fn the_pipe_name_follows_the_tag() {
        assert_eq!(pipe_path_for("g4"), r"\\.\pipe\blink-ipc-g4");
    }
}
