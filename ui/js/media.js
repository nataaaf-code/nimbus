// Media: Now Playing dari Windows SMTC (Spotify, browser, dll) tanpa login.
import { $, S, inv, icon, mmss, on, listen, T } from "./core.js";

let md = null, pos = 0, at = 0, shownCover = "";
const B = document.body;

async function poll() {
  try { md = await inv("media"); } catch { md = null; }
  pos = md?.pos || 0;
  at = performance.now();
  draw();
}

function draw() {
  $("#mTitle").textContent = md ? md.title || "Tanpa judul" : "Tidak ada media";
  $("#mArtist").textContent = md ? md.artist || "" : T ? "Putar musik di Spotify atau pemutar lain" : "Jalankan sebagai aplikasi Windows";
  $("#mPlay").innerHTML = icon(md?.playing ? "pause" : "play");
  B.classList.toggle("playing", !!md?.playing);
  const art = $("#mArt");
  const cover = md?.cover || "";
  if (cover !== shownCover) {
    shownCover = cover;
    art.style.backgroundImage = cover ? `url("${cover}")` : "";
    art.classList.toggle("cover", !!cover);
  }
  const artTxt = art.querySelector(".art-ov span");
  if (artTxt) artTxt.textContent = md?.artist || "";
  tick();
}

function tick() {
  const d = md?.dur || 0;
  const p = md?.playing ? Math.min(d, pos + (performance.now() - at) / 1000) : pos;
  $("#mPos").textContent = mmss(p);
  $("#mDur").textContent = mmss(d);
  $("#mBar").style.width = d ? (p / d) * 100 + "%" : "0%";
}

const ctl = (op) => () => inv("media_ctl", { op }).then(() => setTimeout(poll, 250)).catch(() => {});
$("#mPrev").onclick = ctl("prev");
$("#mPlay").onclick = ctl("toggle");
$("#mNext").onclick = ctl("next");

// ======================================================================
// Spektrum audio — data real dari WASAPI loopback
// ======================================================================
const N = 52;
const bars     = new Float32Array(N);  // nilai tampilan utama
const miniBars = new Float32Array(16); // nilai tampilan mini di notch

let liveBars = null;    // payload dari Rust [f32;52]
let unlisten = null;

// rAF handles
let anim = null, miniAnim = null, pulseAnim = null;

// Canvas utama
let sCtx = null, sW = 0, sH = 0;

// Canvas mini
let mCtx = null, mW = 0, mH = 0;

// --- Simulasi fallback ---
let sPh = 0, sE = 0, sBt = 0;
function simStep() {
  sPh += 0.016;
  sBt = (sBt + 1) % 26;
  const onBeat = sBt < 3, nearBeat = sBt < 7;
  const eT = onBeat ? 1.0 : nearBeat ? 0.65 + Math.random() * 0.2 : 0.35 + Math.random() * 0.3;
  sE += (eT - sE) * 0.18;
  for (let i = 0; i < N; i++) {
    const t = i / (N - 1);
    let v;
    if (t < 0.22) {
      v = sE * (onBeat ? 1.2 : 0.9) * (0.55 + 0.45 * Math.sin(sPh * 1.1 + i * 0.6)) * (0.85 + 0.15 * Math.random());
    } else if (t < 0.62) {
      v = sE * 0.78 * (0.45 + 0.55 * Math.abs(Math.sin(sPh * 2.3 + i * 0.75 + 1.5))) * (0.75 + 0.25 * Math.random());
    } else {
      v = sE * 0.50 * (0.25 + 0.75 * Math.random()) * (onBeat ? 1.0 : 0.6);
    }
    bars[i] += (Math.max(0, Math.min(1, v)) - bars[i]) * (v > bars[i] ? 0.42 : 0.10);
  }
}

// Warna solid grey/white — cocok di dark mode
function barColor(t, alpha) {
  // Bass sedikit lebih hangat, high sedikit lebih cool — subtil
  const warm = Math.round(210 + t * 20);   // 210..230
  const cool = Math.round(215 + t * 15);   // 215..230
  const r = warm, g = warm, b = cool;
  return { r, g, b, css: `rgba(${r},${g},${b},${alpha.toFixed(2)})` };
}

