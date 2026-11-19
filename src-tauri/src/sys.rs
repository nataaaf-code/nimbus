//! Integrasi Windows (WinRT + Core Audio). Khusus Windows. BELUM dikompilasi/diuji (lihat docs/HANDOFF.md).
use serde::{Serialize, Deserialize};
use std::process::Command;
use windows::Media::Control::*;
type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String { x.to_string() }

#[derive(Serialize)] pub struct Media { pub title: String, pub artist: String, pub playing: bool, pub pos: i64, pub dur: i64, pub cover: Option<String> }
#[derive(Serialize)] pub struct Net { pub ssid: String, pub secure: bool, pub bars: u8, pub connected: bool }
#[derive(Serialize)] pub struct SystemStats { pub cpu: f32, pub memory_used: u64, pub memory_total: u64, pub net_rx: u64, pub net_tx: u64, pub battery: Option<u8>, pub charging: bool }

pub fn hide_console(c: &mut Command) { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
fn ps(script: &str) -> R<String> {
    let mut c = Command::new("powershell"); c.args(["-NoProfile", "-Command", script]); hide_console(&mut c);
    let o = c.output().map_err(e)?;
    if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}
/// CPU (delta GetSystemTimes sejak panggilan sebelumnya), RAM, dan total byte jaringan
/// antarmuka fisik (UI menghitung laju dari selisihnya). Tanpa PowerShell.
pub fn system_stats() -> R<SystemStats> {
    static PREV: std::sync::Mutex<Option<(u64, u64, u64)>> = std::sync::Mutex::new(None);
    let (i, k, u) = crate::win::system_times();
    let cpu = {
        let mut g = PREV.lock().map_err(e)?;
        let v = match *g {
            Some((pi, pk, pu)) => {
                let total = (k - pk) + (u - pu);
                if total == 0 { 0.0 } else { (1.0 - (i - pi) as f64 / total as f64) * 100.0 }
            }
            None => 0.0,
        };
        *g = Some((i, k, u));
        v.clamp(0.0, 100.0) as f32
    };
    let (used, total) = crate::win::memory();
    let (rx, tx) = net_bytes();
    let (battery, charging) = battery_status().unwrap_or((None, false));
    Ok(SystemStats { cpu, memory_used: used, memory_total: total, net_rx: rx, net_tx: tx, battery, charging })
}
fn net_bytes() -> (u64, u64) {
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    let (mut rx, mut tx) = (0u64, 0u64);
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t).is_ok() && !t.is_null() {
            let n = (*t).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), n);
            for r in rows {
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                // bit0 = HardwareInterface, bit1 = FilterInterface; 24 = loopback
                if r.Type != 24 && flags & 1 != 0 && flags & 2 == 0 {
                    rx += r.InOctets;
                    tx += r.OutOctets;
                }
            }
            FreeMibTable(t as *const _);
        }
    }
    (rx, tx)
}
fn battery_status() -> Option<(Option<u8>, bool)> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    // 255 = tidak ada baterai (desktop/PC), 0 = tidak diketahui
    let pct = if s.BatteryLifePercent < 101 { Some(s.BatteryLifePercent) } else { None };
    let charging = s.ACLineStatus == 1;
    Some((pct, charging))
}
// ---- Volume & kecerahan
#[repr(C)]
#[derive(Clone, Copy)]
struct PHYSICAL_MONITOR {
    h_physical_monitor: *mut std::ffi::c_void,
    sz_physical_monitor_description: [u16; 128],
}

#[link(name = "dxva2")]
extern "system" {
    fn GetNumberOfPhysicalMonitorsFromHMONITOR(
        h_monitor: *mut std::ffi::c_void,
        pdw_number_of_physical_monitors: *mut u32,
    ) -> i32;
    fn GetPhysicalMonitorsFromHMONITOR(
        h_monitor: *mut std::ffi::c_void,
        dw_physical_monitor_array_size: u32,
        p_physical_monitor_array: *mut PHYSICAL_MONITOR,
    ) -> i32;
    fn GetMonitorBrightness(
        h_monitor: *mut std::ffi::c_void,
        pdw_minimum_brightness: *mut u32,
        pdw_current_brightness: *mut u32,
        pdw_maximum_brightness: *mut u32,
    ) -> i32;
    fn SetMonitorBrightness(h_monitor: *mut std::ffi::c_void, dw_new_brightness: u32) -> i32;
    fn DestroyPhysicalMonitors(
        dw_physical_monitor_array_size: u32,
        p_physical_monitor_array: *const PHYSICAL_MONITOR,
    ) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn MonitorFromWindow(hwnd: *mut std::ffi::c_void, dw_flags: u32) -> *mut std::ffi::c_void;
}

