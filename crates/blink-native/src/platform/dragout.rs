//! Dragging files out of the notch (the shelf) into another app: the mirror of `dropper`.
//!
//! The files are handed over the way Explorer hands them over (a shell data object with the files in it), so Discord, a folder,
//! a mail window and anything else that accepts a dropped file takes them. Nothing is copied or moved by Blink.

use windows::core::{implement, BOOL, PCWSTR};
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK};
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{SHCreateShellItemArrayFromIDLists, SHParseDisplayName, BHID_DataObject};

#[implement(IDropSource)]
struct Source;

#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> windows::core::HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if grfkeystate.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> windows::core::HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// A data object holding these files, as the shell makes one (from a shell item array, the way Explorer builds the object for a
/// selection). `None` when none of the paths can be used.
pub fn data_object(paths: &[String]) -> Option<IDataObject> {
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
    for path in paths {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if unsafe { SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None) }.is_ok() && !pidl.is_null() {
            pidls.push(pidl);
        }
    }
    if pidls.is_empty() {
        return None;
    }
    let constant: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const ITEMIDLIST).collect();
    let object: Option<IDataObject> = unsafe {
        SHCreateShellItemArrayFromIDLists(&constant)
            .ok()
            .and_then(|items| items.BindToHandler::<_, IDataObject>(None, &BHID_DataObject).ok())
    };
    for pidl in pidls {
        unsafe { CoTaskMemFree(Some(pidl as *const std::ffi::c_void)) };
    }
    object
}

/// Whether a data object carries files (the format every file drop target asks for).
pub fn carries_files(data: &IDataObject) -> bool {
    let format = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    unsafe { data.QueryGetData(&format) }.is_ok()
}

/// Carry these files with the mouse until the button is let go (offered as a copy: the originals stay where they are). Returns
/// whether somebody took them, and a line saying how it went.
/// Runs on the window's thread, which has OLE started; it returns when the drop is over.
pub fn drag_out(paths: &[String]) -> (bool, String) {
    let Some(data) = data_object(paths) else { return (false, format!("no data object for {} path(s)", paths.len())) };
    let carries = carries_files(&data);
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT_NONE;
    let result = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    // A target that moved the files itself reports no effect (an "optimized move"): files that are gone were taken too.
    let taken = effect != DROPEFFECT_NONE || paths.iter().any(|p| !std::path::Path::new(p).exists());
    (taken, format!("files={} carries_hdrop={carries} result=0x{:08x} effect={}", paths.len(), result.0, effect.0))
}
