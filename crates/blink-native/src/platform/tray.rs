//! The notification-area icon and its menu. The icon is drawn here, so there is
//! no resource to ship.

use std::ffi::c_void;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, TrackPopupMenu,
    HICON, ICONINFO, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

/// Sent to the window for tray mouse events (`lparam` is the mouse message).
pub const WM_TRAY: u32 = 0x8000 + 3;
const ICON_ID: u32 = 1;

pub const CMD_PAUSE: u32 = 1;
pub const CMD_REFRESH: u32 = 2;
pub const CMD_SETTINGS: u32 = 3;
pub const CMD_QUIT: u32 = 4;

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

fn fill(pm: &mut Pixmap, path: &tiny_skia::Path, rgb: (u8, u8, u8)) {
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color(Color::from_rgba8(rgb.0, rgb.1, rgb.2, 255));
    pm.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// A flat-topped, round-bottomed shape between the given edges.
fn notch_path(left: f32, right: f32, top: f32, bottom: f32, r: f32) -> tiny_skia::Path {
    let mut p = PathBuilder::new();
    p.move_to(left, top);
    p.line_to(right, top);
    p.line_to(right, bottom - r);
    p.quad_to(right, bottom, right - r, bottom);
    p.line_to(left + r, bottom);
    p.quad_to(left, bottom, left, bottom - r);
    p.close();
    p.finish().expect("notch path")
}

/// A 32x32 icon: a pale notch hanging from the top edge with an amber dot.
fn draw_icon() -> Pixmap {
    let mut pm = Pixmap::new(32, 32).expect("icon");
    fill(&mut pm, &notch_path(3.0, 29.0, 4.0, 24.0, 9.0), (0x22, 0x22, 0x22));
    fill(&mut pm, &notch_path(5.0, 27.0, 4.0, 22.0, 7.5), (0xF2, 0xF2, 0xF2));
    let mut dot = PathBuilder::new();
    dot.push_circle(16.0, 13.0, 3.6);
    fill(&mut pm, &dot.finish().expect("dot path"), (0xF5, 0xB5, 0x44));
    pm
}

fn make_icon() -> Option<HICON> {
    let pm = draw_icon();
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color: HBITMAP = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dst = std::slice::from_raw_parts_mut(bits.cast::<u32>(), 32 * 32);
        for (i, px) in pm.data().chunks_exact(4).enumerate() {
            dst[i] = (u32::from(px[3]) << 24) | (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2]);
        }
        let mask = CreateBitmap(32, 32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color }).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn fixed<const N: usize>(text: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (i, c) in text.encode_utf16().take(N - 1).enumerate() {
        out[i] = c;
    }
    out
}

impl Tray {
    pub fn add(hwnd: HWND, tooltip: &str) -> Option<Tray> {
        let icon = make_icon()?;
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            szTip: fixed::<128>(tooltip),
            ..Default::default()
        };
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            Some(Tray { hwnd, icon })
        } else {
            unsafe {
                let _ = DestroyIcon(icon);
            }
            None
        }
    }
}

impl Tray {
    /// Show a system notification from the tray icon. Windows 10 and 11 draw it as a toast,
    /// with no extra DLLs loaded, and report a click as NIN_BALLOONUSERCLICK.
    pub fn balloon(&self, title: &str, body: &str) -> bool {
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            uFlags: NIF_INFO,
            szInfo: fixed::<256>(body),
            szInfoTitle: fixed::<64>(title),
            dwInfoFlags: NIIF_USER,
            ..Default::default()
        };
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.hwnd, uID: ICON_ID, ..Default::default() };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyIcon(self.icon);
        }
    }
}

pub struct MenuLabels {
    pub pause: String,
    pub refresh: String,
    pub settings: String,
    pub quit: String,
}

/// Show the menu at the cursor and return the chosen command (0 for none).
pub fn show_menu(hwnd: HWND, labels: &MenuLabels, paused: bool) -> u32 {
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let pause = wide(&labels.pause);
        let _ = AppendMenuW(menu, if paused { MF_STRING | MF_CHECKED } else { MF_STRING }, CMD_PAUSE as usize, PCWSTR(pause.as_ptr()));
        let refresh = wide(&labels.refresh);
        let _ = AppendMenuW(menu, MF_STRING, CMD_REFRESH as usize, PCWSTR(refresh.as_ptr()));
        let settings = wide(&labels.settings);
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS as usize, PCWSTR(settings.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let quit = wide(&labels.quit);
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, PCWSTR(quit.as_ptr()));
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // The menu only closes when clicking elsewhere if its owner is the foreground window.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as u32
    }
}
