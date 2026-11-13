// WASAPI loopback: tangkap output audio sistem → analisis Goertzel → event Tauri `spectrum`.
// Payload: [f32; 52] amplitudo band frekuensi [0.0 .. 1.0], ~60fps.
#![allow(non_snake_case)]

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
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

static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) { return; }
    std::thread::spawn(move || {
        if let Err(e) = unsafe { run(&app) } {
            eprintln!("[spectrum] {e}");
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn stop() { RUNNING.store(false, Ordering::SeqCst); }

unsafe fn run(app: &AppHandle) -> windows::core::Result<()> {
    // Abaikan RPC_E_CHANGED_MODE (0x80010106) — COM sudah init di thread ini
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    let fmt = client.GetMixFormat()?;
    let sr  = (*fmt).nSamplesPerSec as f64;
    let ch  = (*fmt).nChannels as usize;
    // wFormatTag 3 = IEEE_FLOAT, 0xFFFE = EXTENSIBLE (di WASAPI shared selalu f32)
    let is_f32 = (*fmt).wBitsPerSample == 32
        && ((*fmt).wFormatTag == 3 || (*fmt).wFormatTag == 0xFFFE);

    // Buffer default (hnsDuration = 0) supaya latensi minimal
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, LOOPBACK, 0, 0, fmt, None)?;
    CoTaskMemFree(Some(fmt as *const _ as *const std::ffi::c_void));

    if !is_f32 {
        eprintln!("[spectrum] format audio bukan float32, spektrum dinonaktifkan");
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

    let mut ring  = vec![0.0f32; WIN];
    let mut wp    = 0usize;
    let mut fill  = 0usize;
    let mut smooth = [0.0f32; BARS];

    // Level referensi: Goertzel pada sinus full-scale + Hann window ≈ WIN/4
    let ref_amp = WIN as f32 / 4.0;

    while RUNNING.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(14));

        // Kuras buffer audio engine
        loop {
            let avail = capture.GetNextPacketSize()?;
            if avail == 0 { break; }

            let mut ptr     = std::ptr::null_mut::<u8>();
            let mut nframes = 0u32;
            let mut flags   = 0u32;
            capture.GetBuffer(&mut ptr, &mut nframes, &mut flags, None, None)?;

            if nframes > 0 && !ptr.is_null() {
                let samples =
                    std::slice::from_raw_parts(ptr as *const f32, nframes as usize * ch);
                for f in 0..nframes as usize {
                    let mut mono = 0.0f32;
                    for c in 0..ch { mono += samples[f * ch + c]; }
                    ring[wp % WIN] = mono / ch as f32;
                    wp = wp.wrapping_add(1);
                    if fill < WIN { fill += 1; }
                }
            }
            capture.ReleaseBuffer(nframes)?;
        }

        if fill < WIN { continue; }

        // Window analisis (WIN sampel terbaru + Hann)
        let base = wp.wrapping_sub(WIN);
        let mut win = [0.0f32; WIN];
        for i in 0..WIN {
            let hann = (0.5 * (1.0 - (TAU * i as f64 / (WIN - 1) as f64).cos())) as f32;
            win[i] = ring[(base + i) % WIN] * hann;
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

            // Konversi ke tampilan: kompres logaritmik agar sinyal lemah tetap terlihat
            // amp=1.0 → 1.0, amp=0.1 → 0.67, amp=0.01 → 0.33, amp<1e-9 → 0
            let v = if amp > 1e-9 {
                (1.0f32 + amp.log10() / 3.0).clamp(0.0, 1.0)
            } else { 0.0 };

            // Attack cepat, release lambat — seperti VU meter nyata
            let spd = if v > smooth[b] { 0.55 } else { 0.12 };
            smooth[b] += (v - smooth[b]) * spd;
        }

        let _ = app.emit("spectrum", smooth.as_ref());
    }

    let _ = client.Stop();
    Ok(())
}