// ---- Spectrum utama ----
function drawSpec() {
  anim = requestAnimationFrame(drawSpec);
  resizeSpec();
  if (!sCtx) return;
  sCtx.clearRect(0, 0, sW, sH);

  if (!T) {
    simStep();
  } else {
    // Di aplikasi: murni data WASAPI. Senyap/belum ada data → bar turun ke baseline.
    for (let i = 0; i < N; i++) {
      const v = liveBars ? liveBars[i] ?? 0 : 0;
      bars[i] += (v - bars[i]) * (v > bars[i] ? 0.65 : 0.12);
    }
  }

  const dpr   = devicePixelRatio || 1;
  const gap   = Math.max(2, Math.round(3 * dpr));
  const bw    = (sW - gap * (N - 1)) / N;
  const baseY = sH - 2 * dpr;
  const maxH  = baseY * 0.9;

  for (let i = 0; i < N; i++) {
    const v = bars[i];
    const x = i * (bw + gap);
    if (v < 0.004) {
      sCtx.shadowBlur = 0;
      sCtx.fillStyle = "rgba(220,222,228,0.12)";
      const dh = 1.5 * dpr;
      sCtx.beginPath();
      if (sCtx.roundRect) {
        sCtx.roundRect(x, baseY - dh, bw, dh, dh / 2);
      } else {
        sCtx.rect(x, baseY - dh, bw, dh);
      }
      sCtx.fill();
      continue;
    }
    const h  = Math.max(2.5 * dpr, v * maxH);
    const ty = baseY - h;
    const t  = i / (N - 1);
    const { r, g, b } = barColor(t, 1);
    const alpha = 0.48 + v * 0.47;

    // Gradient vertikal menyatu ke dasar notch yang gelap
    const grad = sCtx.createLinearGradient(0, ty, 0, baseY);
    grad.addColorStop(0, `rgba(${r},${g},${b},${alpha.toFixed(2)})`);
    grad.addColorStop(0.65, `rgba(${r},${g},${b},${(alpha * 0.35).toFixed(2)})`);
    grad.addColorStop(1, `rgba(${r},${g},${b},0)`);
    sCtx.fillStyle = grad;

    // Glow lembut di puncak bar aktif
    if (v > 0.08) {
      sCtx.shadowColor = `rgba(225,227,232,0.32)`;
      sCtx.shadowBlur  = Math.min(8, v * 9) * dpr;
    } else {
      sCtx.shadowBlur = 0;
    }

    const cr = Math.min(bw / 2, 3 * dpr);
    sCtx.beginPath();
    if (sCtx.roundRect) {
      sCtx.roundRect(x, ty, bw, h, [cr, cr, 0, 0]);
    } else {
      sCtx.rect(x, ty, bw, h);
    }
    sCtx.fill();
  }
  sCtx.shadowBlur = 0;
}

function resizeSpec() {
  const c = $("#mSpec");
  if (!c) return;
  const dpr = devicePixelRatio || 1;
  const nw  = Math.round(c.offsetWidth * dpr);
  const nh  = Math.round(c.offsetHeight * dpr);
  if (!nw || !nh) { sCtx = null; return; }
  if (sCtx && nw === sW && nh === sH) return;
  sW = nw; sH = nh;
  c.width = sW; c.height = sH;
  sCtx = c.getContext("2d");
}

function startSpec() {
  if (!anim) drawSpec();
  ensureSpecCapture();
}

function stopSpec() {
  if (anim) { cancelAnimationFrame(anim); anim = null; }
}

