# Perbaikan Error Kompilasi WiFiAdapter WinRT

> Date: 2026-11-03
> Status: verified
> Change type: fix
> Scope: src-tauri/src/sys.rs

## Outcome

Proses `cargo check` dan `npm run dev` (`tauri dev`) sekarang berhasil mengompilasi biner tanpa error E0599 dan aplikasi Nimbus (`nimbus.exe`, sebelumnya `windows-notch.exe`) berhasil berjalan normal di Windows.

## Implementation

Pada [src-tauri/src/sys.rs](../../../src-tauri/src/sys.rs#L70-L74), pemanggilan fungsi WinRT `WiFiAdapter::FindAllAsync()` diperbaiki menjadi `WiFiAdapter::FindAllAdaptersAsync()` sesuai definisi API WinRT pada crate `windows` 0.58.

## Decisions and tradeoffs

Menggunakan nama metode resmi WinRT `FindAllAdaptersAsync` yang diekspos oleh crate `windows` 0.58 untuk `Windows::Devices::WiFi::WiFiAdapter`.

## Verification

- `cargo check` di `src-tauri` berhasil dengan exit code 0.
- Proses `tauri dev` / `cargo run` berhasil melakukan build dan menjalankan proses target Nimbus (PID terverifikasi aktif).

## Limitations and follow-up

- Pengujian fungsional runtime (koneksi Wi-Fi aktual, status izin `WiFiAdapter::RequestAccessAsync` pada un-packaged desktop app) perlu diuji lebih lanjut saat penggunaan aktif.

## Related documentation

- [HANDOFF.md](../../HANDOFF.md)
