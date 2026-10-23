//! The real controls: what the panel's buttons and sliders do to Windows.
//!
//! One worker thread does all of it, so a slow call (a radio switching, a network
//! connecting) never freezes the notch. The panel reads a shared copy of the state and
//! sends commands; the worker keeps the copy up to date after each command and whenever
//! the panel is about to be shown (`Refresh`).

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use windows::core::{Interface, GUID, HRESULT, PCWSTR};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TOPOLOGY_CLONE,
    DISPLAYCONFIG_TOPOLOGY_EXTEND, DISPLAYCONFIG_TOPOLOGY_EXTERNAL, DISPLAYCONFIG_TOPOLOGY_ID, DISPLAYCONFIG_TOPOLOGY_INTERNAL, QDC_DATABASE_CURRENT,
    SDC_APPLY, SDC_TOPOLOGY_CLONE, SDC_TOPOLOGY_EXTEND, SDC_TOPOLOGY_EXTERNAL, SDC_TOPOLOGY_INTERNAL,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::NetworkManagement::WiFi::{
    WlanConnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS,
    WLAN_INTERFACE_INFO_LIST, wlan_connection_mode_profile, dot11_BSS_type_any,
};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS, SW_SHOWNORMAL};

use crate::events::SystemEvent;
use crate::modules::controls::{ControlCommand, ControlsBackend, ControlsState, Network, Output, ProjectMode};

// ------------------------------------------------- the one undocumented piece

/// Switching the default output has no public API; this is the interface Windows' own Sound
/// settings and every audio switcher use. Only `SetDefaultEndpoint` is called.
#[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows_core::IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, fx: i32, key: *const PROPERTYKEY, value: *mut PROPVARIANT) -> HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, fx: i32, key: *const PROPERTYKEY, value: *mut PROPVARIANT) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

// ------------------------------------------------------------------ backend

pub struct WinControls {
    shared: Arc<Mutex<ControlsState>>,
    tx: Sender<ControlCommand>,
}

impl WinControls {
    pub fn start() -> Arc<WinControls> {
        let shared = Arc::new(Mutex::new(ControlsState::default()));
        let (tx, rx) = channel();
        let worker_state = shared.clone();
        let _ = std::thread::Builder::new().name("blink-controls".into()).spawn(move || worker(rx, worker_state));
        let me = Arc::new(WinControls { shared, tx });
        me.send(ControlCommand::Refresh);
        me
    }

    /// The volume or brightness changed somewhere else (a key, Windows' own slider): keep up.
    pub fn apply_event(&self, event: &SystemEvent) {
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        match event {
            SystemEvent::Volume { percent, muted } => {
                s.volume = *percent;
                s.muted = *muted;
            }
            SystemEvent::Brightness { percent } => s.brightness = Some(*percent),
            SystemEvent::Wifi { connected, ssid } => {
                for n in s.networks.iter_mut() {
                    n.connected = *connected && ssid.as_deref() == Some(n.name.as_str());
                }
            }
            _ => {}
        }
    }
}

