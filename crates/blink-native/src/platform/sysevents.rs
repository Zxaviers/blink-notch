//! Where system events come from. Everything here is event driven: Windows tells us
//! when something happens, and the notch sleeps in between (no timers, no polling).
//!
//! - **Power**: a power-setting notification for the power source and the charge level.
//! - **Wi-Fi**: the WLAN service's connect and disconnect notifications.
//! - **Audio**: MMDevice notifications for outputs appearing, going and becoming the default
//!   one, and the default output's volume and mute. These run on their own thread.
//! - **Keys**: raw keyboard input, used for exactly two things: the Caps Lock key and the keys
//!   that switch the keyboard layout (Shift, Alt, Windows, Space). Every other key is looked at
//!   for its code only to throw it away; nothing is stored, logged or sent anywhere.
//!
//! Sources run on other threads post a `SystemEvent` to the window thread as `WM_SYSEVENT`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Sender};

use windows::core::{implement, Interface, GUID, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SENGLISHDISPLAYNAME, LOCALE_SISO639LANGNAME};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMEndpoint, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};
use windows::Win32::NetworkManagement::WiFi::{
    WlanOpenHandle, WlanRegisterNotification, L2_NOTIFICATION_DATA, WLAN_CONNECTION_NOTIFICATION_DATA, WLAN_NOTIFICATION_SOURCE_ACM,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PropVariantToUInt32, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Power::{GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyState};
use windows::Win32::UI::Input::{GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, DEVICE_NOTIFY_WINDOW_HANDLE};

use crate::events::{AudioKind, SystemEvent};

/// Posted to the window; `lparam` is a `Box<SystemEvent>` raw pointer.
pub const WM_SYSEVENT: u32 = 0x8000 + 8;
pub const WM_INPUT: u32 = 0x00FF;
pub const WM_POWERBROADCAST: u32 = 0x0218;
pub const PBT_POWERSETTINGCHANGE: usize = 0x8013;

static TARGET: AtomicIsize = AtomicIsize::new(0);

/// Which sources came up (a desktop PC has no battery or Wi-Fi, a service may be off).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sources {
    pub power: bool,
    pub wifi: bool,
    pub audio: bool,
    pub keys: bool,
}

fn post(event: SystemEvent) {
    let target = TARGET.load(Ordering::Relaxed);
    if target == 0 {
        return;
    }
    let raw = Box::into_raw(Box::new(event));
    if unsafe { PostMessageW(Some(HWND(target as *mut c_void)), WM_SYSEVENT, WPARAM(0), LPARAM(raw as isize)) }.is_err() {
        drop(unsafe { Box::from_raw(raw) });
    }
}

pub fn start(hwnd: HWND) -> Sources {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    Sources { power: start_power(hwnd), wifi: start_wifi(), audio: start_audio(), keys: start_keys(hwnd) }
}

// ------------------------------------------------------------------- power

fn start_power(hwnd: HWND) -> bool {
    unsafe {
        let a = RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_ACDC_POWER_SOURCE, DEVICE_NOTIFY_WINDOW_HANDLE);
        let b = RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_BATTERY_PERCENTAGE_REMAINING, DEVICE_NOTIFY_WINDOW_HANDLE);
        a.is_ok() && b.is_ok()
    }
}

/// The current power state, as an event.
pub fn power_event() -> Option<SystemEvent> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    // ACLineStatus: 0 battery, 1 plugged in, 255 unknown. BatteryLifePercent 255: unknown (no battery).
    if status.ACLineStatus == 255 {
        return None;
    }
    Some(SystemEvent::Power {
        on_ac: status.ACLineStatus == 1,
        percent: (status.BatteryLifePercent != 255).then_some(status.BatteryLifePercent),
        charging: status.BatteryFlag != 255 && status.BatteryFlag & 8 != 0,
    })
}

// -------------------------------------------------------------------- wi-fi

const ACM_CONNECTION_COMPLETE: u32 = 10;
const ACM_DISCONNECTED: u32 = 21;

