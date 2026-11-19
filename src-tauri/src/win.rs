//! FFI Win32 langsung: kursor, deteksi browser di bawah notch, snapshot proses, CPU/RAM,
//! pemilik koneksi TCP (untuk memetakan hook HTTP -> PID claude), dan kill pohon proses.
use std::ffi::c_void;
use std::process::Command;

type H = *mut c_void;
const INVALID: isize = -1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point { pub x: i32, pub y: i32 }

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileTime { lo: u32, hi: u32 }
impl FileTime { pub fn v(&self) -> u64 { ((self.hi as u64) << 32) | self.lo as u64 } }

#[repr(C)]
struct ProcessEntry32W {
    size: u32, usage: u32, pid: u32, heap: usize, module: u32, threads: u32,
    ppid: u32, pri: i32, flags: u32, exe: [u16; 260],
}

#[repr(C)]
#[derive(Default)]
struct MemCounters {
    cb: u32, faults: u32, peak_ws: usize, ws: usize, qpp: usize, qp: usize,
    qpnp: usize, qnp: usize, pagefile: usize, peak_pagefile: usize,
}

#[repr(C)]
#[derive(Default)]
struct MemStatusEx {
    len: u32, load: u32, total: u64, avail: u64, total_pf: u64, avail_pf: u64,
    total_v: u64, avail_v: u64, avail_ext: u64,
}

#[link(name = "user32")]
extern "system" {
    fn GetCursorPos(p: *mut Point) -> i32;
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn GetTopWindow(h: H) -> H;
    fn GetWindow(h: H, cmd: u32) -> H;
    fn IsWindowVisible(h: H) -> i32;
    fn IsIconic(h: H) -> i32;
    fn GetWindowRect(h: H, r: *mut Rect) -> i32;
    fn GetClassNameW(h: H, buf: *mut u16, n: i32) -> i32;
    fn GetWindowThreadProcessId(h: H, pid: *mut u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> H;
    fn Process32FirstW(h: H, e: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(h: H, e: *mut ProcessEntry32W) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
    fn CloseHandle(h: H) -> i32;
    fn GetProcessTimes(h: H, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
    fn K32GetProcessMemoryInfo(h: H, p: *mut MemCounters, cb: u32) -> i32;
    fn QueryFullProcessImageNameW(h: H, flags: u32, buf: *mut u16, size: *mut u32) -> i32;
    fn GetExitCodeProcess(h: H, code: *mut u32) -> i32;
    fn GetSystemTimes(idle: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
    fn GlobalMemoryStatusEx(p: *mut MemStatusEx) -> i32;
}

#[link(name = "iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(t: *mut c_void, size: *mut u32, order: i32, af: u32, class: u32, reserved: u32) -> u32;
}

#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: H,
    h_instance: H,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: *const c_void,
    lp_template_name: *const u16,
    pv_reserved: *mut c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[repr(C)]
struct BrowseInfoW {
    hwnd_owner: H,
    pidl_root: *const c_void,
    psz_display_name: *mut u16,
    lpsz_title: *const u16,
    ul_flags: u32,
    lpfn: *const c_void,
    l_param: isize,
    i_image: i32,
}

#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(bi: *mut BrowseInfoW) -> *mut c_void;
    fn SHGetPathFromIDListW(pidl: *const c_void, path: *mut u16) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoTaskMemFree(pv: *mut c_void);
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Dialog pilih file native Windows (bisa multi-select).
pub fn pick_files(owner_hwnd: isize) -> Vec<String> {
    let filter: Vec<u16> = "Semua file (*.*)\0*.*\0Gambar & Dokumen\0*.png;*.jpg;*.jpeg;*.webp;*.gif;*.pdf;*.txt;*.md;*.json;*.rs;*.js;*.ts;*.py\0\0"
        .encode_utf16()
        .collect();
    let title = wide_z("Pilih lampiran");
    let mut buf = vec![0u16; 32768];
    let mut ofn = OpenFileNameW {
        l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
        hwnd_owner: owner_hwnd as H,
        h_instance: std::ptr::null_mut(),
        lpstr_filter: filter.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: buf.as_mut_ptr(),
        n_max_file: buf.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: std::ptr::null(),
        lpstr_title: title.as_ptr(),
        flags: 0x0008_0000 | 0x0000_0200 | 0x0000_1000 | 0x0000_0800 | 0x0000_0008,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: std::ptr::null(),
        l_cust_data: 0,
        lpfn_hook: std::ptr::null(),
        lp_template_name: std::ptr::null(),
        pv_reserved: std::ptr::null_mut(),
        dw_reserved: 0,
        flags_ex: 0,
    };
    let ok = unsafe { GetOpenFileNameW(&mut ofn) != 0 };
    if !ok { return Vec::new(); }

    let mut parts = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        let end = buf[i..].iter().position(|&c| c == 0).unwrap_or(0);
        if end == 0 { break; }
        parts.push(String::from_utf16_lossy(&buf[i..i + end]));
        i += end + 1;
    }
    match parts.len() {
        0 => Vec::new(),
        1 => parts,
        _ => {
            let dir = &parts[0];
            parts[1..].iter().map(|f| std::path::Path::new(dir).join(f).to_string_lossy().to_string()).collect()
        }
    }
}

/// Dialog pilih folder native Windows.
pub fn pick_folder(owner_hwnd: isize, title_str: &str) -> Option<String> {
    let title = wide_z(title_str);
    let mut display = [0u16; 260];
    let mut path_buf = [0u16; 1024];
    unsafe {
        let hr = CoInitializeEx(std::ptr::null_mut(), 0x2); // COINIT_APARTMENTTHREADED
        let mut bi = BrowseInfoW {
            hwnd_owner: owner_hwnd as H,
            pidl_root: std::ptr::null(),
            psz_display_name: display.as_mut_ptr(),
            lpsz_title: title.as_ptr(),
            ul_flags: 0x0001 | 0x0040 | 0x0010, // BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX
            lpfn: std::ptr::null(),
            l_param: 0,
            i_image: 0,
        };
        let pidl = SHBrowseForFolderW(&mut bi);
        let res = if !pidl.is_null() {
            let ok = SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) != 0;
            CoTaskMemFree(pidl);
            if ok {
                let len = path_buf.iter().position(|&c| c == 0).unwrap_or(path_buf.len());
                Some(String::from_utf16_lossy(&path_buf[..len]))
            } else { None }
        } else { None };
        if hr >= 0 { CoUninitialize(); }
        res
    }
}

// ---------- input ----------
pub fn cursor() -> Point { let mut p = Point::default(); unsafe { GetCursorPos(&mut p) }; p }
pub fn key_down(vk: i32) -> bool { unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 } }