fn with_physical_monitors<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&[PHYSICAL_MONITOR]) -> Option<T>,
{
    unsafe {
        let hmon = MonitorFromWindow(std::ptr::null_mut(), 1);
        if hmon.is_null() {
            return None;
        }
        let mut count = 0u32;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) == 0 || count == 0 {
            return None;
        }
        let mut monitors = vec![
            PHYSICAL_MONITOR {
                h_physical_monitor: std::ptr::null_mut(),
                sz_physical_monitor_description: [0; 128],
            };
            count as usize
        ];
        if GetPhysicalMonitorsFromHMONITOR(hmon, count, monitors.as_mut_ptr()) == 0 {
            return None;
        }
        let res = f(&monitors);
        DestroyPhysicalMonitors(count, monitors.as_ptr());
        res
    }
}

fn get_dxva2_brightness() -> Option<u32> {
    with_physical_monitors(|mons| {
        for m in mons {
            let mut min = 0u32;
            let mut cur = 0u32;
            let mut max = 0u32;
            if unsafe { GetMonitorBrightness(m.h_physical_monitor, &mut min, &mut cur, &mut max) } != 0 {
                return Some(cur);
            }
        }
        None
    })
}

fn set_dxva2_brightness(level: u32) -> bool {
    with_physical_monitors(|mons| {
        let mut ok = false;
        for m in mons {
            if unsafe { SetMonitorBrightness(m.h_physical_monitor, level) } != 0 {
                ok = true;
            }
        }
        if ok { Some(()) } else { None }
    }).is_some()
}

fn vol() -> R<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::{Media::Audio::*, System::Com::*};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
        en.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(e)?.Activate(CLSCTX_ALL, None).map_err(e)
    }
}
pub fn set_volume(l: f32) -> R<()> { unsafe { vol()?.SetMasterVolumeLevelScalar(l.clamp(0.0, 1.0), std::ptr::null()).map_err(e) } }
pub fn set_brightness(n: u32) -> R<()> {
    if set_dxva2_brightness(n) {
        return Ok(());
    }
    ps(&format!("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods).WmiSetBrightness(1,{n})")).map(|_| ())
}
pub fn levels() -> R<(f32, Option<u32>)> {
    let v = unsafe { vol()?.GetMasterVolumeLevelScalar().map_err(e)? };
    let b = get_dxva2_brightness().or_else(|| {
        ps("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness).CurrentBrightness")
            .ok()
            .and_then(|s| s.lines().next()?.trim().parse().ok())
    });
    Ok((v, b))
}
// ---- Media (SMTC: Spotify, YouTube Music, dll)
fn sess() -> R<Option<GlobalSystemMediaTransportControlsSession>> {
    Ok(GlobalSystemMediaTransportControlsSessionManager::RequestAsync().map_err(e)?.get().map_err(e)?.GetCurrentSession().ok())
}
fn get_thumbnail_base64(props: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<String> {
    use base64::Engine;
    use windows::Storage::Streams::DataReader;

    let thumb_ref = props.Thumbnail().ok()?;
    let stream = thumb_ref.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 5_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    let content_type = stream.ContentType().map(|s| s.to_string()).unwrap_or_else(|_| "image/jpeg".into());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{content_type};base64,{b64}"))
}
pub fn media() -> R<Option<Media>> {
    let Some(s) = sess()? else { return Ok(None) };
    let p = s.TryGetMediaPropertiesAsync().map_err(e)?.get().map_err(e)?;
    let t = s.GetTimelineProperties().map_err(e)?;
    let playing = s.GetPlaybackInfo().map_err(e)?.PlaybackStatus().map_err(e)? == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
    let title = p.Title().map_err(e)?.to_string();
    let artist = p.Artist().map_err(e)?.to_string();
    // Cover di-cache per lagu: membaca + base64 thumbnail tiap poll (2 dtk) itu mahal
    static COVER: std::sync::Mutex<(String, Option<String>)> = std::sync::Mutex::new((String::new(), None));
    let key = format!("{title}\u{1f}{artist}");
    let cover = {
        let mut c = COVER.lock().map_err(e)?;
        if c.0 != key || c.1.is_none() {
            *c = (key, get_thumbnail_base64(&p));
        }
        c.1.clone()
    };
    Ok(Some(Media {
        title,
        artist,
        playing,
        pos: t.Position().map_err(e)?.Duration / 10_000_000,
        dur: t.EndTime().map_err(e)?.Duration / 10_000_000,
        cover,
    }))
}
pub fn media_ctl(op: &str) -> R<()> {
    let Some(s) = sess()? else { return Err("tidak ada media".into()) };
    match op { "prev" => s.TrySkipPreviousAsync(), "next" => s.TrySkipNextAsync(), _ => s.TryTogglePlayPauseAsync() }.map_err(e)?.get().map_err(e)?;
    Ok(())
}
// ---- Radio (Wi-Fi, Bluetooth)
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

#[derive(Serialize)]
pub struct RadioInfo {
    pub wifi_on: bool,
    pub wifi_ssid: Option<String>,
    pub bt_on: bool,
    pub bt_name: Option<String>,
    pub bt_battery: Option<u8>,
}

fn radio(k: &str) -> R<Option<Radio>> {
    let kind = if k == "wifi" { RadioKind::WiFi } else { RadioKind::Bluetooth };
    for r in Radio::GetRadiosAsync().map_err(e)?.get().map_err(e)? { if r.Kind().map_err(e)? == kind { return Ok(Some(r)) } }
    Ok(None)
}

fn current_wifi_ssid() -> Option<String> {
    if let Ok(out) = ps("netsh wlan show interfaces") {
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SSID") && !trimmed.starts_with("SSID name") && !trimmed.starts_with("BSSID") {
                if let Some((_, val)) = trimmed.split_once(':') {
                    let ssid = val.trim().to_string();
                    if !ssid.is_empty() {
                        return Some(ssid);
                    }
                }
            }
        }
    }
    None
}