unsafe extern "system" fn wlan_callback(data: *mut L2_NOTIFICATION_DATA, _context: *mut c_void) {
    if data.is_null() {
        return;
    }
    let d = &*data;
    if d.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM || d.pData.is_null() || (d.dwDataSize as usize) < std::mem::size_of::<WLAN_CONNECTION_NOTIFICATION_DATA>() - 2 {
        return;
    }
    let info = &*(d.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA);
    let name = || {
        let n = (info.dot11Ssid.uSSIDLength as usize).min(32);
        String::from_utf8_lossy(&info.dot11Ssid.ucSSID[..n]).into_owned()
    };
    match d.NotificationCode {
        ACM_CONNECTION_COMPLETE if info.wlanReasonCode == 0 => post(SystemEvent::Wifi { connected: true, ssid: Some(name()).filter(|s| !s.is_empty()) }),
        ACM_DISCONNECTED => post(SystemEvent::Wifi { connected: false, ssid: None }),
        _ => {}
    }
}

fn start_wifi() -> bool {
    unsafe {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        if WlanOpenHandle(2, None, &mut version, &mut handle) != 0 {
            return false; // no WLAN service: a desktop without Wi-Fi
        }
        WlanRegisterNotification(handle, WLAN_NOTIFICATION_SOURCE_ACM, true, Some(wlan_callback), None, None, None) == 0
    }
}

// ------------------------------------------------------------------- audio

const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
const PKEY_AUDIOENDPOINT_FORMFACTOR: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 0 };
const DEVICE_STATE_ACTIVE: u32 = 1;

enum AudioCmd {
    State(String, u32),
    DefaultChanged(String),
}

#[implement(IMMNotificationClient)]
struct DeviceWatcher {
    tx: Sender<AudioCmd>,
}

impl IMMNotificationClient_Impl for DeviceWatcher_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        if let Ok(id) = unsafe { id.to_string() } {
            let _ = self.tx.send(AudioCmd::State(id, state.0));
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, id: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            let _ = self.tx.send(AudioCmd::DefaultChanged(unsafe { id.to_string() }.unwrap_or_default()));
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeWatcher;

impl IAudioEndpointVolumeCallback_Impl for VolumeWatcher_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            post(SystemEvent::Volume { percent: (d.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as u8, muted: d.bMuted.as_bool() });
        }
        Ok(())
    }
}

struct Endpoint {
    name: String,
    kind: AudioKind,
}

unsafe fn describe(enumerator: &IMMDeviceEnumerator, id: &str) -> Option<Endpoint> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let device = enumerator.GetDevice(PCWSTR(wide.as_ptr())).ok()?;
    // Only outputs (headphones, speakers); microphones come and go without being news.
    let endpoint: IMMEndpoint = device.cast().ok()?;
    if endpoint.GetDataFlow().ok()? != eRender {
        return None;
    }
    let store = device.OpenPropertyStore(STGM_READ).ok()?;
    let text = |pv: PROPVARIANT| PropVariantToStringAlloc(&pv).ok().and_then(|p| p.to_string().ok());
    let name = text(store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok()?)?;
    let form = PropVariantToUInt32(&store.GetValue(&PKEY_AUDIOENDPOINT_FORMFACTOR).ok()?).unwrap_or(10);
    // Speakers 1, Headphones 3, Headset 5.
    let kind = match form {
        3 | 5 => AudioKind::Headphones,
        1 => AudioKind::Speakers,
        _ => AudioKind::Other,
    };
    Some(Endpoint { name, kind })
}

/// Point the volume callback at the current default output.
unsafe fn watch_volume(enumerator: &IMMDeviceEnumerator, current: &mut Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>) {
    if let Some((endpoint, callback)) = current.take() {
        let _ = endpoint.UnregisterControlChangeNotify(&callback);
    }
    let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else { return };
    let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return };
    let callback: IAudioEndpointVolumeCallback = VolumeWatcher.into();
    if endpoint.RegisterControlChangeNotify(&callback).is_ok() {
        *current = Some((endpoint, callback));
    }
}