impl ControlsBackend for WinControls {
    fn state(&self) -> ControlsState {
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn send(&self, command: ControlCommand) {
        let _ = self.tx.send(command);
    }
}

fn worker(rx: Receiver<ControlCommand>, shared: Arc<Mutex<ControlsState>>) {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let mut ctx = Context::new();
    while let Ok(first) = rx.recv() {
        // Many slider moves arrive in a burst: only the last of each kind matters.
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        let mut last_volume = None;
        let mut last_brightness = None;
        let mut rest = Vec::new();
        for c in batch {
            match c {
                ControlCommand::SetVolume(_) => last_volume = Some(c),
                ControlCommand::SetBrightness(_) => last_brightness = Some(c),
                other => rest.push(other),
            }
        }
        for c in last_volume.into_iter().chain(last_brightness).chain(rest) {
            let refresh_all = matches!(c, ControlCommand::Refresh);
            ctx.run(&c);
            let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
            if refresh_all {
                *s = ctx.read_all();
            } else {
                ctx.read_after(&c, &mut s);
            }
        }
    }
}

struct Context {
    wlan: Option<HANDLE>,
}

impl Context {
    fn new() -> Context {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        let wlan = (unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) } == 0).then_some(handle);
        Context { wlan }
    }

    fn run(&mut self, c: &ControlCommand) {
        unsafe {
            match c {
                ControlCommand::SetVolume(v) => set_volume(*v),
                ControlCommand::VolumeStep(d) => {
                    if let Some((v, _)) = read_volume() {
                        set_volume((i16::from(v) + i16::from(*d)).clamp(0, 100) as u8);
                    }
                }
                ControlCommand::ToggleMute => {
                    if let Some(ep) = default_endpoint() {
                        if let Ok(m) = ep.GetMute() {
                            let _ = ep.SetMute(!m.as_bool(), std::ptr::null());
                        }
                    }
                }
                ControlCommand::SetOutput(id) => set_default_output(id),
                ControlCommand::SetBrightness(b) => set_brightness(*b),
                ControlCommand::SetWifi(on) => set_radio(RadioKind::WiFi, *on),
                ControlCommand::SetBluetooth(on) => set_radio(RadioKind::Bluetooth, *on),
                ControlCommand::ConnectWifi(name) => self.connect(name),
                ControlCommand::Project(m) => project(*m),
                ControlCommand::Cast => cast(),
                ControlCommand::WifiSettings => open_uri("ms-settings:network-wifi"),
                ControlCommand::Refresh => {}
            }
        }
    }

    /// Update the shared state after one command, reading back only what it could have changed.
    fn read_after(&mut self, c: &ControlCommand, s: &mut ControlsState) {
        match c {
            ControlCommand::SetVolume(_) | ControlCommand::VolumeStep(_) | ControlCommand::ToggleMute => {
                if let Some((v, m)) = read_volume() {
                    s.volume = v;
                    s.muted = m;
                }
            }
            ControlCommand::SetOutput(_) => {
                s.outputs = read_outputs();
                if let Some((v, m)) = read_volume() {
                    s.volume = v;
                    s.muted = m;
                }
            }
            ControlCommand::SetBrightness(b) => s.brightness = read_brightness().or(Some(*b)),
            ControlCommand::SetWifi(_) | ControlCommand::SetBluetooth(_) => {
                s.wifi = read_radio(RadioKind::WiFi);
                s.bluetooth = read_radio(RadioKind::Bluetooth);
                s.networks = self.read_networks();
            }
            ControlCommand::ConnectWifi(_) => s.networks = self.read_networks(),
            ControlCommand::Project(_) => {
                s.project = read_project();
                s.displays = unsafe { GetSystemMetrics(SM_CMONITORS) }.clamp(0, 255) as u8;
            }
            ControlCommand::Cast | ControlCommand::WifiSettings | ControlCommand::Refresh => {}
        }
    }

    fn read_all(&mut self) -> ControlsState {
        let (volume, muted) = read_volume().unwrap_or((0, false));
        ControlsState {
            volume,
            muted,
            outputs: read_outputs(),
            brightness: read_brightness(),
            wifi: read_radio(RadioKind::WiFi),
            bluetooth: read_radio(RadioKind::Bluetooth),
            networks: self.read_networks(),
            project: read_project(),
            displays: unsafe { GetSystemMetrics(SM_CMONITORS) }.clamp(0, 255) as u8,
        }
    }

    // ----------------------------------------------------------------- Wi-Fi

    fn first_interface(&self) -> Option<(HANDLE, GUID)> {
        let handle = self.wlan?;
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(handle, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let guid = ((*list).dwNumberOfItems > 0).then(|| (*list).InterfaceInfo[0].InterfaceGuid);
            WlanFreeMemory(list as *const c_void);
            guid.map(|g| (handle, g))
        }
    }

    /// Networks this computer already has a profile for, the connected one first.
    fn read_networks(&mut self) -> Vec<Network> {
        let Some((handle, guid)) = self.first_interface() else { return Vec::new() };
        let mut out: Vec<Network> = Vec::new();
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(handle, &guid, 0, None, &mut list) != 0 || list.is_null() {
                return out;
            }
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), (*list).dwNumberOfItems as usize);
            for n in items {
                const CONNECTED: u32 = 1;
                const HAS_PROFILE: u32 = 2;
                if n.dwFlags & HAS_PROFILE == 0 {
                    continue;
                }
                let len = n.strProfileName.iter().position(|c| *c == 0).unwrap_or(n.strProfileName.len());
                let name = String::from_utf16_lossy(&n.strProfileName[..len]);
                if !out.iter().any(|o| o.name == name) {
                    out.push(Network { name, connected: n.dwFlags & CONNECTED != 0 });
                }
            }
            WlanFreeMemory(list as *const c_void);
        }
        out.sort_by_key(|n| !n.connected);
        out
    }

    fn connect(&mut self, name: &str) {
        let Some((handle, guid)) = self.first_interface() else { return };
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(wide.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_any,
            dwFlags: 0,
        };
        unsafe {
            let _ = WlanConnect(handle, &guid, &params, None);
        }
    }
}

