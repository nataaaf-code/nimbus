// WASAPI loopback: tangkap output audio sistem → analisis Goertzel → event Tauri `spectrum`.
// Payload: [f32; 52] amplitudo band frekuensi [0.0 .. 1.0], ~60fps.
#![allow(non_snake_case)]

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

pub const BARS: usize = 52;
const F_MIN: f64 = 20.0;
const F_MAX: f64 = 18_000.0;
const WIN: usize = 2048;
// AUDCLNT_STREAMFLAGS_LOOPBACK = 0x00020000
const LOOPBACK: u32 = 0x0002_0000;
const BUFFERFLAGS_SILENT: u32 = 0x2;

static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) { return; }
    std::thread::spawn(move || {
        // Abaikan RPC_E_CHANGED_MODE — COM sudah init di thread ini
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // Pulih otomatis: perangkat dicabut/diganti, audio engine restart, dll.
        while RUNNING.load(Ordering::Relaxed) {
            if let Err(e) = unsafe { run(&app) } {
                eprintln!("[spectrum] {e} — mencoba ulang");
                let _ = app.emit("spectrum", [0.0f32; BARS].as_ref());
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

pub fn stop() { RUNNING.store(false, Ordering::SeqCst); }

unsafe fn default_device_id(en: &IMMDeviceEnumerator) -> Option<String> {
    let d = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
    let p = d.GetId().ok()?;
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
    s
}

/// Satu sesi capture. Kembali Ok(()) bila perangkat default berganti (pemanggil akan init ulang).
unsafe fn run(app: &AppHandle) -> windows::core::Result<()> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let dev_id = default_device_id(&enumerator);
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    let fmt = client.GetMixFormat()?;
    let sr  = (*fmt).nSamplesPerSec as f64;
    let ch  = ((*fmt).nChannels as usize).max(1);
    // wFormatTag 3 = IEEE_FLOAT, 0xFFFE = EXTENSIBLE (di WASAPI shared selalu f32)
    let is_f32 = (*fmt).wBitsPerSample == 32
        && ((*fmt).wFormatTag == 3 || (*fmt).wFormatTag == 0xFFFE);

    let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, LOOPBACK, 0, 0, fmt, None);
    CoTaskMemFree(Some(fmt as *const _ as *const std::ffi::c_void));
    init?;

    if !is_f32 {
        eprintln!("[spectrum] format audio bukan float32, spektrum dinonaktifkan");
        std::thread::sleep(Duration::from_secs(5));
        return Ok(());
    }

    let capture: IAudioCaptureClient = client.GetService()?;
    client.Start()?;

    // Koefisien Goertzel untuk BARS frekuensi log-spaced
    let coeffs: Vec<f32> = (0..BARS)
        .map(|i| {
            let f = F_MIN * (F_MAX / F_MIN).powf(i as f64 / (BARS - 1) as f64);
            (2.0 * (TAU * f / sr).cos()) as f32
        })
        .collect();
    let hann: Vec<f32> = (0..WIN)
        .map(|i| (0.5 * (1.0 - (TAU * i as f64 / (WIN - 1) as f64).cos())) as f32)
        .collect();

    let mut ring  = vec![0.0f32; WIN];
    let mut wp    = 0usize;
    let mut fill  = 0usize;
    let mut smooth = [0.0f32; BARS];
    let mut win   = vec![0.0f32; WIN];
    let mut idle_sent = false;

    // Level referensi: Goertzel pada sinus full-scale + Hann window ≈ WIN/4
    let ref_amp = WIN as f32 / 4.0;
    let tick = Duration::from_millis(14);
    // Saat tidak ada yang diputar, loopback berhenti mengirim paket sama sekali.
    // Isi ring dengan senyap supaya bar turun, bukan membeku di frame terakhir.
    let silence_per_tick = (sr * tick.as_secs_f64()).round() as usize;
    let mut last_pkt = Instant::now();
    let mut last_dev_check = Instant::now();

    let push = |v: f32, ring: &mut Vec<f32>, wp: &mut usize, fill: &mut usize| {
        ring[*wp % WIN] = v;
        *wp = wp.wrapping_add(1);
        if *fill < WIN { *fill += 1; }
    };

    let result = (|| -> windows::core::Result<()> {
        while RUNNING.load(Ordering::Relaxed) {
            std::thread::sleep(tick);

            if last_dev_check.elapsed() > Duration::from_secs(2) {
                last_dev_check = Instant::now();
                if default_device_id(&enumerator) != dev_id { return Ok(()); }
            }

            // Kuras buffer audio engine
            let mut got = false;
            loop {
                let avail = capture.GetNextPacketSize()?;
                if avail == 0 { break; }

                let mut ptr     = std::ptr::null_mut::<u8>();
                let mut nframes = 0u32;
                let mut flags   = 0u32;
                capture.GetBuffer(&mut ptr, &mut nframes, &mut flags, None, None)?;

                if nframes > 0 {
                    got = true;
                    if flags & BUFFERFLAGS_SILENT != 0 || ptr.is_null() {
                        for _ in 0..nframes { push(0.0, &mut ring, &mut wp, &mut fill); }
                    } else {
                        let samples =
                            std::slice::from_raw_parts(ptr as *const f32, nframes as usize * ch);
                        for f in 0..nframes as usize {
                            let mut mono = 0.0f32;
                            for c in 0..ch { mono += samples[f * ch + c]; }
                            push(mono / ch as f32, &mut ring, &mut wp, &mut fill);
                        }
                    }
                }
                capture.ReleaseBuffer(nframes)?;
            }

            if got {
                last_pkt = Instant::now();
            } else if last_pkt.elapsed() > Duration::from_millis(120) {
                for _ in 0..silence_per_tick { push(0.0, &mut ring, &mut wp, &mut fill); }
            }

            if fill < WIN { continue; }

            // Window analisis (WIN sampel terbaru + Hann)
            let base = wp.wrapping_sub(WIN);
            for i in 0..WIN {
                win[i] = ring[(base + i) % WIN] * hann[i];
            }

            // Goertzel per bar frekuensi
            for (b, &c) in coeffs.iter().enumerate() {
                let (mut s1, mut s2) = (0.0f32, 0.0f32);
                for &x in win.iter() {
                    let s0 = x + c * s1 - s2;
                    s2 = s1; s1 = s0;
                }
                // Magnitude DFT ternormalisasi terhadap sinus full-scale
                let p   = (s1 * s1 + s2 * s2 - c * s1 * s2).abs();
                let amp = p.sqrt() / ref_amp;

                // Kompres logaritmik agar sinyal lemah tetap terlihat
                // amp=1.0 → 1.0, amp=0.1 → 0.67, amp=0.01 → 0.33, amp≤0.001 → 0
                let v = if amp > 1e-9 {
                    (1.0f32 + amp.log10() / 3.0).clamp(0.0, 1.0)
                } else { 0.0 };

                // Attack cepat, release lambat — seperti VU meter nyata
                let spd = if v > smooth[b] { 0.55 } else { 0.12 };
                smooth[b] += (v - smooth[b]) * spd;
            }

            // Hemat IPC: saat senyap kirim satu frame nol saja, lalu diam
            let silent = smooth.iter().all(|&v| v < 0.002);
            if silent {
                if idle_sent { continue; }
                smooth = [0.0; BARS];
                idle_sent = true;
            } else {
                idle_sent = false;
            }
            let _ = app.emit("spectrum", smooth.as_ref());
        }
        Ok(())
    })();

    let _ = client.Stop();
    result
}