fn get_bt_connected() -> (Option<String>, Option<u8>) {
    let script = r#"
        $dev = Get-PnpDevice -InstanceId 'BTHENUM\DEV_*','BTHLE\DEV_*' -Status OK -ErrorAction SilentlyContinue | Where-Object {
            (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName '{83da6326-97a6-4088-9453-a1923f573b29} 15' -ErrorAction SilentlyContinue).Data -eq $true
        } | Select-Object -First 1
        if ($dev) {
            $mac = if ($dev.InstanceId -match 'DEV_([0-9A-Fa-f]{12})') { $Matches[1] } else { '' }
            $bat = $null
            if ($mac) {
                Get-PnpDevice -InstanceId ('*' + $mac + '*') -ErrorAction SilentlyContinue | ForEach-Object {
                    if ($null -eq $bat) {
                        $b = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName '{104ea319-6ee2-4701-bd47-8ddbf425bbe5} 2' -ErrorAction SilentlyContinue).Data
                        if ($null -ne $b) { $bat = [int]$b }
                    }
                }
            }
            $name = $dev.FriendlyName -replace ' Hands-Free.*','' -replace ' Avrcp.*','' -replace ' Bluetooth Device',''
            [PSCustomObject]@{ name = $name; battery = $bat } | ConvertTo-Json -Compress
        } else {
            '{}'
        }
    "#;
    if let Ok(out) = ps(script) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
            let name = v["name"].as_str().map(|s| s.to_string());
            let bat = v["battery"].as_u64().map(|b| b as u8);
            return (name, bat);
        }
    }
    (None, None)
}

pub fn radios() -> R<RadioInfo> {
    let on = |k: &str| -> R<bool> { Ok(radio(k)?.map(|r| r.State().map(|s| s == RadioState::On).unwrap_or(false)).unwrap_or(false)) };
    let w_on = on("wifi").unwrap_or(false);
    let b_on = on("bluetooth").unwrap_or(false);
    let (bt_name, bt_battery) = if b_on { get_bt_connected() } else { (None, None) };
    let wifi_ssid = if w_on { current_wifi_ssid() } else { None };
    Ok(RadioInfo {
        wifi_on: w_on,
        wifi_ssid,
        bt_on: b_on,
        bt_name,
        bt_battery,
    })
}
pub fn set_radio(k: &str, on: bool) -> R<()> {
    if Radio::RequestAccessAsync().map_err(e)?.get().map_err(e)? != RadioAccessStatus::Allowed { return Err("akses radio ditolak".into()) }
    let r = radio(k)?.ok_or("radio tidak ditemukan")?;
    r.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).map_err(e)?.get().map_err(e)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub mac: String,
}

