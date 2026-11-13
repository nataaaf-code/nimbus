# Usage Stats OpenCode Realistis & Mudah Dibaca

**Tanggal**: 2026-11-04  
**Domain**: ui / sys  
**Status**: Terverifikasi (cargo check + CLI stats + render mock)

## Latar Belakang
Chip usage sebelumnya memakai angka karangan: `Sisa Kuota: NN%` dihitung dari `steps / 500`, dan teks idle masih hardcoded `opencode 62%`. OpenCode tidak menyediakan kuota persentase seperti itu, sehingga data terlihat tidak realistis.

## Perubahan

### Backend (`src-tauri/src/main.rs`)
- Menambahkan helper `opencode_command()` agar `agent_send` dan `agent_usage` memakai deteksi biner yang sama (`NOTCH_OPENCODE_BIN`, instalasi Node.js, atau `opencode.cmd` di PATH).
- `agent_usage` sekarang menjalankan `opencode stats --json --days 0` (statistik hari ini), bukan endpoint `/api/experimental/session/stats` yang tidak tersedia di OpenCode v2.
- `NOTCH_AGENT_CWD` tetap dihormati saat menjalankan stats.

### UI (`ui/index.html`)
- Menghapus perhitungan kuota palsu. Chip menampilkan jumlah langkah hari ini, mis. `725 langkah`.
- Menambahkan baris statistik 4 kolom di tab AI Agent: **langkah**, **token**, **perintah**, **tool berhasil**.
- Tooltip chip/baris berisi rincian: sesi, token masuk/keluar/reasoning, cache dibaca, dan rasio tool berhasil.
- Angka diformat gaya Indonesia (`1.234`, `1,99 jt`, `216,98 rb`, `71,52 jt`) agar mudah dibaca.
- Teks idle notch menjadi `opencode · 725` (jumlah langkah hari ini), bukan persentase hardcoded. Bila stats gagal, chip/baris disembunyikan dan idle tetap `opencode`.

## Verifikasi
- `cargo check --manifest-path src-tauri/Cargo.toml` berhasil.
- `opencode stats --json --days 0` mengembalikan `steps`, `prompts`, `sessions`, `tokens`, dan `tools.totals` sesuai yang dipakai UI.
- Render UI dengan data mock (725 langkah, 1,99 jt token, 14 perintah, 99% tool) menunjukkan baris statistik muat di panel 330px tanpa overflow.
- Belum dijalankan ulang di aplikasi Tauri pada sesi ini; perilaku runtime mengandalkan command `agent_usage` yang sudah dicek kompilasinya.
