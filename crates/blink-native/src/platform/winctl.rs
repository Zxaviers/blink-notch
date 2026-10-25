//! The real controls: what the panel's buttons and sliders do to Windows.
//!
//! One worker thread does all of it, so a slow call (a radio switching, a network
//! connecting) never freezes the notch. The panel reads a shared copy of the state and
//! sends commands; the worker keeps the copy up to date after each command and whenever
//! the panel is about to be shown (`Refresh`).

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    eCapture, eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::NetworkManagement::WiFi::{
    WlanConnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle, WlanScan, WlanSetProfile, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS,
    WLAN_INTERFACE_INFO_LIST, wlan_connection_mode_profile, dot11_BSS_type_any,
};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS, SW_SHOWNORMAL};

use blink_core::sonar::Sonar;

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
    /// The volume the panel shows is Sonar's master (Windows' own volume events are then about one channel and are not it).
    sonar: Arc<AtomicBool>,
}

impl WinControls {
    pub fn start() -> Arc<WinControls> {
        let shared = Arc::new(Mutex::new(ControlsState::default()));
        let (tx, rx) = channel();
        let worker_state = shared.clone();
        let sonar = Arc::new(AtomicBool::new(false));
        let worker_sonar = sonar.clone();
        let _ = std::thread::Builder::new().name("blink-controls".into()).spawn(move || worker(rx, worker_state, worker_sonar));
        let me = Arc::new(WinControls { shared, tx, sonar });
        me.send(ControlCommand::Refresh);
        me
    }