fn start_audio() -> bool {
    let (ready_tx, ready_rx) = channel::<bool>();
    std::thread::Builder::new()
        .name("blink-audio".into())
        .spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
                let _ = ready_tx.send(false);
                return;
            };
            let (tx, rx) = channel::<AudioCmd>();
            let watcher: IMMNotificationClient = DeviceWatcher { tx }.into();
            let registered = enumerator.RegisterEndpointNotificationCallback(&watcher).is_ok();
            let mut volume = None;
            watch_volume(&enumerator, &mut volume);
            let _ = ready_tx.send(registered);
            // Device queries happen here, not inside the notification callbacks.
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    AudioCmd::State(id, state) => {
                        if let Some(e) = describe(&enumerator, &id) {
                            post(SystemEvent::AudioDevice { name: e.name, kind: e.kind, connected: state == DEVICE_STATE_ACTIVE });
                        }
                    }
                    AudioCmd::DefaultChanged(id) => {
                        watch_volume(&enumerator, &mut volume);
                        // Switching to headphones is news; switching back to speakers is not.
                        if let Some(e) = describe(&enumerator, &id).filter(|e| e.kind == AudioKind::Headphones) {
                            post(SystemEvent::AudioDevice { name: e.name, kind: e.kind, connected: true });
                        }
                    }
                }
            }
        })
        .is_ok()
        && ready_rx.recv().unwrap_or(false)
}

// -------------------------------------------------------------------- keys

const VK_CAPITAL: u16 = 0x14;

fn start_keys(hwnd: HWND) -> bool {
    let device = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
    unsafe { RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32) }.is_ok()
}

/// What a raw keyboard message tells us, reduced to what the notch cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyHint {
    /// Caps Lock was pressed.
    Caps,
    /// A key that can switch the keyboard layout was released.
    LayoutKey,
    /// Anything else: dropped here.
    Other,
}

/// Read a `WM_INPUT` message. Only the key code and whether it was a press or a release
/// are looked at, and only to decide between the three outcomes above.
pub fn key_hint(lparam: LPARAM) -> KeyHint {
    unsafe {
        let mut size = 0u32;
        let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        GetRawInputData(HRAWINPUT(lparam.0 as *mut c_void), RID_INPUT, None, &mut size, header);
        if size == 0 || size as usize > std::mem::size_of::<RAWINPUT>() {
            return KeyHint::Other;
        }
        let mut raw = RAWINPUT::default();
        if GetRawInputData(HRAWINPUT(lparam.0 as *mut c_void), RID_INPUT, Some((&mut raw as *mut RAWINPUT).cast()), &mut size, header) != size {
            return KeyHint::Other;
        }
        if raw.header.dwType != 1 {
            return KeyHint::Other; // not the keyboard
        }
        let k = raw.data.keyboard;
        let released = k.Flags & 1 != 0;
        match k.VKey {
            VK_CAPITAL if !released => KeyHint::Caps,
            // Shift, Control, Alt, Windows, Space (and their left and right variants)
            0x10 | 0x11 | 0x12 | 0x20 | 0x5B | 0x5C | 0xA0..=0xA5 if released => KeyHint::LayoutKey,
            _ => KeyHint::Other,
        }
    }
}

pub fn caps_lock_on() -> bool {
    unsafe { GetKeyState(i32::from(VK_CAPITAL)) & 1 != 0 }
}

/// The keyboard layout of the window you are typing in: (short code, name).
pub fn foreground_layout() -> Option<(String, String)> {
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let hkl = GetKeyboardLayout(thread);
        let lang = (hkl.0 as usize & 0xffff) as u32;
        let info = |kind| {
            let mut buf = [0u16; 96];
            let n = GetLocaleInfoW(lang, kind, Some(&mut buf));
            (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
        };
        let code = info(LOCALE_SISO639LANGNAME)?.to_uppercase();
        Some((code, info(LOCALE_SENGLISHDISPLAYNAME).unwrap_or_default()))
    }
}
