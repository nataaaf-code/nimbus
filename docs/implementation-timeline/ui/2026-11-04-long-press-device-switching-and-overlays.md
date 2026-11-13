# Long-Press Quick Device Switching & Overlays (Wi-Fi & Bluetooth)

**Tanggal**: 2026-11-04  
**Domain**: ui / sys  
**Status**: Selesai & Terverifikasi  

## Latar Belakang & Kebutuhan
Pengguna membutuhkan kemudahan mengganti perangkat Wi-Fi dan Bluetooth langsung dari Notch tanpa harus selalu membuka pengaturan Windows secara terpisah:
- **Klik biasa (< 400ms)**: Menyalakan/mematikan radio Wi-Fi atau Bluetooth secara instan.
- **Klik tahan / Long-press (>= 400ms)**: Menampilkan popup/overlay daftar perangkat Bluetooth yang terpasang atau jaringan Wi-Fi yang tersedia, lengkap dengan status koneksi, persentase baterai, dan opsi sambung.

## Perubahan yang Dilakukan

### 1. Backend Rust (`src-tauri/src/sys.rs` & `src-tauri/src/main.rs`)
- **Struktur `BtDevice`**:
  - `name`: Nama friendly perangkat Bluetooth.
  - `connected`: Boolean status aktif dari properti PnP `{83da6326-97a6-4088-9453-a1923f573b29} 15`.
  - `battery`: Persentase baterai dari key `{104ea319-6ee2-4701-bd47-8ddbf425bbe5} 2`.
  - `mac`: Alamat MAC perangkat.
- **Fungsi `bt_list()`**: Query PnP device terhubung dan tersambung dengan pengurutan (perangkat terhubung berada di paling atas).
- **Fungsi `bt_connect(name)`**: Menginisiasi koneksi atau navigasi ke `ms-settings:bluetooth`.
- **Fungsi `open_settings(uri)`**: Membuka protokol URI Windows Settings (misal `ms-settings:network-wifi` atau `ms-settings:bluetooth`).
- **Eksposur Perintah Tauri**: Mendaftarkan `bt_list`, `bt_connect`, dan `open_settings` pada `tauri::generate_handler!`.

### 2. Antarmuka Frontend (`ui/index.html`)
- **Long-Press Detection**:
  - Implementasi fungsi `makeLongPress(el, onShortClick, onLongPress, duration = 400)`.
  - Dilengkapi pencegahan pemicuan jika kursor bergerak lebih dari 8 piksel (membatalkan gesture saat drag/klik lepas).
  - Feedback visual taktil `.tile.held` (`transform: scale(0.94)` dan aksen outline Coral).
- **Bluetooth Device Overlay (`#blo`)**:
  - Container `.wl#blo` yang menampilkan daftar perangkat Bluetooth terpasang.
  - Indikator status (Tersambung / Siap disambung), ikon perangkat, dan badge baterai (`🔋 XX%`).
  - Tombol pintasan langsung "Buka Pengaturan Bluetooth Windows".
- **Wi-Fi Overlay Enhancement (`#wlo`)**:
  - Penambahan tombol pintasan langsung "Buka Pengaturan Wi-Fi Windows".
- **Cleanup State**:
  - Otomatis menutup overlay ketika tab diganti atau notch tertutup.

## Verifikasi
- `cargo test --manifest-path src-tauri/Cargo.toml`: 5 unit test lulus (termasuk `test_bt_list`, `test_radios`, `test_brightness_and_volume`, `test_browser_check`, `test_clean_ansi`).
- `cargo build --manifest-path src-tauri/Cargo.toml`: Berhasil membangun binary debug tanpa error.
