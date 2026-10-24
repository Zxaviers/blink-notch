//! Files and folders dragged onto the notch.
//!
//! The window is a drop target. While something with files is over it, the window is told (so the
//! notch can open its drop zone); when they are let go, their paths are handed over. Nothing is
//! copied or read: only the names of what was dropped.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{HWND, LPARAM, POINTL, WPARAM};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{IDropTarget, IDropTarget_Impl, OleInitialize, ReleaseStgMedium, RegisterDragDrop, CF_HDROP, DROPEFFECT, DROPEFFECT_LINK, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Posted to the window: `wparam` 1 when files come over it, 0 when they leave.
pub const WM_DRAG: u32 = 0x8000 + 20;
/// Posted to the window when files are dropped: `lparam` is a `Box<Vec<String>>` (the window takes it back).
pub const WM_DROPPED: u32 = 0x8000 + 21;

#[implement(IDropTarget)]
struct Dropper {
    hwnd: isize,
}

fn files_format() -> FORMATETC {
    FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

fn has_files(data: &Option<&IDataObject>) -> bool {
    data.is_some_and(|d| unsafe { d.QueryGetData(&files_format()) }.is_ok())
}

impl Dropper {
    fn post(&self, message: u32, wparam: usize, lparam: isize) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut c_void)), message, WPARAM(wparam), LPARAM(lparam));
        }
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for Dropper_Impl {
    fn DragEnter(&self, pdataobj: Ref<'_, IDataObject>, _grfkeystate: MODIFIERKEYS_FLAGS, _pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        let accept = has_files(&pdataobj.as_ref());
        unsafe {
            *pdweffect = if accept { DROPEFFECT_LINK } else { DROPEFFECT_NONE };
        }
        if accept {
            self.post(WM_DRAG, 1, 0);
        }
        Ok(())
    }

    fn DragOver(&self, _grfkeystate: MODIFIERKEYS_FLAGS, _pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            // Still the same drag: whatever DragEnter decided stays.
            if *pdweffect != DROPEFFECT_NONE {
                *pdweffect = DROPEFFECT_LINK;
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.post(WM_DRAG, 0, 0);
        Ok(())
    }

    fn Drop(&self, pdataobj: Ref<'_, IDataObject>, _grfkeystate: MODIFIERKEYS_FLAGS, _pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            *pdweffect = DROPEFFECT_NONE;
        }
        let Some(data) = pdataobj.as_ref() else { return Ok(()) };
        let mut paths: Vec<String> = Vec::new();
        unsafe {
            if let Ok(mut medium) = data.GetData(&files_format()) {
                let drop = HDROP(medium.u.hGlobal.0);
                let count = DragQueryFileW(drop, u32::MAX, None);
                for i in 0..count {
                    let len = DragQueryFileW(drop, i, None) as usize;
                    let mut buffer = vec![0u16; len + 1];
                    let got = DragQueryFileW(drop, i, Some(&mut buffer)) as usize;
                    paths.push(String::from_utf16_lossy(&buffer[..got]));
                }
                ReleaseStgMedium(&mut medium);
                *pdweffect = DROPEFFECT_LINK;
            }
        }
        if paths.is_empty() {
            self.post(WM_DRAG, 0, 0);
        } else {
            self.post(WM_DROPPED, 0, Box::into_raw(Box::new(paths)) as isize);
        }
        Ok(())
    }
}

static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Whether the window is a drop target (it is, unless Windows refused).
pub fn registered() -> bool {
    REGISTERED.load(Ordering::Relaxed)
}

/// Make the window accept dropped files. Must run on the window's thread.
pub fn register(hwnd: HWND) {
    unsafe {
        let _ = OleInitialize(None);
        let target: IDropTarget = Dropper { hwnd: hwnd.0 as isize }.into();
        REGISTERED.store(RegisterDragDrop(hwnd, &target).is_ok(), Ordering::Relaxed);
    }
}
