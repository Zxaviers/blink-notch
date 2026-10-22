//! The control pipe: lets a test (or a script) drive the running app without a
//! real mouse or keyboard. Off unless `BLINK_CONTROL=<name>` is set; then it
//! listens on `\\.\pipe\blink-control-<name>`, owner only, one JSON command per
//! line, one JSON reply per line.
//!
//! The pipe thread never touches app state: it posts each command to the window
//! thread (where all state lives) and waits for the answer.

use std::ffi::c_void;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, HWND, INVALID_HANDLE_VALUE, LPARAM, LocalFree, WPARAM};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Posted to the window thread; `lparam` is a `Box<Request>` raw pointer.
pub const WM_CONTROL: u32 = 0x8000 + 1; // WM_APP + 1

pub struct Request {
    pub line: String,
    pub reply: Sender<String>,
}

pub fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\blink-control-{name}")
}

/// Start the listener thread. `hwnd` is the window that handles `WM_CONTROL`.
pub fn start(name: &str, hwnd: HWND) {
    let path: Vec<u16> = pipe_path(name).encode_utf16().chain(Some(0)).collect();
    let raw_hwnd = hwnd.0 as isize;
    std::thread::spawn(move || serve(path, raw_hwnd));
}

fn serve(path: Vec<u16>, raw_hwnd: isize) {
    let hwnd = HWND(raw_hwnd as *mut c_void);
    loop {
        // Owner-only access: D = DACL, P = protected, GA = generic all, OW = owner rights.
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let mut sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, ..Default::default() };
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:P(A;;GA;;;OW)"), SDDL_REVISION_1, &mut sd, None).is_ok() {
                sa.lpSecurityDescriptor = sd.0;
            }
        }
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(path.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                64 * 1024,
                64 * 1024,
                0,
                Some(&sa),
            )
        };
        unsafe {
            if !sd.0.is_null() {
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
        }
        if pipe == INVALID_HANDLE_VALUE {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        // Blocks until a client connects (ERROR_PIPE_CONNECTED means it already did).
        let _ = unsafe { ConnectNamedPipe(pipe, None) };
        talk(pipe, hwnd);
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
    }
}

fn talk(pipe: HANDLE, hwnd: HWND) {
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut read = 0u32;
        let ok = unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut read), None) };
        if ok.is_err() || read == 0 {
            return;
        }
        pending.extend_from_slice(&buf[..read as usize]);
        while let Some(at) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=at).collect();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let (tx, rx) = channel();
            let request = Box::into_raw(Box::new(Request { line, reply: tx }));
            if unsafe { PostMessageW(Some(hwnd), WM_CONTROL, WPARAM(0), LPARAM(request as isize)) }.is_err() {
                drop(unsafe { Box::from_raw(request) });
                return;
            }
            let mut answer = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| r#"{"ok":false,"error":"timeout"}"#.to_string());
            answer.push('\n');
            let mut written = 0u32;
            if unsafe { WriteFile(pipe, Some(answer.as_bytes()), Some(&mut written), None) }.is_err() {
                return;
            }
        }
    }
}