pub fn bt_list() -> R<Vec<BtDevice>> {
    let script = r#"
        $list = @()
        Get-PnpDevice -InstanceId 'BTHENUM\DEV_*' -Status OK -ErrorAction SilentlyContinue | ForEach-Object {
            $conn = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName '{83da6326-97a6-4088-9453-a1923f573b29} 15' -ErrorAction SilentlyContinue).Data -eq $true
            $mac = if ($_.InstanceId -match 'DEV_([0-9A-Fa-f]{12})') { $Matches[1] } else { '' }
            $bat = $null
            if ($mac -and $conn) {
                Get-PnpDevice -InstanceId ('*' + $mac + '*') -ErrorAction SilentlyContinue | ForEach-Object {
                    if ($null -eq $bat) {
                        $b = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName '{104ea319-6ee2-4701-bd47-8ddbf425bbe5} 2' -ErrorAction SilentlyContinue).Data
                        if ($null -ne $b) { $bat = [int]$b }
                    }
                }
            }
            $n = $_.FriendlyName -replace ' Hands-Free.*','' -replace ' Avrcp.*','' -replace ' Bluetooth Device',''
            if ($n -notlike '*Enumerator*' -and $n -notlike '*Adapter*' -and $n -notlike '*Service*') {
                $list += [PSCustomObject]@{ name = $n; connected = $conn; battery = $bat; mac = $mac }
            }
        }
        $list | Sort-Object -Property @{Expression="connected"; Descending=$true}, @{Expression="name"; Descending=$false} | ConvertTo-Json -Compress
    "#;
    let out = ps(script)?;
    if out.is_empty() || out == "{}" || out == "[]" {
        return Ok(vec![]);
    }
    if let Ok(list) = serde_json::from_str::<Vec<BtDevice>>(&out) {
        return Ok(list);
    }
    if let Ok(single) = serde_json::from_str::<BtDevice>(&out) {
        return Ok(vec![single]);
    }
    Ok(vec![])
}

pub fn bt_connect(_name: &str) -> R<()> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/c", "start", "ms-settings:bluetooth"]);
    hide_console(&mut cmd);
    let _ = cmd.spawn();
    Ok(())
}
// ---- Wi-Fi (Native Win32 wlanapi)
use windows::Win32::NetworkManagement::WiFi::*;
use windows::Win32::Foundation::HANDLE;

