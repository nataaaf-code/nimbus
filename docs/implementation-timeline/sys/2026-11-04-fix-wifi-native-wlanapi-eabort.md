# Perbaikan Error Wi-Fi Operation Aborted (0x80004004) dengan Native Win32 wlanapi

**Tanggal**: 2026-11-04  
**Domain**: sys  
**Status**: Selesai & Terverifikasi  

## Gejala & Akar Masalah
- **Gejala**: Ketika user membuka daftar Wi-Fi (klik tahan pada tile Wi-Fi), muncul pesan error `Operation aborted (0x80004004)`.
- **Akar Masalah**:
  - Implementasi sebelumnya menggunakan UWP/WinRT `Windows.Devices.WiFi.WiFiAdapter::ScanAsync()`.
  - Pada arsitektur Windows 10/11, API `WiFiAdapter::ScanAsync()` dan `ConnectAsync()` didesain khusus untuk aplikasi UWP/packaged (AppX/MSIX) dan mensyaratkan capability restricted `wiFiControl` serta izin lokasi foreground.
  - Untuk aplikasi desktop Win32 standar / unpackaged (seperti Tauri), `ScanAsync().get()` akan tertahan selama ~20 detik sebelum dibatalkan oleh kernel Windows dengan kode COM `0x80004004` (`E_ABORT`).

## Solusi & Perbaikan

1. **Migrasi ke Native Win32 `wlanapi.dll`**:
   - Menambahkan feature `"Win32_NetworkManagement_WiFi"` pada crate `windows` (v0.58).
   - Menggunakan fungsi native C Windows:
     - `WlanOpenHandle` & `WlanCloseHandle`
     - `WlanEnumInterfaces`
     - `WlanScan` (asynchronous, non-blocking hardware scan)
     - `WlanGetAvailableNetworkList`
     - `WlanFreeMemory`
   - Menggunakan `std::slice::from_raw_parts` untuk memetakan struct C flexible array member (`InterfaceInfo` & `Network`).
   - Membaca langsung flag bitwise `WLAN_AVAILABLE_NETWORK_CONNECTED` (`0x00000001`) untuk menentukan apakah sebuah SSID sedang terhubung secara aktif.
   - Mengonversi `wlanSignalQuality` (0–100%) menjadi 1–4 bar sinyal secara presisi.

2. **Perbaikan `wifi_connect`**:
   - Jika jaringan sudah memiliki profil yang tersimpan: langsung memanggil `netsh wlan connect name="<SSID>"`.
   - Jika user memberikan password baru: membuat profil WLAN XML sementara, mendaftarkannya dengan `netsh wlan add profile`, lalu melakukan koneksi.
   - Jika gagal atau butuh interaksi kredensial khusus, fallback membuka `ms-settings:network-wifi`.

## Hasil & Verifikasi
- Pengujian unit test `test_wifi_list`:
  - **Sebelum**: Gagal setelah ~20 detik dengan panic `Operation aborted (0x80004004)`.
  - **Sesudah**: Sukses dalam **0.19 detik** mendeteksi seluruh jaringan Wi-Fi di sekitar (12 SSID terdeteksi termasuk jaringan yang sedang terkoneksi aktif).
- Pengujian regresi penuh: 6 unit tests Rust berhasil 100% tanpa error.
- Binary rilis Nimbus (`nimbus.exe`, sebelumnya `windows-notch.exe`) berhasil dikompilasi dan berjalan aktif.