// ---------- proses ----------
pub struct Proc { pub pid: u32, pub ppid: u32, pub name: String }

pub fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(0x2, 0);
        if snap as isize == INVALID || snap.is_null() { return out; }
        let mut e: ProcessEntry32W = std::mem::zeroed();
        e.size = std::mem::size_of::<ProcessEntry32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e.exe.iter().position(|&c| c == 0).unwrap_or(260);
                out.push(Proc { pid: e.pid, ppid: e.ppid, name: String::from_utf16_lossy(&e.exe[..len]) });
                if Process32NextW(snap, &mut e) == 0 { break; }
            }
        }
        CloseHandle(snap);
    }
    out
}

pub struct ProcInfo { pub path: String, pub created_ms: i64, pub cpu_100ns: u64, pub mem: u64 }

pub fn filetime_ms(v: u64) -> i64 { (v as i64 - 116_444_736_000_000_000) / 10_000 }

pub fn proc_info(pid: u32) -> Option<ProcInfo> {
    unsafe {
        let h = OpenProcess(0x1000 | 0x0010, 0, pid);
        let h = if h.is_null() { OpenProcess(0x1000, 0, pid) } else { h };
        if h.is_null() { return None; }
        let mut buf = [0u16; 520];
        let mut size = buf.len() as u32;
        let path = if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size) != 0 {
            String::from_utf16_lossy(&buf[..size as usize])
        } else { String::new() };
        let (mut c, mut x, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        let ok = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0;
        let mut m = MemCounters { cb: std::mem::size_of::<MemCounters>() as u32, ..Default::default() };
        let mem = if K32GetProcessMemoryInfo(h, &mut m, m.cb) != 0 { m.ws as u64 } else { 0 };
        CloseHandle(h);
        Some(ProcInfo {
            path,
            created_ms: if ok { filetime_ms(c.v()) } else { 0 },
            cpu_100ns: if ok { k.v() + u.v() } else { 0 },
            mem,
        })
    }
}