pub fn wifi_list() -> R<Vec<Net>> {
    unsafe {
        let mut negotiated = 0;
        let mut handle = HANDLE::default();
        let status = WlanOpenHandle(2, None, &mut negotiated, &mut handle);
        if status != 0 {
            return Err(format!("WlanOpenHandle failed: {status}"));
        }

        let mut if_list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        let status = WlanEnumInterfaces(handle, None, &mut if_list_ptr);
        if status != 0 || if_list_ptr.is_null() {
            let _ = WlanCloseHandle(handle, None);
            return Err(format!("WlanEnumInterfaces failed: {status}"));
        }

        let if_list = &*if_list_ptr;
        let if_slice = std::slice::from_raw_parts(if_list.InterfaceInfo.as_ptr(), if_list.dwNumberOfItems as usize);
        let mut v: Vec<Net> = Vec::new();

        for info in if_slice {
            let guid = info.InterfaceGuid;

            // Trigger non-blocking hardware scan
            let _ = WlanScan(handle, &guid, None, None, None);

            let mut net_list_ptr: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            let status = WlanGetAvailableNetworkList(handle, &guid, 2, None, &mut net_list_ptr);
            if status == 0 && !net_list_ptr.is_null() {
                let net_list = &*net_list_ptr;
                let net_slice = std::slice::from_raw_parts(net_list.Network.as_ptr(), net_list.dwNumberOfItems as usize);
                for net in net_slice {
                    let len = (net.dot11Ssid.uSSIDLength as usize).min(32);
                    if len == 0 {
                        continue;
                    }
                    let ssid = String::from_utf8_lossy(&net.dot11Ssid.ucSSID[..len]).trim().to_string();
                    if ssid.is_empty() {
                        continue;
                    }
                    let connected = (net.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED) != 0;
                    let secure = net.bSecurityEnabled.as_bool();
                    let bars = match net.wlanSignalQuality {
                        0..=25 => 1,
                        26..=50 => 2,
                        51..=75 => 3,
                        _ => 4,
                    };

                    if let Some(existing) = v.iter_mut().find(|x| x.ssid == ssid) {
                        if connected {
                            existing.connected = true;
                        }
                        if bars > existing.bars {
                            existing.bars = bars;
                        }
                    } else {
                        v.push(Net { ssid, secure, bars, connected });
                    }
                }
                WlanFreeMemory(net_list_ptr as *const _);
            }
        }

        WlanFreeMemory(if_list_ptr as *const _);
        let _ = WlanCloseHandle(handle, None);

        v.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.bars.cmp(&a.bars)).then(a.ssid.cmp(&b.ssid)));
        Ok(v)
    }
}
pub fn wifi_connect(ssid: &str, pw: Option<String>) -> R<()> {
    if let Some(password) = pw {
        if !password.is_empty() {
            let temp_dir = std::env::temp_dir();
            let profile_path = temp_dir.join(format!("wlan_{}.xml", std::process::id()));
            let xml = format!(
                r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
    <name>{ssid}</name>
    <SSIDConfig>
        <SSID>
            <name>{ssid}</name>
        </SSID>
    </SSIDConfig>
    <connectionType>ESS</connectionType>
    <connectionMode>auto</connectionMode>
    <MSM>
        <security>
            <authEncryption>
                <authentication>WPA2PSK</authentication>
                <encryption>AES</encryption>
                <useOneX>false</useOneX>
            </authEncryption>
            <sharedKey>
                <keyType>passPhrase</keyType>
                <protected>false</protected>
                <keyMaterial>{password}</keyMaterial>
            </sharedKey>
        </security>
    </MSM>
</WLANProfile>"#
            );
            if std::fs::write(&profile_path, xml).is_ok() {
                let mut cmd = Command::new("netsh");
                cmd.args(["wlan", "add", "profile", &format!("filename={}", profile_path.to_string_lossy())]);
                hide_console(&mut cmd);
                let _ = cmd.output();
                let _ = std::fs::remove_file(profile_path);
            }
        }
    }

    let mut cmd = Command::new("netsh");
    cmd.args(["wlan", "connect", &format!("name={ssid}")]);
    hide_console(&mut cmd);
    let out = cmd.output().map_err(e)?;
    let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() || msg.contains("successfully") || msg.contains("berhasil") {
        Ok(())
    } else {
        let mut fallback = Command::new("cmd");
        fallback.args(["/c", "start", "ms-settings:network-wifi"]);
        hide_console(&mut fallback);
        let _ = fallback.spawn();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uji perangkat keras: butuh audio, layar, dan radio sungguhan. Jalankan manual dengan
    // `cargo test -- --ignored`, jangan di CI.
    #[test]
    #[ignore = "needs real audio, display, and radio hardware"]
    fn test_brightness_and_volume() {
        let (vol, bright) = levels().expect("levels should succeed");
        println!("Volume: {vol}, Brightness: {bright:?}");
        assert!(vol >= 0.0 && vol <= 1.0);
        if let Some(b) = bright {
            assert!(b <= 100);
            assert!(set_brightness(b).is_ok());
        }
    }

    #[test]
    #[ignore = "needs real radio hardware"]
    fn test_radios() {
        let r = radios().expect("radios should succeed");
        println!("WiFi on: {}, ssid: {:?}, BT on: {}, name: {:?}, battery: {:?}", r.wifi_on, r.wifi_ssid, r.bt_on, r.bt_name, r.bt_battery);
    }

    #[test]
    #[ignore = "needs real Bluetooth hardware"]
    fn test_bt_list() {
        let list = bt_list().expect("bt_list should succeed");
        println!("Paired BT devices: {}", list.len());
        for d in list {
            println!("- {} (connected: {}, battery: {:?}, mac: {})", d.name, d.connected, d.battery, d.mac);
        }
    }

    #[test]
    #[ignore = "needs a real Wi-Fi adapter"]
    fn test_wifi_list() {
        match wifi_list() {
            Ok(list) => {
                println!("WiFi networks found: {}", list.len());
                for n in list {
                    println!("- {} (bars: {}, secure: {}, connected: {})", n.ssid, n.bars, n.secure, n.connected);
                }
            }
            Err(err) => {
                println!("wifi_list error: {}", err);
                panic!("wifi_list failed: {}", err);
            }
        }
    }
}
