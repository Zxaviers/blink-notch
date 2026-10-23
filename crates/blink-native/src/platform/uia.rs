//! UI Automation: lets screen readers and other assistive tools see the notch.
//!
//! The notch is drawn by hand, so Windows knows nothing about its contents. This
//! module answers `WM_GETOBJECT` with providers built over a snapshot of the app's
//! `access::Tree`. UI Automation calls providers from its own threads, so they
//! only read the snapshot (behind a mutex); anything that changes state is posted
//! to the window thread. Nothing here exists until an assistive tool first asks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::core::{implement, Error, IUnknown, Interface, Result, BSTR};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    IInvokeProvider, IInvokeProvider_Impl, IRawElementProviderFragment, IRawElementProviderFragmentRoot, IRawElementProviderFragmentRoot_Impl,
    IRawElementProviderFragment_Impl, IRawElementProviderSimple, IRawElementProviderSimple_Impl, NavigateDirection, NavigateDirection_FirstChild,
    NavigateDirection_LastChild, NavigateDirection_NextSibling, NavigateDirection_Parent, NavigateDirection_PreviousSibling,
    NotificationKind_Other, NotificationProcessing_ImportantMostRecent, ProviderOptions, ProviderOptions_ServerSideProvider,
    StructureChangeType_ChildrenInvalidated, UiaAppendRuntimeId, UiaClientsAreListening, UiaHostProviderFromHwnd, UiaRaiseAutomationEvent,
    UiaRaiseNotificationEvent, UiaRaiseStructureChangedEvent, UiaRect, UiaReturnRawElementProvider, UIA_AutomationFocusChangedEventId,
    UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId, UIA_FrameworkIdPropertyId,
    UIA_HasKeyboardFocusPropertyId, UIA_HelpTextPropertyId, UIA_InvokePatternId, UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId,
    UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_ListControlTypeId, UIA_ListItemControlTypeId,
    UIA_NamePropertyId, UIA_PaneControlTypeId, UIA_TextControlTypeId, UIA_PATTERN_ID, UIA_PROPERTY_ID,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::access::{Role, Tree, ROOT};

/// Posted to the window by `Invoke`; `wparam` is the element key.
pub const WM_A11Y_INVOKE: u32 = 0x8000 + 7;
pub const WM_GETOBJECT: u32 = 0x003D;
const UIA_ROOT_OBJECT_ID: i32 = -25;