pub fn alive(pid: u32) -> bool {
    if pid == 0 { return false; }
    unsafe {
        let h = OpenProcess(0x1000, 0, pid);
        if h.is_null() { return false; }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == 259 // STILL_ACTIVE
    }
}

/// Matikan proses beserta seluruh turunannya.
pub fn kill_tree(pid: u32) -> Result<(), String> {
    let mut c = Command::new("taskkill");
    c.args(["/PID", &pid.to_string(), "/T", "/F"]);
    crate::sys::hide_console(&mut c);
    let o = c.output().map_err(|e| e.to_string())?;
    if o.status.success() || !alive(pid) { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

/// PID pemilik koneksi TCP IPv4 dari sisi klien (port lokal klien -> port server kita).
pub fn tcp_owner(client_port: u16, server_port: u16) -> Option<u32> {
    unsafe {
        let mut size = 0u32;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, 2, 5, 0);
        if size == 0 { return None; }
        let mut buf = vec![0u32; (size as usize / 4) + 8];
        size = (buf.len() * 4) as u32;
        if GetExtendedTcpTable(buf.as_mut_ptr() as *mut c_void, &mut size, 0, 2, 5, 0) != 0 { return None; }
        let n = buf[0] as usize;
        let port = |v: u32| (((v & 0xff) << 8) | ((v >> 8) & 0xff)) as u16;
        for i in 0..n {
            let r = &buf[1 + i * 6..1 + i * 6 + 6];
            if port(r[2]) == client_port && port(r[4]) == server_port { return Some(r[5]); }
        }
        None
    }
}

// ---------- sistem ----------
pub fn system_times() -> (u64, u64, u64) {
    let (mut i, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default());
    unsafe { GetSystemTimes(&mut i, &mut k, &mut u) };
    (i.v(), k.v(), u.v())
}
pub fn memory() -> (u64, u64) {
    let mut m = MemStatusEx { len: std::mem::size_of::<MemStatusEx>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut m) };
    (m.total.saturating_sub(m.avail), m.total)
}

// ---------- browser di bawah notch ----------
fn class_of(h: H) -> String {
    let mut b = [0u16; 128];
    let n = unsafe { GetClassNameW(h, b.as_mut_ptr(), 128) };
    if n > 0 { String::from_utf16_lossy(&b[..n as usize]) } else { String::new() }
}
fn is_browser_window(h: H) -> bool {
    let c = class_of(h);
    if c == "Chrome_WidgetWin_1" || c == "MozillaWindowClass" {
        // Chrome_WidgetWin_1 juga dipakai VS Code/Electron; cek nama exe.
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        if let Some(i) = proc_info(pid) {
            let p = i.path.to_lowercase();
            return ["chrome.exe", "msedge.exe", "brave.exe", "firefox.exe", "opera.exe", "vivaldi.exe", "arc.exe", "zen.exe"]
                .iter().any(|b| p.ends_with(b));
        }
        return c == "MozillaWindowClass";
    }
    false
}
/// True bila jendela teratas yang menutupi tengah-atas layar adalah browser (tab bar akan tertutup notch).
pub fn browser_under(center_x: i32) -> bool {
    let me = std::process::id();
    unsafe {
        let mut cur = GetTopWindow(std::ptr::null_mut());
        let mut n = 0;
        while !cur.is_null() && n < 80 {
            n += 1;
            let mut pid = 0u32;
            GetWindowThreadProcessId(cur, &mut pid);
            if pid != me && IsWindowVisible(cur) != 0 && IsIconic(cur) == 0 {
                let mut r = Rect::default();
                if GetWindowRect(cur, &mut r) != 0 {
                    let (w, h) = (r.right - r.left, r.bottom - r.top);
                    if w > 300 && h > 200 && r.top <= 15 && r.bottom >= 150 && center_x >= r.left && center_x <= r.right {
                        let c = class_of(cur);
                        if !matches!(c.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                            return is_browser_window(cur);
                        }
                    }
                }
            }
            cur = GetWindow(cur, 2);
        }
    }
    false
}