// -------------------------------------------------------------------- audio

unsafe fn enumerator() -> Option<IMMDeviceEnumerator> {
    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()
}

unsafe fn default_endpoint() -> Option<IAudioEndpointVolume> {
    let device = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
    device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
}

fn read_volume() -> Option<(u8, bool)> {
    unsafe {
        let ep = default_endpoint()?;
        let level = ep.GetMasterVolumeLevelScalar().ok()?;
        Some(((level * 100.0).round().clamp(0.0, 100.0) as u8, ep.GetMute().ok()?.as_bool()))
    }
}

fn set_volume(percent: u8) {
    unsafe {
        if let Some(ep) = default_endpoint() {
            let _ = ep.SetMasterVolumeLevelScalar(f32::from(percent.min(100)) / 100.0, std::ptr::null());
            // Raising the volume of a muted output is a request to hear it.
            if percent > 0 {
                let _ = ep.SetMute(false, std::ptr::null());
            }
        }
    }
}

fn read_outputs() -> Vec<Output> {
    unsafe {
        let Some(en) = enumerator() else { return Vec::new() };
        let default_id = en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| d.GetId().ok()).and_then(|p| p.to_string().ok());
        let Ok(devices) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        let count = devices.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..count {
            let Ok(d) = devices.Item(i) else { continue };
            let Some(id) = d.GetId().ok().and_then(|p| p.to_string().ok()) else { continue };
            let name = d
                .OpenPropertyStore(STGM_READ)
                .ok()
                .and_then(|s| s.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok())
                .and_then(|pv| PropVariantToStringAlloc(&pv).ok())
                .and_then(|p| p.to_string().ok())
                .unwrap_or_else(|| "Audio".to_string());
            out.push(Output { active: default_id.as_deref() == Some(id.as_str()), id, name });
        }
        out
    }
}

fn set_default_output(id: &str) {
    unsafe {
        let Ok(policy) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else { return };
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        for role in [eConsole, eMultimedia, eCommunications] {
            let _ = policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role);
        }
    }
}

// ------------------------------------------------------------------ brightness

// The video driver's brightness controls for the built-in screen.
// ntddvdeo.h: 0x230494 is QUERY_SUPPORTED_BRIGHTNESS (the list of levels), not this one.
const IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS: u32 = 0x0023_0498;
const IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS: u32 = 0x0023_049C;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DisplayBrightness {
    policy: u8,
    ac: u8,
    dc: u8,
}

unsafe fn lcd() -> Option<HANDLE> {
    let name: Vec<u16> = r"\\.\LCD".encode_utf16().chain(Some(0)).collect();
    CreateFileW(PCWSTR(name.as_ptr()), 0xC000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None).ok()
}

