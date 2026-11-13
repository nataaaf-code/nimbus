# Usage Hari Ini + Kuota/Sisa Harian

**Tanggal**: 2026-11-05
**Domain**: ui
**Status**: Terverifikasi (syntax check; belum dijalankan di aplikasi Tauri)

## Latar Belakang
Pengguna ingin panel AI Agent hanya menampilkan usage hari ini, sekaligus angka kuota dan sisa. OpenCode free tier tidak menyediakan API kuota resmi (limit per IP, dinamis, tanpa counter), jadi digunakan budget lokal yang eksplisit.

## Perubahan (`ui/index.html`)
- Menambah `DAILY_QUOTA` (default **200 langkah/hari**), bisa diubah lewat `localStorage.setItem("notch.quota", "300")`.
- Chip usage kini menampilkan `sisa/kuota` (mis. `88/200`), bukan jumlah langkah saja.
- Baris statistik 4 kolom: **dipakai**, **kuota/hari**, **sisa · %**, **token**. Angka sisa diberi aksen saat tersisa ≤20%.
- Tooltip menegaskan bahwa ini budget lokal, bukan kuota resmi OpenCode.
- Teks idle notch menjadi `opencode · 88/200`.
- `opencode stats --json --days 0` tetap dipakai, sehingga hitungan kembali ke nol tiap tengah malam lokal.
- Usage di-refresh tiap 60 detik dan saat tombol `↺` ditekan, agar pergantian hari ikut terdeteksi tanpa restart.

## Verifikasi
- `node --check` pada skrip hasil ekstrak `ui/index.html` berhasil.
- Belum dijalankan ulang di aplikasi Tauri pada sesi ini.