    /// The volume or brightness changed somewhere else (a key, Windows' own slider): keep up.
    pub fn apply_event(&self, event: &SystemEvent) {
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        match event {
            SystemEvent::Volume { percent, muted } => {
                if !self.sonar.load(Ordering::Relaxed) {
                    s.volume = *percent;
                    s.muted = *muted;
                }
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

fn worker(rx: Receiver<ControlCommand>, shared: Arc<Mutex<ControlsState>>, sonar_flag: Arc<AtomicBool>) {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let mut ctx = Context::new();
    ctx.look_for_sonar();
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
            sonar_flag.store(ctx.sonar_target().is_some(), Ordering::Relaxed);
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
    /// Sonar's master, when SteelSeries Sonar runs in classic mode (looked for again whenever the panel is about to show).
    sonar: Option<Sonar>,
    sonar_looked: Option<Instant>,
}

impl Context {
    fn new() -> Context {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        let wlan = (unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) } == 0).then_some(handle);
        Context { wlan, sonar: None, sonar_looked: None }
    }

    /// Look for Sonar through GG (at most every few seconds: most PCs have neither and the answer rarely changes).
    fn look_for_sonar(&mut self) {
        if self.sonar_looked.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        self.sonar_looked = Some(Instant::now());
        self.sonar = Sonar::discover().filter(Sonar::is_classic);
    }

    /// Sonar's master, when the sound goes through Sonar: its virtual output is the default one. Anything else is Windows' volume.
    fn sonar_target(&self) -> Option<&Sonar> {
        self.sonar.as_ref().filter(|_| default_output_name().is_some_and(|n| n.contains("Sonar")))
    }

    /// The volume and mute the panel shows.
    fn volume(&self) -> Option<(u8, bool)> {
        self.sonar_target().and_then(Sonar::master).or_else(read_volume)
    }

    fn set_level(&self, percent: u8) {
        match self.sonar_target() {
            Some(s) if s.set_volume(percent) => {
                // Raising the volume of a muted output is a request to hear it.
                if percent > 0 {
                    let _ = s.set_muted(false);
                }
            }
            _ => set_volume(percent),
        }
    }

    fn run(&mut self, c: &ControlCommand) {
        unsafe {
            match c {
                ControlCommand::SetVolume(v) => self.set_level(*v),
                ControlCommand::VolumeStep(d) => {
                    if let Some((v, _)) = self.volume() {
                        self.set_level((i16::from(v) + i16::from(*d)).clamp(0, 100) as u8);
                    }
                }
                ControlCommand::ToggleMute => {
                    let by_sonar = self.sonar_target().and_then(|s| s.set_muted(!s.master()?.1).then_some(())).is_some();
                    if let Some(ep) = default_endpoint().filter(|_| !by_sonar) {
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
                ControlCommand::ScanWifi => self.scan(),
                ControlCommand::Project(m) => project(*m),
                ControlCommand::ToggleMic => {
                    if let Some(ep) = default_capture() {
                        if let Ok(m) = ep.GetMute() {
                            let _ = ep.SetMute(!m.as_bool(), std::ptr::null());
                        }
                    }
                }
                ControlCommand::Cast => cast(),
                ControlCommand::Lock => {
                    let _ = LockWorkStation();
                }
                ControlCommand::Snip => open_uri("ms-screenclip:"),
                ControlCommand::OpenSettings => open_uri("ms-settings:"),
                ControlCommand::WifiSettings => open_uri("ms-settings:network-wifi"),
                ControlCommand::Refresh => self.look_for_sonar(),
                ControlCommand::ShowPage(_) => {}
            }
        }
    }

    /// Update the shared state after one command, reading back only what it could have changed.
    fn read_after(&mut self, c: &ControlCommand, s: &mut ControlsState) {
        match c {
            ControlCommand::SetVolume(_) | ControlCommand::VolumeStep(_) | ControlCommand::ToggleMute => {
                if let Some((v, m)) = self.volume() {
                    s.volume = v;
                    s.muted = m;
                }
            }
            ControlCommand::SetOutput(_) => {
                s.outputs = read_outputs();
                if let Some((v, m)) = self.volume() {
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
            ControlCommand::ToggleMic => s.mic_muted = read_mic_muted(),
            ControlCommand::ConnectWifi(_) => s.networks = self.read_networks(),
            ControlCommand::Project(_) => {
                s.project = read_project();
                s.displays = unsafe { GetSystemMetrics(SM_CMONITORS) }.clamp(0, 255) as u8;
            }
            ControlCommand::Cast
            | ControlCommand::Lock
            | ControlCommand::Snip
            | ControlCommand::OpenSettings
            | ControlCommand::WifiSettings
            | ControlCommand::ScanWifi
            | ControlCommand::Refresh
            | ControlCommand::ShowPage(_) => {}
        }
    }

    fn read_all(&mut self) -> ControlsState {
        let (volume, muted) = self.volume().unwrap_or((0, false));
        ControlsState {
            volume,
            muted,
            outputs: read_outputs(),
            brightness: read_brightness(),
            wifi: read_radio(RadioKind::WiFi),
            bluetooth: read_radio(RadioKind::Bluetooth),
            mic_muted: read_mic_muted(),
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

    /// Networks in range (the one in use first, then ones Windows has a password for, then by signal).
    fn read_networks(&mut self) -> Vec<Network> {
        self.read_networks_raw().into_iter().map(|r| r.network).collect()
    }

    fn read_networks_raw(&mut self) -> Vec<RawNetwork> {
        let Some((handle, guid)) = self.first_interface() else { return Vec::new() };
        let mut out: Vec<RawNetwork> = Vec::new();
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(handle, &guid, 0, None, &mut list) != 0 || list.is_null() {
                return out;
            }
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), (*list).dwNumberOfItems as usize);
            for n in items {
                const CONNECTED: u32 = 1;
                const HAS_PROFILE: u32 = 2;
                let ssid_len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                let ssid = String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..ssid_len]).to_string();
                if ssid.is_empty() {
                    continue; // a hidden network has no name to show
                }
                let saved = n.dwFlags & HAS_PROFILE != 0;
                let profile = if saved {
                    let len = n.strProfileName.iter().position(|c| *c == 0).unwrap_or(n.strProfileName.len());
                    String::from_utf16_lossy(&n.strProfileName[..len])
                } else {
                    ssid.clone()
                };
                let network = Network { name: profile.clone(), connected: n.dwFlags & CONNECTED != 0, signal: n.wlanSignalQuality.min(100) as u8, secured: n.bSecurityEnabled.as_bool(), saved };
                // The same name can be heard on several access points: keep the strongest.
                match out.iter_mut().find(|o| o.network.name == network.name) {
                    Some(existing) => {
                        if network.connected || (!existing.network.connected && network.signal > existing.network.signal) {
                            *existing = RawNetwork { network, ssid };
                        }
                    }
                    None => out.push(RawNetwork { network, ssid }),
                }
            }
            WlanFreeMemory(list as *const c_void);
        }
        out.sort_by_key(|o| (!o.network.connected, !o.network.saved, 255 - o.network.signal));
        out
    }

    /// Ask the Wi-Fi card to look around again (the list updates a moment later).
    fn scan(&mut self) {
        let Some((handle, guid)) = self.first_interface() else { return };
        unsafe {
            let _ = WlanScan(handle, &guid, None, None, None);
        }
    }

    /// A saved network connects directly; an open one gets a profile first; one that needs a
    /// password is Windows' to ask for.
    fn connect(&mut self, name: &str) {
        let Some((handle, guid)) = self.first_interface() else { return };
        let Some(found) = self.read_networks_raw().into_iter().find(|r| r.network.name == name) else { return };
        let profile = if found.network.saved {
            name.to_string()
        } else if !found.network.secured {
            let xml = open_profile_xml(&found.ssid);
            let wide: Vec<u16> = xml.encode_utf16().chain(Some(0)).collect();
            let mut reason = 0u32;
            let rc = unsafe { WlanSetProfile(handle, &guid, 0, PCWSTR(wide.as_ptr()), PCWSTR::null(), false, None, &mut reason) };
            if rc != 0 {
                return;
            }
            found.ssid.clone()
        } else {
            open_uri("ms-settings:network-wifi");
            return;
        };
        let wide: Vec<u16> = profile.encode_utf16().chain(Some(0)).collect();
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

struct RawNetwork {
    network: Network,
    /// The name as heard on the air (the profile may be called something else).
    ssid: String,
}

/// The profile of a network with no password: connect by hand, no security.
fn open_profile_xml(ssid: &str) -> String {
    let name = ssid.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;");
    format!(
        "<?xml version=\"1.0\"?><WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\"><name>{name}</name><SSIDConfig><SSID><name>{name}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>manual</connectionMode><MSM><security><authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption></security></MSM></WLANProfile>"
    )
}

// -------------------------------------------------------------------- audio

unsafe fn enumerator() -> Option<IMMDeviceEnumerator> {
    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()
}

/// The friendly name of the default output.
fn default_output_name() -> Option<String> {
    unsafe {
        let device = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok()?;
        PropVariantToStringAlloc(&value).ok()?.to_string().ok()
    }
}

unsafe fn default_endpoint() -> Option<IAudioEndpointVolume> {
    let device = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
    device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
}

unsafe fn default_capture() -> Option<IAudioEndpointVolume> {
    let device = enumerator()?.GetDefaultAudioEndpoint(eCapture, eConsole).ok()?;
    device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
}

/// Whether the default microphone is muted; None when there is no microphone.
fn read_mic_muted() -> Option<bool> {
    unsafe { Some(default_capture()?.GetMute().ok()?.as_bool()) }
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