// ---- Mini spectrum di notch ----
function drawMini() {
  if (!mCtx || !mW || !mH) { miniAnim = null; return; }
  miniAnim = requestAnimationFrame(drawMini);
  mCtx.clearRect(0, 0, mW, mH);

  // Downsample 52 → 16 bar
  for (let i = 0; i < 16; i++) {
    const src = Math.round(i * (N - 1) / 15);
    const v   = liveBars ? (liveBars[src] ?? 0) : bars[src]; // pakai bars (sudah smooth)
    miniBars[i] += (v - miniBars[i]) * (v > miniBars[i] ? 0.6 : 0.14);
  }

  const dpr = devicePixelRatio || 1;
  const gap = Math.max(1, Math.round(1 * dpr));
  const bw  = (mW - gap * 15) / 16;

  for (let i = 0; i < 16; i++) {
    const v = miniBars[i];
    if (v < 0.01) continue;
    const h = Math.max(dpr, v * mH * 0.90);
    const x = i * (bw + gap);
    const t = i / 15;
    const { r, g, b } = barColor(t, 0.9);
    mCtx.fillStyle = `rgba(${r},${g},${b},${(0.5 + v * 0.5).toFixed(2)})`;
    mCtx.fillRect(x, mH - h, bw, h);
  }
}

function resizeMini() {
  const c = $("#miniSpec");
  if (!c) return;
  const dpr = devicePixelRatio || 1;
  const nw  = Math.round(c.offsetWidth * dpr);
  const nh  = Math.round(c.offsetHeight * dpr);
  if (!nw || !nh) return;
  mW = nw; mH = nh;
  c.width = mW; c.height = mH;
  mCtx = c.getContext("2d");
}

function startMini() {
  resizeMini();
  if (!miniAnim) drawMini();
  B.classList.add("has-spec");
}

function stopMini() {
  if (miniAnim) { cancelAnimationFrame(miniAnim); miniAnim = null; }
  B.classList.remove("has-spec");
}

// ---- Line pulse — orange glow ikut bass saat notch nutup di browser ----
const linePulse = document.getElementById("linePulse");
let pulseE = 0; // energi bass yang sudah di-smooth

function drivePulse() {
  pulseAnim = requestAnimationFrame(drivePulse);
  if (!linePulse) return;

  if (document.body.dataset.mode !== "line") {
    // Di luar mode line: matikan pulse langsung
    if (+linePulse.style.opacity > 0) linePulse.style.opacity = "0";
    return;
  }

  // Rata-rata bass (bar 0-14 = ~20-600 Hz)
  let bassE = 0;
  if (liveBars) {
    for (let i = 0; i < 15; i++) bassE += liveBars[i] ?? 0;
    bassE /= 15;
  }

  // Smooth: attack cepat (beat), release lambat
  const target = md?.playing ? bassE : 0;
  pulseE += (target - pulseE) * (target > pulseE ? 0.55 : 0.07);

  // Opacity: 0 saat diam, 0.08–0.88 saat main (ikut beat)
  const o = md?.playing ? Math.min(0.88, 0.08 + pulseE * 0.80) : 0;
  linePulse.style.opacity = o.toFixed(3);
}

// ---- Pastikan capture Rust berjalan ----
let captureStarted = false;
async function ensureSpecCapture() {
  if (!T) return;
  if (captureStarted) return;
  captureStarted = true;
  try {
    unlisten = await listen("spectrum", (e) => {
      liveBars = e.payload;
      if (!miniAnim) startMini();
    });
    await inv("spectrum_start");
  } catch (err) {
    console.error("spectrum", err);
    captureStarted = false;
  }
}

export function initMedia() {
  draw();

  // Event handler tab/open/close jalan di browser maupun Tauri
  on("tab", (t) => {
    if (t === "media") { if (T) poll(); if (S.open) startSpec(); }
    else stopSpec();
  });
  on("open", () => {
    if (S.tab === "media") startSpec();
  });
  on("close", () => {
    stopSpec();
    // Mini spec tetap jalan saat notch nutup kalau musik masih main
    if (!md?.playing) stopMini();
  });

  if (S.open && S.tab === "media") startSpec();
  // Capture idle hampir gratis (Rust diam saat senyap), jadi nyalakan sejak awal
  ensureSpecCapture();
  drivePulse();

  if (!T) return;

  poll();
  setInterval(poll, 2000);
  setInterval(() => { if (S.open && S.tab === "media" && md?.playing) tick(); }, 500);
}
