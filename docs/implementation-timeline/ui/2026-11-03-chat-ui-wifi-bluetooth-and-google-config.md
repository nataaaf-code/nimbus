# Perbaikan Chat UI Agent, Status Wi-Fi & Bluetooth, dan Form Config Google Calendar

> Date: 2026-11-03
> Status: verified
> Change type: feature
> Scope: ui/index.html, src-tauri/src/sys.rs, src-tauri/src/google.rs, src-tauri/src/main.rs

## Outcome

1. Chatting UI pada AI Agent kini memiliki bubble rapi (membedakan pesan user, balasan agent, dan log proses), log auto-scroll dengan scrollbar elegan, dan chip indikator usage stats/limit token langsung dari service OpenCode.
2. Tombol Wi-Fi kini menampilkan nama SSID jaringan yang aktif tersambung (bukan sekadar "Nyala/Mati"), dan tombol Bluetooth menampilkan nama perangkat yang terhubung beserta persentase baterainya (misal `HAYLOU HQ3 • 60%`).
3. Kalender Google kini menangani ketiadaan `%APPDATA%\WindowsNotch\config.json` dengan menyediakan form input Client ID & Client Secret langsung di antarmuka tab, tombol "Simpan & Login", dan tombol pintasan "Buka Folder Config" tanpa error manual.

## Implementation

- **AI Agent Chat & Usage**:
  - `ui/index.html`: Merombak layout `#lg` menjadi scrollable flex container dengan message bubble `.log-msg.user`, `.log-msg.agent`, dan `.log-msg.system`.
  - Mengalirkan token stream dan menambahkan auto-scroll ke bawah saat menerima output baru.
  - Menambahkan command Tauri `agent_usage` yang memanggil OpenCode stats API dan menampilkannya di chip header.
- **Wi-Fi & Bluetooth Info**:
  - `src-tauri/src/sys.rs`: Mengembalikan struct `RadioInfo` yang mencakup `wifi_ssid`, `bt_name`, dan `bt_battery`.
  - Mendeteksi nama SSID yang terhubung dari antarmuka adapter Windows dan query baterai perangkat Bluetooth melalui property DEVPKEY Windows.
- **Google Calendar Config Form**:
  - `src-tauri/src/google.rs`: Menyediakan `status()` dengan deteksi ketersediaan kredensial, `save_creds()` untuk menyimpan JSON, dan `open_config_folder()` untuk membuka folder AppData via Windows Explorer.
  - `ui/index.html`: Menampilkan form konfigurasi inline jika kredensial belum ada, sehingga pengguna tidak perlu membuat file JSON secara manual.

## Verification

- `cargo test -- --nocapture` berhasil dengan 2 test lulus:
  - `sys::tests::test_brightness_and_volume ... ok`
  - `sys::tests::test_radios ... ok` (terverifikasi membaca SSID Wi-Fi dan baterai Bluetooth).
- Aplikasi `windows-notch.exe` aktif berjalan.