/// Set the first time an assistive tool asks for the notch; until then no tree is built.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// An assistive tool is asking: from now on keep the snapshot current.
pub fn set_active() {
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

struct State {
    tree: Tree,
    /// Screen position of the window's top-left corner (physical px) and the display scale.
    origin: (i32, i32),
    scale: f32,
}

pub struct Shared {
    hwnd: isize,
    state: Mutex<State>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn shared(hwnd: HWND) -> Arc<Shared> {
    SHARED
        .get_or_init(|| Arc::new(Shared { hwnd: hwnd.0 as isize, state: Mutex::new(State { tree: Tree::default(), origin: (0, 0), scale: 1.0 }) }))
        .clone()
}

impl Shared {
    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    fn with<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

// ------------------------------------------------------------------ helpers

fn unavailable() -> Error {
    // UIA_E_ELEMENTNOTAVAILABLE: the element went away between the client's calls.
    Error::from_hresult(windows::core::HRESULT(0x8004_0201u32 as i32))
}

fn control_type(role: Role) -> i32 {
    match role {
        Role::Pane => UIA_PaneControlTypeId.0,
        Role::List => UIA_ListControlTypeId.0,
        Role::ListItem => UIA_ListItemControlTypeId.0,
        Role::Button => UIA_ButtonControlTypeId.0,
        Role::Text => UIA_TextControlTypeId.0,
    }
}

fn property(shared: &Shared, key: i32, id: UIA_PROPERTY_ID) -> VARIANT {
    shared.with(|s| {
        let Some(n) = s.tree.find(key) else { return VARIANT::default() };
        match id {
            x if x == UIA_NamePropertyId => VARIANT::from(BSTR::from(n.name.as_str())),
            x if x == UIA_ControlTypePropertyId => VARIANT::from(control_type(n.role)),
            x if x == UIA_ClassNamePropertyId => VARIANT::from(BSTR::from(if key == ROOT { "BlinkNotch" } else { "BlinkElement" })),
            x if x == UIA_AutomationIdPropertyId => VARIANT::from(BSTR::from(format!("Blink.{key}").as_str())),
            x if x == UIA_FrameworkIdPropertyId => VARIANT::from(BSTR::from("Blink")),
            x if x == UIA_HelpTextPropertyId => VARIANT::from(BSTR::from(n.help.as_str())),
            x if x == UIA_IsEnabledPropertyId || x == UIA_IsControlElementPropertyId || x == UIA_IsContentElementPropertyId => VARIANT::from(true),
            x if x == UIA_IsKeyboardFocusablePropertyId => VARIANT::from(n.action.is_some()),
            x if x == UIA_HasKeyboardFocusPropertyId => VARIANT::from(n.focused),
            x if x == UIA_IsOffscreenPropertyId => VARIANT::from(n.offscreen),
            _ => VARIANT::default(),
        }
    })
}

fn bounds(shared: &Shared, key: i32) -> UiaRect {
    shared.with(|s| {
        let Some(n) = s.tree.find(key) else { return UiaRect::default() };
        let (x, y, w, h) = n.rect;
        UiaRect {
            left: f64::from(s.origin.0) + f64::from(x * s.scale),
            top: f64::from(s.origin.1) + f64::from(y * s.scale),
            width: f64::from(w * s.scale),
            height: f64::from(h * s.scale),
        }
    })
}

fn runtime_id(shared: &Shared, key: i32) -> Result<*mut SAFEARRAY> {
    let ids = [UiaAppendRuntimeId as i32, shared.hwnd as i32, key];
    unsafe {
        let array = SafeArrayCreateVector(VT_I4, 0, ids.len() as u32);
        if array.is_null() {
            return Err(Error::from_hresult(E_FAIL));
        }
        for (i, id) in ids.iter().enumerate() {
            SafeArrayPutElement(array, &(i as i32), (id as *const i32).cast())?;
        }
        Ok(array)
    }
}

fn fragment(shared: &Arc<Shared>, key: i32) -> IRawElementProviderFragment {
    if key == ROOT {
        let simple: IRawElementProviderSimple = RootProvider { shared: shared.clone() }.into();
        simple.cast().expect("root is a fragment")
    } else {
        let simple: IRawElementProviderSimple = ItemProvider { shared: shared.clone(), key }.into();
        simple.cast().expect("item is a fragment")
    }
}

fn navigate(shared: &Arc<Shared>, key: i32, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
    let target = shared.with(|s| {
        let t = &s.tree;
        match direction {
            x if x == NavigateDirection_Parent => t.parent_of(key).map(|n| n.key),
            x if x == NavigateDirection_NextSibling => t.sibling(key, 1).map(|n| n.key),
            x if x == NavigateDirection_PreviousSibling => t.sibling(key, -1).map(|n| n.key),
            x if x == NavigateDirection_FirstChild => t.children(key).first().map(|n| n.key),
            x if x == NavigateDirection_LastChild => t.children(key).last().map(|n| n.key),
            _ => None,
        }
    });
    // No such neighbour: S_OK with nothing, which is how UI Automation is told "none".
    target.map(|k| fragment(shared, k)).ok_or_else(Error::empty)
}

// ---------------------------------------------------------------- providers

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot)]
struct RootProvider {
    shared: Arc<Shared>,
}

impl IRawElementProviderSimple_Impl for RootProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty())
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(property(&self.shared, ROOT, id))
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        unsafe { UiaHostProviderFromHwnd(self.shared.hwnd()) }
    }
}

impl IRawElementProviderFragment_Impl for RootProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        navigate(&self.shared, ROOT, direction)
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        runtime_id(&self.shared, ROOT)
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        Ok(bounds(&self.shared, ROOT))
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        fragment(&self.shared, ROOT).cast()
    }
}