/// The built-in screen's brightness, or None when there is none (a desktop monitor).
fn read_brightness() -> Option<u8> {
    unsafe {
        let h = lcd()?;
        let mut b = DisplayBrightness::default();
        let mut got = 0u32;
        // ucDisplayPolicy 3 = both; the AC value is what the slider shows while plugged in.
        let ok = DeviceIoControl(h, IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS, None, 0, Some((&mut b as *mut DisplayBrightness).cast()), 3, Some(&mut got), None).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| b.ac.max(b.dc).min(100)).filter(|_| got >= 3)
    }
}

fn set_brightness(percent: u8) {
    unsafe {
        let Some(h) = lcd() else { return };
        let p = percent.min(100);
        // Policy 3 sets both the plugged-in and the on-battery level, so it sticks either way.
        let b = DisplayBrightness { policy: 3, ac: p, dc: p };
        let mut got = 0u32;
        let _ = DeviceIoControl(h, IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS, Some((&b as *const DisplayBrightness).cast()), 3, None, 0, Some(&mut got), None);
        let _ = CloseHandle(h);
    }
}

// ---------------------------------------------------------------------- radios

fn find_radio(kind: RadioKind) -> Option<Radio> {
    let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
    radios.into_iter().find(|r| r.Kind().ok() == Some(kind))
}

fn read_radio(kind: RadioKind) -> Option<bool> {
    Some(find_radio(kind)?.State().ok()? == RadioState::On)
}

fn set_radio(kind: RadioKind, on: bool) {
    let _ = Radio::RequestAccessAsync().and_then(|a| a.join());
    if let Some(radio) = find_radio(kind) {
        let _ = radio.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).and_then(|a| a.join());
    }
}

// --------------------------------------------------------------------- project

fn read_project() -> Option<ProjectMode> {
    unsafe {
        let (mut paths, mut modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut paths, &mut modes).0 != 0 {
            return None;
        }
        let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        let mut topology = DISPLAYCONFIG_TOPOLOGY_ID::default();
        if QueryDisplayConfig(QDC_DATABASE_CURRENT, &mut paths, path_info.as_mut_ptr(), &mut modes, mode_info.as_mut_ptr(), Some(&mut topology)).0 != 0 {
            return None;
        }
        match topology {
            t if t == DISPLAYCONFIG_TOPOLOGY_INTERNAL => Some(ProjectMode::PcOnly),
            t if t == DISPLAYCONFIG_TOPOLOGY_CLONE => Some(ProjectMode::Duplicate),
            t if t == DISPLAYCONFIG_TOPOLOGY_EXTEND => Some(ProjectMode::Extend),
            t if t == DISPLAYCONFIG_TOPOLOGY_EXTERNAL => Some(ProjectMode::SecondOnly),
            _ => None,
        }
    }
}

fn project(mode: ProjectMode) {
    let topology = match mode {
        ProjectMode::PcOnly => SDC_TOPOLOGY_INTERNAL,
        ProjectMode::Duplicate => SDC_TOPOLOGY_CLONE,
        ProjectMode::Extend => SDC_TOPOLOGY_EXTEND,
        ProjectMode::SecondOnly => SDC_TOPOLOGY_EXTERNAL,
    };
    unsafe {
        let _ = SetDisplayConfig(None, None, SDC_APPLY | topology);
    }
}

// ------------------------------------------------------------------------ misc

/// The cast (wireless display) list is Windows' own panel; Win+K opens it.
fn cast() {
    const VK_LWIN: u8 = 0x5B;
    const VK_K: u8 = 0x4B;
    unsafe {
        keybd_event(VK_LWIN, 0, Default::default(), 0);
        keybd_event(VK_K, 0, Default::default(), 0);
        keybd_event(VK_K, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_LWIN, 0, KEYEVENTF_KEYUP, 0);
    }
}

fn open_uri(uri: &str) {
    let wide: Vec<u16> = uri.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(None, windows::core::w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

#[allow(dead_code)]
fn _interface_check() -> Option<()> {
    // Keeps the `Interface` import honest on compilers that would otherwise flag it.
    let _: Option<GUID> = None;
    let _ = IPolicyConfig::IID;
    let _ = <IPolicyConfig as Interface>::IID;
    None
}