impl IRawElementProviderFragmentRoot_Impl for RootProvider_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let key = self.shared.with(|s| {
            let lx = ((x - f64::from(s.origin.0)) / f64::from(s.scale)) as f32;
            let ly = ((y - f64::from(s.origin.1)) / f64::from(s.scale)) as f32;
            s.tree.at_point(lx, ly).map(|n| n.key)
        });
        Ok(fragment(&self.shared, key.unwrap_or(ROOT)))
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        match self.shared.with(|s| s.tree.focused().map(|n| n.key)) {
            Some(key) => Ok(fragment(&self.shared, key)),
            None => Err(Error::empty()),
        }
    }
}

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IInvokeProvider)]
struct ItemProvider {
    shared: Arc<Shared>,
    key: i32,
}

impl IRawElementProviderSimple_Impl for ItemProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        let invokable = self.shared.with(|s| s.tree.find(self.key).is_some_and(|n| n.action.is_some()));
        if pattern == UIA_InvokePatternId && invokable {
            let simple: IRawElementProviderSimple = ItemProvider { shared: self.shared.clone(), key: self.key }.into();
            let invoke: IInvokeProvider = simple.cast()?;
            return invoke.cast();
        }
        Err(Error::empty())
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        if self.shared.with(|s| s.tree.find(self.key).is_none()) {
            return Err(unavailable());
        }
        Ok(property(&self.shared, self.key, id))
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        Err(Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for ItemProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        navigate(&self.shared, self.key, direction)
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        runtime_id(&self.shared, self.key)
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        if self.shared.with(|s| s.tree.find(self.key).is_none()) {
            return Err(unavailable());
        }
        Ok(bounds(&self.shared, self.key))
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        fragment(&self.shared, ROOT).cast()
    }
}

impl IInvokeProvider_Impl for ItemProvider_Impl {
    fn Invoke(&self) -> Result<()> {
        if self.shared.with(|s| s.tree.find(self.key).is_none()) {
            return Err(unavailable());
        }
        // State lives on the window thread: ask it to do the work.
        unsafe { PostMessageW(Some(self.shared.hwnd()), WM_A11Y_INVOKE, WPARAM(self.key as isize as usize), LPARAM(0)) }
            .map_err(|_| Error::from_hresult(E_FAIL))
    }
}

// ----------------------------------------------------------- window thread

/// Answer `WM_GETOBJECT`. None when the request is not for the UI Automation root.
pub fn on_get_object(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if lparam.0 as i32 != UIA_ROOT_OBJECT_ID {
        return None;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    let simple: IRawElementProviderSimple = RootProvider { shared: shared(hwnd) }.into();
    Some(unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &simple) })
}

/// What changed since the last snapshot, for the caller to announce.
pub struct Publish {
    pub structure_changed: bool,
    pub focus_changed: Option<i32>,
}

/// Replace the snapshot (window thread) and tell listening clients what changed.
pub fn publish(hwnd: HWND, tree: Tree, origin: (i32, i32), scale: f32) {
    let shared = shared(hwnd);
    let change = {
        let mut s = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        let structure_changed = !s.tree.same_structure(&tree);
        let before = s.tree.focused().map(|n| n.key);
        let after = tree.focused().map(|n| n.key);
        s.tree = tree;
        s.origin = origin;
        s.scale = scale;
        Publish { structure_changed, focus_changed: if before != after { after } else { None } }
    };
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    if change.structure_changed {
        let root: IRawElementProviderSimple = RootProvider { shared: shared.clone() }.into();
        unsafe {
            let _ = UiaRaiseStructureChangedEvent(&root, StructureChangeType_ChildrenInvalidated, std::ptr::null_mut(), 0);
        }
    }
    if let Some(key) = change.focus_changed {
        let element: IRawElementProviderSimple = if key == ROOT {
            RootProvider { shared: shared.clone() }.into()
        } else {
            ItemProvider { shared: shared.clone(), key }.into()
        };
        unsafe {
            let _ = UiaRaiseAutomationEvent(&element, UIA_AutomationFocusChangedEventId);
        }
    }
}

/// Have the screen reader say `text` (a reminder, "Marked done: ...").
pub fn announce(hwnd: HWND, text: &str) {
    if !active() || !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    let root: IRawElementProviderSimple = RootProvider { shared: shared(hwnd) }.into();
    unsafe {
        let _ = UiaRaiseNotificationEvent(&root, NotificationKind_Other, NotificationProcessing_ImportantMostRecent, &BSTR::from(text), &BSTR::from("blink"));
    }
}
