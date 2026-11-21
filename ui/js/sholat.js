// Jadwal Sholat Resmi (Bimas Islam Kemenag RI) + Animasi Matahari/Bulan + Kalender Hijriah Bulanan
import { $, $$, S, T, inv, esc, ld, sv, on, toast, setTabHeight } from "./core.js";

const CITY_KEY = "nimbus.sholat.city";
const NOTIF_KEY = "nimbus.sholat.notif";
const DOTS_HTML = `<span class="m-dots" aria-hidden="true"><i></i><i></i><i></i></span>`;

const PRAYERS = [
  { key: "imsak", label: "Imsak", wajib: false },
  { key: "subuh", label: "Subuh", wajib: true },
  { key: "terbit", label: "Terbit", wajib: false },
  { key: "dhuha", label: "Dhuha", wajib: false },
  { key: "dzuhur", label: "Dzuhur", wajib: true },
  { key: "ashar", label: "Ashar", wajib: true },
  { key: "maghrib", label: "Maghrib", wajib: true },
  { key: "isya", label: "Isya", wajib: true },
];

const MONTHS_ID = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
const DAYS_SHORT = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];

let city = ld(CITY_KEY, null); // { id, lokasi, lat, lon, qibla_deg, kaaba_km }
let notifOn = ld(NOTIF_KEY, true);
let subTab = "today"; // "today" | "month" | "city"
let monthMode = "cal"; // "cal" | "table"

const nowInit = new Date();
let viewYear = nowInit.getFullYear();
let viewMonth = nowInit.getMonth() + 1; // 1..12
let selectedDay = nowInit.getDate();

let todaySched = null; // MonthSchedule untuk bulan berjalan
let viewSched = null; // MonthSchedule untuk bulan yang dilihat di tab Bulanan
let monthCache = new Map(); // "cityId:yyyy:mm" -> MonthSchedule
let loadingToday = false;
let loadingView = false;
let cityTimer = null;
let notifiedKeys = new Set();

// Animasi langit & matahari/bulan
let skyAnim = null;
let skyCtx = null;
let skyW = 0;
let skyH = 0;
let skyPaused = window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches || false;
let hoverTargetMin = null; // preview menit saat hover waktu sholat
let simMin = nowMinutes();
let skyTick = 0;

// Bintang statis dengan fase kelip unik
const STARS = Array.from({ length: 34 }, (_, i) => ({
  x: ((i * 73 + 17) % 100) / 100,
  y: ((i * 41 + 11) % 62) / 100,
  r: 0.7 + ((i * 19) % 10) * 0.11,
  ph: i * 1.13,
  sp: 0.7 + (i % 5) * 0.25,
}));

// Awan datar minimalis
const CLOUDS = [
  { x: 0.12, y: 0.28, w: 68, h: 11, sp: 0.00018, a: 0.09 },
  { x: 0.56, y: 0.2, w: 92, h: 13, sp: 0.00012, a: 0.07 },
  { x: 0.82, y: 0.36, w: 58, h: 9, sp: 0.00022, a: 0.08 },
];

function nowMinutes() {
  const d = new Date();
  return d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60;
}

function parseHHMM(str) {
  if (!str || !str.includes(":")) return null;
  const [h, m] = str.split(":").map(Number);
  if (!Number.isFinite(h) || !Number.isFinite(m)) return null;
  return h * 60 + m;
}

function fmtHMS(totalSec) {
  const s = Math.max(0, Math.floor(totalSec));
  const hh = Math.floor(s / 3600);
  const mm = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  return `${String(hh).padStart(2, "0")}:${String(mm).padStart(2, "0")}:${String(ss).padStart(2, "0")}`;
}

function titleCaseCity(raw) {
  if (!raw) return "Kota Jakarta";
  return raw
    .toLowerCase()
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .replace(/\bDki\b/g, "DKI")
    .replace(/\bDi\b/g, "DI")
    .replace(/\bKab\./g, "Kab.");
}

function lunarPhaseName(hijriDay) {
  const d = Number(hijriDay) || 1;
  if (d <= 2 || d >= 29) return "Hilal Baru";
  if (d <= 6) return "Sabit Awal";
  if (d <= 9) return "Perempat Awal";
  if (d <= 12) return "Cembung Awal";
  if (d <= 16) return "Purnama";
  if (d <= 20) return "Cembung Akhir";
  if (d <= 24) return "Perempat Akhir";
  return "Sabit Akhir";
}

function compassDir(deg) {
  const dirs = ["Utara", "Timur Laut", "Timur", "Tenggara", "Selatan", "Barat Daya", "Barat", "Barat Laut"];
  return dirs[Math.round((((deg % 360) + 360) % 360) / 45) % 8];
}

function moveInd(wrapSel, indSel) {
  const wrap = $(wrapSel);
  const ind = $(indSel);
  if (!wrap || !ind) return;
  const active = wrap.querySelector("button.on");
  if (!active || !active.offsetWidth) {
    ind.classList.remove("on");
    return;
  }
  ind.style.width = active.offsetWidth + "px";
  ind.style.transform = `translateX(${active.offsetLeft}px)`;
  ind.classList.add("on");
}

function getTodayEntry() {
  if (!todaySched?.days?.length) return null;
  const d = new Date().getDate();
  return todaySched.days.find((x) => x.day === d) || todaySched.days[0];
}

// ======================================================================
// Sub-navigasi Tab Sholat
// ======================================================================

function setSubTab(name) {
  subTab = name;
  $$("#shTabs button").forEach((b) => b.classList.toggle("on", b.dataset.sv === name));
  $$(".sh-pane").forEach((p) => p.classList.toggle("on", p.dataset.sp === name));
  const pauseBtn = $("#shSkyPause");
  if (pauseBtn) pauseBtn.hidden = name !== "today";

  setTabHeight("sholat", name === "today" ? 272 : 320);

  requestAnimationFrame(() => {
    moveInd("#shTabs", "#shTabInd");
    if (name === "month") moveInd("#shMonthMode", "#shModeInd");
  });

  if (name === "today") {
    if (S.open && S.tab === "sholat") startSky();
  } else {
    stopSky();
  }

  if (name === "month") {
    ensureMonth(viewYear, viewMonth, false, true);
  } else if (name === "city") {
    renderQiblaCard();
    loadCities($("#shCityIn")?.value || "");
    setTimeout(() => $("#shCityIn")?.focus(), 40);
  }
}

// ======================================================================
// Pengambilan Data Resmi Kemenag RI + Hijriah
// ======================================================================

const syncedKeys = new Set();

async function syncMonthBg(cityId, year, month, notifySame = false) {
  if (!T) return;
  const key = `${cityId}:${year}:${month}`;
  if (!notifySame && syncedKeys.has(key)) return;
  syncedKeys.add(key);
  try {
    const updated = await inv("sholat_sync_month", {
      cityId,
      year,
      month,
      lat: city?.lat ?? null,
      lon: city?.lon ?? null,
    });
    if (!updated) {
      if (notifySame) toast("Jadwal sholat sudah terbaru");
      return;
    }
    monthCache.set(key, updated);
    if (city?.id !== cityId) return;

    const now = new Date();
    const isTodayM = year === now.getFullYear() && month === now.getMonth() + 1;
    if (isTodayM) {
      todaySched = updated;
      updateCityPill();
      renderToday();
      renderQiblaCard();
    }
    if (viewYear === year && viewMonth === month) {
      viewSched = updated;
      if (subTab === "month") renderMonthView();
    }
    toast("Jadwal sholat diperbarui dari server Kemenag");
  } catch {
    if (notifySame) toast("Offline, memakai jadwal tersimpan");
  }
}

async function fetchMonthRaw(cityId, year, month, force = false) {
  const key = `${cityId}:${year}:${month}`;
  if (!force && monthCache.has(key)) return monthCache.get(key);

  let data;
  if (T) {
    data = await inv("sholat_month", {
      cityId,
      year,
      month,
      lat: city?.lat ?? null,
      lon: city?.lon ?? null,
      force,
    });
  } else {
    // Fallback pratinjau browser langsung ke MyQuran v2 (Kemenag)
    const mm = String(month).padStart(2, "0");
    const r = await fetch(`https://api.myquran.com/v2/sholat/jadwal/${cityId}/${year}/${mm}`);
    const j = await r.json();
    const days = (j?.data?.jadwal || []).map((it, idx) => ({
      date: it.date || "",
      day: Number((it.date || "").split("-")[2]) || idx + 1,
      label: it.tanggal || "",
      imsak: it.imsak,
      subuh: it.subuh,
      terbit: it.terbit,
      dhuha: it.dhuha,
      dzuhur: it.dzuhur,
      ashar: it.ashar,
      maghrib: it.maghrib,
      isya: it.isya,
      hijri_day: ((idx + 10) % 30) + 1,
      hijri_month: 6,
      hijri_month_name: "Jumadil Akhir",
      hijri_year: "1448",
      hijri_label: `${((idx + 10) % 30) + 1} Jumadil Akhir 1448 H`,
      holidays: [],
    }));
    data = {
      city_id: cityId,
      lokasi: j?.data?.lokasi || "KOTA JAKARTA",
      daerah: j?.data?.daerah || "DKI JAKARTA",
      year,
      month,
      source: "Bimas Islam Kemenag RI",
      qibla_deg: 295.15,
      kaaba_km: 7912,
      hijri_range: "Jumadil Awal – Jumadil Akhir 1448 H",
      days,
    };
  }
  monthCache.set(key, data);
  return data;
}

async function ensureCity() {
  if (city?.id) return city;
  if (T) {
    try {
      const d = await inv("sholat_detect_city");
      city = {
        id: d.id,
        lokasi: d.lokasi,
        lat: d.lat,
        lon: d.lon,
        qibla_deg: d.qibla_deg,
        kaaba_km: d.kaaba_km,
      };
      sv(CITY_KEY, city);
      return city;
    } catch {}
  }
  city = {
    id: "1301",
    lokasi: "KOTA JAKARTA",
    lat: -6.2088,
    lon: 106.8456,
    qibla_deg: 295.15,
    kaaba_km: 7912,
  };
  sv(CITY_KEY, city);
  return city;
}

async function ensureToday(force = false) {
  if (loadingToday) return;
  loadingToday = true;
  renderTodayStatus("Memuat jadwal resmi Kemenag RI…");
  try {
    await ensureCity();
    const d = new Date();
    const y = d.getFullYear();
    const m = d.getMonth() + 1;
    if (force && todaySched && T) {
      await syncMonthBg(city.id, y, m, true);
    } else {
      todaySched = await fetchMonthRaw(city.id, y, m, force);
      if (viewYear === y && viewMonth === m) viewSched = todaySched;
      updateCityPill();
      renderToday();
      renderQiblaCard();
      if (subTab === "month") renderMonthView();
      syncMonthBg(city.id, y, m, false);
    }
  } catch (e) {
    renderTodayStatus(`Gagal memuat jadwal: ${String(e)}`);
  } finally {
    loadingToday = false;
  }
}

async function ensureMonth(year, month, force = false, render = true) {
  viewYear = year;
  viewMonth = month;
  if (loadingView) return;
  loadingView = true;
  const sameView = viewSched && viewSched.city_id === city?.id && viewSched.year === year && viewSched.month === month;
  if (render && !(force && sameView)) renderMonthLoading();
  try {
    await ensureCity();
    if (force && sameView && T) {
      await syncMonthBg(city.id, year, month, true);
    } else {
      viewSched = await fetchMonthRaw(city.id, year, month, force);
      const now = new Date();
      if (year === now.getFullYear() && month === now.getMonth() + 1) {
        todaySched = viewSched;
        selectedDay = Math.min(selectedDay || now.getDate(), viewSched.days.length);
      } else {
        selectedDay = Math.min(selectedDay || 1, viewSched.days.length);
      }
      if (render) renderMonthView();
      syncMonthBg(city.id, year, month, false);
    }
  } catch (e) {
    const body = $("#shMonthBody");
    if (body && !sameView) body.innerHTML = `<div class="m-empty"><b>Gagal memuat jadwal bulanan</b><span>${esc(String(e))}</span></div>`;
  } finally {
    loadingView = false;
  }
}

function updateCityPill() {
  const txt = $("#shCityTxt");
  if (txt) txt.textContent = titleCaseCity(todaySched?.lokasi || city?.lokasi || "Kota Jakarta");
}

function renderTodayStatus(msg) {
  const grid = $("#shGrid");
  if (grid && !todaySched) {
    grid.innerHTML = `<div class="sh-loading">${DOTS_HTML}<span>${esc(msg)}</span></div>`;
  }
}

// ======================================================================
// Hitung Status Waktu Sholat & Countdown Real-Time
// ======================================================================

function computePrayerState(dayEntry, refMin = nowMinutes()) {
  if (!dayEntry) return null;
  const times = PRAYERS.map((p) => ({
    ...p,
    str: dayEntry[p.key] || "--:--",
    min: parseHHMM(dayEntry[p.key]),
  }));

  // Tentukan sholat aktif & berikutnya
  let activeIdx = -1;
  let nextIdx = -1;
  for (let i = 0; i < times.length; i++) {
    if (times[i].min === null) continue;
    if (refMin >= times[i].min) activeIdx = i;
    else if (nextIdx === -1) nextIdx = i;
  }

  let next = nextIdx >= 0 ? times[nextIdx] : times.find((t) => t.key === "subuh") || times[0];
  let prev = activeIdx >= 0 ? times[activeIdx] : times[times.length - 1];
  let minsLeft;
  if (nextIdx >= 0) {
    minsLeft = next.min - refMin;
  } else {
    // Lewat Isya -> menuju Subuh esok hari
    const nextDay = todaySched?.days?.find((d) => d.day === dayEntry.day + 1);
    const tomorrowSubuh = parseHHMM(nextDay?.subuh) ?? next.min ?? 260;
    minsLeft = 1440 - refMin + tomorrowSubuh;
  }

  let spanMins = 60;
  if (prev?.min != null && next?.min != null) {
    spanMins = nextIdx >= 0 ? Math.max(1, next.min - prev.min) : Math.max(1, 1440 - prev.min + next.min);
  }
  const progress = Math.max(0, Math.min(1, 1 - minsLeft / spanMins));

  return { times, activeIdx, nextIdx, active: prev, next, minsLeft, progress };
}

function renderToday() {
  const entry = getTodayEntry();
  if (!entry) return;
  const st = computePrayerState(entry);
  if (!st) return;

  // Header info di dalam Sky Hero
  const hijriEl = $("#shHijri");
  const gregEl = $("#shGreg");
  const moonEl = $("#shMoonLbl");
  const qiblaEl = $("#shQiblaPill");

  if (hijriEl) hijriEl.textContent = entry.hijri_label || "Kalender Hijriah";
  if (gregEl) {
    const d = new Date();
    gregEl.textContent = d.toLocaleDateString("id-ID", {
      weekday: "long",
      day: "numeric",
      month: "long",
      year: "numeric",
    });
  }
  if (moonEl) moonEl.textContent = entry.hijri_day ? lunarPhaseName(entry.hijri_day) : "";
  if (qiblaEl) {
    const deg = todaySched?.qibla_deg ?? city?.qibla_deg;
    const km = todaySched?.kaaba_km ?? city?.kaaba_km;
    if (Number.isFinite(deg)) {
      qiblaEl.textContent = `Kiblat ${deg.toFixed(0)}° ${compassDir(deg)}${Number.isFinite(km) ? ` · ${Math.round(km).toLocaleString("id-ID")} km` : ""}`;
    }
  }

  // 8 waktu sholat sebagai baris teks bersih tanpa kotak
  const grid = $("#shGrid");
  if (grid) {
    grid.innerHTML = st.times
      .map((p, i) => {
        const isNext = i === st.nextIdx || (st.nextIdx === -1 && p.key === "subuh");
        const isActive = i === st.activeIdx;
        const isPast = st.activeIdx >= 0 && i < st.activeIdx;
        const cls = ["sh-item"];
        if (isActive) cls.push("active");
        if (isNext) cls.push("next");
        if (isPast) cls.push("past");
        if (!p.wajib) cls.push("sunnah");
        return `
          <button type="button" class="${cls.join(" ")}" data-key="${p.key}" data-min="${p.min ?? ""}" title="Pratinjau posisi matahari saat ${p.label}">
            <span class="sh-n">${esc(p.label)}</span>
            <b class="sh-t">${esc(p.str)}</b>
          </button>`;
      })
      .join("");

    $$(".sh-item", grid).forEach((btn) => {
      const m = parseFloat(btn.dataset.min);
      if (!Number.isFinite(m)) return;
      const previewOn = () => {
        hoverTargetMin = m;
        const lbl = $("#shPreviewLbl");
        if (lbl) {
          lbl.textContent = `${btn.querySelector(".sh-n")?.textContent || ""} ${btn.querySelector(".sh-t")?.textContent || ""}`;
          lbl.hidden = false;
        }
        if (skyPaused) {
          simMin = m;
          drawSky(true);
        }
      };
      const previewOff = () => {
        hoverTargetMin = null;
        const lbl = $("#shPreviewLbl");
        if (lbl) lbl.hidden = true;
        if (skyPaused) {
          simMin = nowMinutes();
          drawSky(true);
        }
      };
      btn.addEventListener("pointerenter", previewOn);
      btn.addEventListener("pointerleave", previewOff);
      btn.addEventListener("focus", previewOn);
      btn.addEventListener("blur", previewOff);
    });
  }

  updateCountdownUI();
}

function updateCountdownUI() {
  const entry = getTodayEntry();
  if (!entry) return;
  const st = computePrayerState(entry);
  if (!st) return;

  const nextLbl = $("#shNextLbl");
  const cdEl = $("#shCountdown");
  const phaseEl = $("#shPhase");
  const progEl = $("#shProgBar");

  if (nextLbl) nextLbl.textContent = `Menuju ${st.next.label} · ${st.next.str}`;
  if (cdEl) cdEl.textContent = fmtHMS(st.minsLeft * 60);
  if (phaseEl) phaseEl.textContent = st.active ? `Waktu ${st.active.label}` : "Menjelang Subuh";
  if (progEl) progEl.style.width = (st.progress * 100).toFixed(2) + "%";

  // Cek perubahan hari otomatis saat lewat tengah malam
  const curDay = new Date().getDate();
  if (todaySched && entry.day !== curDay) {
    ensureToday(false);
  }

  // Notifikasi lembut saat masuk waktu sholat wajib
  if (notifOn) {
    const nowM = Math.floor(nowMinutes());
    const dateKey = entry.date || String(curDay);
    for (const p of st.times) {
      if (!p.wajib || p.min === null) continue;
      if (Math.abs(nowM - p.min) === 0 && new Date().getSeconds() < 10) {
        const nk = `${dateKey}:${p.key}`;
        if (!notifiedKeys.has(nk)) {
          notifiedKeys.add(nk);
          toast(`Waktu ${p.label} telah tiba · ${p.str} (${titleCaseCity(todaySched?.lokasi)})`, "ok", 6000);
        }
      }
    }
  }
}

// ======================================================================
// Animasi Langit, Matahari & Fase Bulan (60fps Flat Minimalist Canvas)
// ======================================================================

function lerp(a, b, t) {
  return a + (b - a) * t;
}

function lerpRgb(c1, c2, t) {
  return [Math.round(lerp(c1[0], c2[0], t)), Math.round(lerp(c1[1], c2[1], t)), Math.round(lerp(c1[2], c2[2], t))];
}

function rgbCss(c, a = 1) {
  return a < 1 ? `rgba(${c[0]},${c[1]},${c[2]},${a.toFixed(3)})` : `rgb(${c[0]},${c[1]},${c[2]})`;
}

function sampleSkyColors(min, tSubuh, tTerbit, tDzuhur, tAshar, tMaghrib, tIsya) {
  // Palet datar berkelas yang menyatu dengan tema gelap Nimbus
  const stops = [
    { m: 0, top: [10, 13, 28], mid: [16, 21, 42], bot: [22, 27, 50] },
    { m: tSubuh, top: [15, 19, 44], mid: [34, 34, 68], bot: [84, 52, 74] },
    { m: tTerbit, top: [26, 42, 74], mid: [64, 72, 102], bot: [212, 112, 74] },
    { m: tTerbit + 70, top: [22, 54, 92], mid: [44, 88, 128], bot: [104, 132, 152] },
    { m: tDzuhur, top: [20, 56, 98], mid: [38, 86, 134], bot: [74, 122, 160] },
    { m: tAshar, top: [28, 48, 84], mid: [68, 76, 108], bot: [148, 110, 92] },
    { m: tMaghrib - 15, top: [30, 32, 62], mid: [94, 54, 64], bot: [224, 104, 66] },
    { m: tMaghrib + 25, top: [18, 20, 44], mid: [52, 34, 58], bot: [122, 58, 60] },
    { m: tIsya, top: [11, 14, 30], mid: [18, 22, 46], bot: [28, 32, 56] },
    { m: 1440, top: [10, 13, 28], mid: [16, 21, 42], bot: [22, 27, 50] },
  ];

  let a = stops[0];
  let b = stops[stops.length - 1];
  for (let i = 0; i < stops.length - 1; i++) {
    if (min >= stops[i].m && min <= stops[i + 1].m) {
      a = stops[i];
      b = stops[i + 1];
      break;
    }
  }
  const f = b.m === a.m ? 0 : Math.max(0, Math.min(1, (min - a.m) / (b.m - a.m)));
  return {
    top: lerpRgb(a.top, b.top, f),
    mid: lerpRgb(a.mid, b.mid, f),
    bot: lerpRgb(a.bot, b.bot, f),
  };
}

function resizeSky() {
  const c = $("#shSky");
  if (!c) return;
  const dpr = devicePixelRatio || 1;
  const nw = Math.round(c.offsetWidth * dpr);
  const nh = Math.round(c.offsetHeight * dpr);
  if (!nw || !nh) {
    skyCtx = null;
    return;
  }
  if (skyCtx && nw === skyW && nh === skyH) return;
  skyW = nw;
  skyH = nh;
  c.width = skyW;
  c.height = skyH;
  skyCtx = c.getContext("2d");
}

function drawSky(singleFrame = false) {
  if (!singleFrame && !skyPaused) {
    skyAnim = requestAnimationFrame(() => drawSky(false));
  } else {
    skyAnim = null;
  }
  resizeSky();
  if (!skyCtx) return;

  if (!skyPaused) skyTick += 0.016;
  const target = hoverTargetMin !== null ? hoverTargetMin : nowMinutes();
  const diff = target - simMin;
  simMin += singleFrame || Math.abs(diff) < 0.05 ? diff : diff * 0.11;

  const entry = getTodayEntry();
  const tSubuh = parseHHMM(entry?.subuh) ?? 265;
  const tTerbit = parseHHMM(entry?.terbit) ?? 335;
  const tDzuhur = parseHHMM(entry?.dzuhur) ?? 710;
  const tAshar = parseHHMM(entry?.ashar) ?? 905;
  const tMaghrib = parseHHMM(entry?.maghrib) ?? 1075;
  const tIsya = parseHHMM(entry?.isya) ?? 1145;

  const dpr = devicePixelRatio || 1;
  const W = skyW;
  const H = skyH;
  const ctx = skyCtx;

  // 1. Gradien langit menyatu ke warna latar notch (#0c0c0d) tanpa kotak
  const pal = sampleSkyColors(simMin, tSubuh, tTerbit, tDzuhur, tAshar, tMaghrib, tIsya);
  const skyGrad = ctx.createLinearGradient(0, 0, 0, H);
  skyGrad.addColorStop(0, "rgba(12, 12, 13, 1)");
  skyGrad.addColorStop(0.24, rgbCss(pal.top, 0.86));
  skyGrad.addColorStop(0.62, rgbCss(pal.mid, 0.72));
  skyGrad.addColorStop(0.84, rgbCss(pal.bot, 0.44));
  skyGrad.addColorStop(1, "rgba(12, 12, 13, 1)");
  ctx.fillStyle = skyGrad;
  ctx.fillRect(0, 0, W, H);

  // Faktor malam (0 = siang, 1 = malam penuh)
  let nightFactor = 0;
  if (simMin < tSubuh) nightFactor = 1;
  else if (simMin < tTerbit + 15) nightFactor = 1 - (simMin - tSubuh) / (tTerbit + 15 - tSubuh);
  else if (simMin > tMaghrib - 15 && simMin < tIsya) nightFactor = (simMin - (tMaghrib - 15)) / (tIsya - (tMaghrib - 15));
  else if (simMin >= tIsya) nightFactor = 1;
  nightFactor = Math.max(0, Math.min(1, nightFactor));

  // 2. Bintang berkedip halus saat malam / fajar / senja
  if (nightFactor > 0.05) {
    for (const st of STARS) {
      const tw = 0.45 + 0.55 * (0.5 + 0.5 * Math.sin(skyTick * st.sp * 1.9 + st.ph));
      ctx.fillStyle = `rgba(255, 250, 240, ${(nightFactor * tw * 0.72).toFixed(3)})`;
      ctx.beginPath();
      ctx.arc(st.x * W, st.y * H, st.r * dpr, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // 3. Awan tipis minimalis
  const cloudAlphaBase = 0.3 + (1 - nightFactor) * 0.6;
  for (const cl of CLOUDS) {
    const cx = (((cl.x + skyTick * cl.sp) % 1.24) - 0.12) * W;
    const cy = cl.y * H;
    const cw = cl.w * dpr;
    const ch = cl.h * dpr * 0.85;
    ctx.fillStyle = `rgba(255, 246, 238, ${(cl.a * cloudAlphaBase).toFixed(3)})`;
    ctx.beginPath();
    if (ctx.roundRect) ctx.roundRect(cx, cy, cw, ch, ch / 2);
    else ctx.rect(cx, cy, cw, ch);
    ctx.fill();
  }

  // Geometri busur lintasan matahari/bulan (kompak dan sejajar di tengah)
  const padX = 112 * dpr;
  const horizonY = H * 0.72;
  const peakY = H * 0.2;
  const arcX = (u) => padX + u * (W - padX * 2);
  const arcY = (u) => horizonY - Math.sin(Math.max(0, Math.min(1, u)) * Math.PI) * (horizonY - peakY);

  // 4. Garis ufuk tipis
  ctx.strokeStyle = "rgba(255, 255, 255, 0.14)";
  ctx.lineWidth = 1 * dpr;
  ctx.beginPath();
  ctx.moveTo(padX - 16 * dpr, horizonY);
  ctx.lineTo(W - padX + 16 * dpr, horizonY);
  ctx.stroke();

  // 5. Busur lintasan putus-putus
  ctx.save();
  ctx.setLineDash([3 * dpr, 5 * dpr]);
  ctx.strokeStyle = "rgba(255, 255, 255, 0.18)";
  ctx.lineWidth = 1.2 * dpr;
  ctx.beginPath();
  for (let i = 0; i <= 48; i++) {
    const u = i / 48;
    const x = arcX(u);
    const y = arcY(u);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
  ctx.restore();

  const isDay = simMin >= tTerbit && simMin <= tMaghrib;
  let bodyU = 0.5;
  if (isDay) {
    bodyU = (simMin - tTerbit) / Math.max(1, tMaghrib - tTerbit);
  } else {
    const nightLen = 1440 - tMaghrib + tTerbit;
    const elapsed = simMin > tMaghrib ? simMin - tMaghrib : 1440 - tMaghrib + simMin;
    bodyU = elapsed / Math.max(1, nightLen);
  }
  bodyU = Math.max(0.01, Math.min(0.99, bodyU));

  // Busur progres yang telah dilalui
  ctx.strokeStyle = isDay ? "rgba(255, 214, 130, 0.55)" : "rgba(190, 214, 255, 0.4)";
  ctx.lineWidth = 1.6 * dpr;
  ctx.beginPath();
  const steps = Math.max(2, Math.round(bodyU * 48));
  for (let i = 0; i <= steps; i++) {
    const u = (i / steps) * bodyU;
    const x = arcX(u);
    const y = arcY(u);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();

  const bx = arcX(bodyU);
  const by = arcY(bodyU);

  if (isDay) {
    // 6. Pendar ufuk lembut saat matahari dekat terbit/terbenam
    const edgeWarmth = 1 - Math.sin(bodyU * Math.PI);
    if (edgeWarmth > 0.2) {
      const hg = ctx.createRadialGradient(bx, horizonY, 0, bx, horizonY, 76 * dpr);
      hg.addColorStop(0, `rgba(255, 130, 74, ${(edgeWarmth * 0.34).toFixed(3)})`);
      hg.addColorStop(1, "rgba(255, 130, 74, 0)");
      ctx.fillStyle = hg;
      ctx.fillRect(0, 0, W, H);
    }

    // 7A. Matahari (halo hangat + sinar berputar lembut + inti)
    const pulse = 1 + 0.06 * Math.sin(skyTick * 2.0);
    const sunR = 9.5 * dpr;

    const outerGlow = ctx.createRadialGradient(bx, by, sunR * 0.4, bx, by, sunR * 3.4 * pulse);
    const warmCore = lerpRgb([255, 146, 78], [255, 224, 130], Math.sin(bodyU * Math.PI));
    outerGlow.addColorStop(0, rgbCss(warmCore, 0.44));
    outerGlow.addColorStop(0.45, rgbCss(warmCore, 0.14));
    outerGlow.addColorStop(1, rgbCss(warmCore, 0));
    ctx.fillStyle = outerGlow;
    ctx.beginPath();
    ctx.arc(bx, by, sunR * 3.4 * pulse, 0, Math.PI * 2);
    ctx.fill();

    ctx.save();
    ctx.translate(bx, by);
    ctx.rotate(skyTick * 0.24);
    ctx.strokeStyle = rgbCss(warmCore, 0.62);
    ctx.lineWidth = 1.4 * dpr;
    ctx.lineCap = "round";
    for (let k = 0; k < 12; k++) {
      const a = (k / 12) * Math.PI * 2;
      const r1 = sunR + 3 * dpr;
      const r2 = sunR + (k % 2 === 0 ? 6.5 : 4.8) * dpr * pulse;
      ctx.beginPath();
      ctx.moveTo(Math.cos(a) * r1, Math.sin(a) * r1);
      ctx.lineTo(Math.cos(a) * r2, Math.sin(a) * r2);
      ctx.stroke();
    }
    ctx.restore();

    const coreGrad = ctx.createRadialGradient(bx - 1.5 * dpr, by - 1.5 * dpr, 1 * dpr, bx, by, sunR);
    coreGrad.addColorStop(0, "#fff8e1");
    coreGrad.addColorStop(0.65, rgbCss(warmCore));
    coreGrad.addColorStop(1, rgbCss(lerpRgb([255, 96, 54], warmCore, Math.sin(bodyU * Math.PI))));
    ctx.fillStyle = coreGrad;
    ctx.beginPath();
    ctx.arc(bx, by, sunR, 0, Math.PI * 2);
    ctx.fill();
  } else {
    // 7B. Bulan dengan fase Hijriah akurat
    const pulse = 1 + 0.04 * Math.sin(skyTick * 1.5);
    const moonR = 9 * dpr;

    const mGlow = ctx.createRadialGradient(bx, by, moonR * 0.5, bx, by, moonR * 3.1 * pulse);
    mGlow.addColorStop(0, "rgba(216, 232, 255, 0.34)");
    mGlow.addColorStop(0.5, "rgba(170, 200, 255, 0.1)");
    mGlow.addColorStop(1, "rgba(170, 200, 255, 0)");
    ctx.fillStyle = mGlow;
    ctx.beginPath();
    ctx.arc(bx, by, moonR * 3.1 * pulse, 0, Math.PI * 2);
    ctx.fill();

    const hDay = entry?.hijri_day || 10;
    const phase = ((hDay - 1) % 29.53) / 29.53;
    ctx.save();
    ctx.beginPath();
    ctx.arc(bx, by, moonR, 0, Math.PI * 2);
    ctx.clip();

    ctx.fillStyle = "#eef4ff";
    ctx.fillRect(bx - moonR, by - moonR, moonR * 2, moonR * 2);

    const dist = Math.abs(phase - 0.5) * 2;
    if (dist > 0.08) {
      const sign = phase < 0.5 ? -1 : 1;
      const offset = sign * (1 - dist) * moonR * 1.9 + sign * 0.25 * moonR;
      ctx.fillStyle = rgbCss(pal.mid, 0.94);
      ctx.beginPath();
      ctx.arc(bx + offset, by - moonR * 0.08, moonR * 1.02, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  }

  // 8. Penanda Terbit & Maghrib di ujung ufuk (kontras terbaca)
  ctx.font = `500 ${Math.round(9.5 * dpr)}px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`;
  ctx.fillStyle = "rgba(244, 243, 241, 0.72)";
  ctx.textBaseline = "middle";
  ctx.textAlign = "right";
  ctx.fillText(`Terbit ${entry?.terbit || "--:--"}`, padX - 22 * dpr, horizonY);
  ctx.textAlign = "left";
  ctx.fillText(`Maghrib ${entry?.maghrib || "--:--"}`, W - padX + 22 * dpr, horizonY);

  // Fade samping agar langit menyatu mulus ke tepi notch tanpa garis batas
  const sideL = ctx.createLinearGradient(0, 0, 28 * dpr, 0);
  sideL.addColorStop(0, "rgba(12, 12, 13, 1)");
  sideL.addColorStop(1, "rgba(12, 12, 13, 0)");
  ctx.fillStyle = sideL;
  ctx.fillRect(0, 0, 28 * dpr, H);

  const sideR = ctx.createLinearGradient(W - 28 * dpr, 0, W, 0);
  sideR.addColorStop(0, "rgba(12, 12, 13, 0)");
  sideR.addColorStop(1, "rgba(12, 12, 13, 1)");
  ctx.fillStyle = sideR;
  ctx.fillRect(W - 28 * dpr, 0, 28 * dpr, H);
}

function syncSkyPauseBtn() {
  const btn = $("#shSkyPause");
  if (!btn) return;
  btn.setAttribute("aria-pressed", String(skyPaused));
  btn.setAttribute("aria-label", skyPaused ? "Lanjutkan animasi langit" : "Jeda animasi langit");
  const u = btn.querySelector("use");
  if (u) u.setAttribute("href", skyPaused ? "#i-play" : "#i-pause");
}

function startSky() {
  if (subTab !== "today") return;
  syncSkyPauseBtn();
  if (skyPaused) {
    drawSky(true);
    return;
  }
  if (!skyAnim) drawSky(false);
}

function stopSky() {
  if (skyAnim) {
    cancelAnimationFrame(skyAnim);
    skyAnim = null;
  }
}

// ======================================================================
// Sub-tab Bulanan: Kalender Interaktif & Tabel Lengkap 1 Bulan Penuh
// ======================================================================

function renderMonthLoading() {
  const lbl = $("#shMonthLbl");
  const hLbl = $("#shMonthHijri");
  const body = $("#shMonthBody");
  if (lbl) lbl.textContent = `${MONTHS_ID[viewMonth - 1]} ${viewYear}`;
  if (hLbl) hLbl.textContent = "Memuat kalender…";
  if (body) body.innerHTML = `<div class="m-empty">${DOTS_HTML}<b>Memuat jadwal ${MONTHS_ID[viewMonth - 1]} ${viewYear}…</b><span>Mengambil data resmi Kemenag RI</span></div>`;
}

function renderMonthView() {
  const lbl = $("#shMonthLbl");
  const hLbl = $("#shMonthHijri");
  const body = $("#shMonthBody");
  if (!body) return;

  if (lbl) lbl.textContent = `${MONTHS_ID[viewMonth - 1]} ${viewYear}`;
  if (hLbl) hLbl.textContent = viewSched?.hijri_range || "Bimas Islam Kemenag RI";

  if (!viewSched?.days?.length) {
    body.innerHTML = `<div class="m-empty"><b>Data jadwal bulan ini belum tersedia</b></div>`;
    return;
  }

  const now = new Date();
  const isCurMonth = now.getFullYear() === viewYear && now.getMonth() + 1 === viewMonth;
  const todayNum = now.getDate();

  if (monthMode === "cal") {
    const firstDow = new Date(viewYear, viewMonth - 1, 1).getDay(); // 0=Min..6=Sab
    const cells = [];
    for (let k = 0; k < firstDow; k++) cells.push(`<span class="sh-c-pad"></span>`);

    for (const d of viewSched.days) {
      const dow = (firstDow + d.day - 1) % 7;
      const isToday = isCurMonth && d.day === todayNum;
      const isSel = d.day === selectedDay;
      const isFri = dow === 5;
      const hasHol = d.holidays?.length > 0;
      const cls = ["sh-c-day"];
      if (isToday) cls.push("today");
      if (isSel) cls.push("sel");
      if (isFri) cls.push("fri");
      if (hasHol) cls.push("hol");
      cells.push(`
        <button type="button" class="${cls.join(" ")}" data-day="${d.day}" title="${esc(d.hijri_label || d.label)}${hasHol ? ` · ${esc(d.holidays.join(", "))}` : ""}">
          <b>${d.day}</b>
          <small>${d.hijri_day ?? ""}</small>
          ${hasHol ? `<i class="sh-hol-dot" aria-hidden="true"></i>` : ""}
        </button>`);
    }

    const sel = viewSched.days.find((x) => x.day === selectedDay) || viewSched.days[0];
    const holHtml = sel?.holidays?.length ? `<span class="sh-hol-txt">${esc(sel.holidays.join(" · "))}</span>` : "";

    body.innerHTML = `
      <div class="sh-month-split">
        <div class="sh-cal-wrap">
          <div class="sh-cal-dow">${DAYS_SHORT.map((n, i) => `<span class="${i === 5 ? "fri" : ""}">${n}</span>`).join("")}</div>
          <div class="sh-cal-grid">${cells.join("")}</div>
        </div>
        <div class="sh-day-detail" id="shDayDetail">
          <div class="sh-dd-h">
            <b>${esc(sel.label || `${sel.day} ${MONTHS_ID[viewMonth - 1]} ${viewYear}`)}</b>
            <span>${esc(sel.hijri_label || "")}</span>
            ${holHtml}
          </div>
          <div class="sh-dd-list">
            ${PRAYERS.map((p) => `<div class="sh-dd-row${p.wajib ? "" : " sunnah"}"><span>${p.label}</span><b>${esc(sel[p.key])}</b></div>`).join("")}
          </div>
        </div>
      </div>`;

    $$(".sh-c-day", body).forEach((btn) => {
      btn.onclick = () => {
        selectedDay = Number(btn.dataset.day);
        renderMonthView();
      };
    });
  } else {
    // Mode Tabel Lengkap 1 Bulan Penuh (baris tombol native untuk aksesibilitas keyboard)
    const rows = viewSched.days
      .map((d) => {
        const isToday = isCurMonth && d.day === todayNum;
        const isSel = d.day === selectedDay;
        const cls = ["sh-tr"];
        if (isToday) cls.push("today");
        if (isSel) cls.push("sel");
        return `
          <button type="button" class="${cls.join(" ")}" data-day="${d.day}">
            <span class="c-d"><b>${String(d.day).padStart(2, "0")}</b><small>${d.hijri_day ? `${d.hijri_day}H` : ""}</small></span>
            <span>${esc(d.imsak)}</span>
            <span class="w">${esc(d.subuh)}</span>
            <span>${esc(d.terbit)}</span>
            <span>${esc(d.dhuha)}</span>
            <span class="w">${esc(d.dzuhur)}</span>
            <span class="w">${esc(d.ashar)}</span>
            <span class="w">${esc(d.maghrib)}</span>
            <span class="w">${esc(d.isya)}</span>
          </button>`;
      })
      .join("");

    body.innerHTML = `
      <div class="sh-tbl">
        <div class="sh-th">
          <span>Tgl</span><span>Imsak</span><span>Subuh</span><span>Terbit</span><span>Dhuha</span><span>Dzuhur</span><span>Ashar</span><span>Maghrib</span><span>Isya</span>
        </div>
        <div class="scroll sh-tbody" id="shTbody">${rows}</div>
      </div>`;

    $$(".sh-tr", body).forEach((row) => {
      row.onclick = () => {
        selectedDay = Number(row.dataset.day);
        $$(".sh-tr", body).forEach((r) => r.classList.toggle("sel", r === row));
      };
    });

    if (isCurMonth) {
      requestAnimationFrame(() => {
        $("#shTbody .sh-tr.today")?.scrollIntoView({ behavior: "auto", block: "center" });
      });
    }
  }
}

function stepMonth(delta) {
  let m = viewMonth + delta;
  let y = viewYear;
  if (m < 1) {
    m = 12;
    y -= 1;
  } else if (m > 12) {
    m = 1;
    y += 1;
  }
  selectedDay = 1;
  ensureMonth(y, m, false, true);
}

// ======================================================================
// Sub-tab Kota & Kompas Kiblat
// ======================================================================

function renderQiblaCard() {
  const deg = todaySched?.qibla_deg ?? city?.qibla_deg ?? 295.15;
  const km = todaySched?.kaaba_km ?? city?.kaaba_km ?? 7912;
  const needle = $("#shNeedle");
  if (needle) needle.style.transform = `rotate(${deg.toFixed(1)}deg)`;
  const degEl = $("#shQiblaDeg");
  const kmEl = $("#shQiblaKm");
  const locEl = $("#shQiblaLoc");
  if (degEl) degEl.textContent = `${deg.toFixed(1)}° ${compassDir(deg)}`;
  if (kmEl) kmEl.textContent = `${Math.round(km).toLocaleString("id-ID")} km ke Ka'bah · Kemenag RI`;
  if (locEl) {
    const daerah = todaySched?.daerah ? ` · ${titleCaseCity(todaySched.daerah)}` : "";
    locEl.textContent = `${titleCaseCity(todaySched?.lokasi || city?.lokasi || "Kota Jakarta")}${daerah}`;
  }
  $("#shNotifSw")?.classList.toggle("on", !!notifOn);
}

async function loadCities(query = "") {
  const listEl = $("#shCityList");
  if (!listEl) return;
  listEl.innerHTML = `<div class="m-empty">${DOTS_HTML}<b>Memuat daftar kota Kemenag...</b></div>`;
  try {
    let items;
    if (T) {
      items = await inv("sholat_search_cities", { query });
    } else {
      const r = await fetch(query.trim() ? `https://api.myquran.com/v2/sholat/kota/cari/${encodeURIComponent(query.trim())}` : "https://api.myquran.com/v2/sholat/kota/semua");
      const j = await r.json();
      items = (j?.data || []).slice(0, 50);
    }
    if (!items?.length) {
      listEl.innerHTML = `<div class="m-empty"><b>Kota tidak ditemukan</b><span>Coba kata kunci lain (mis. Jakarta, Bandung, Surabaya).</span></div>`;
      return;
    }
    listEl.innerHTML = items
      .map((c) => {
        const active = city?.id === c.id;
        return `
          <button type="button" class="m-row${active ? " now" : ""}" data-cid="${esc(c.id)}" data-lok="${esc(c.lokasi)}">
            <span class="sh-city-dot" aria-hidden="true"></span>
            <div class="m-row-b"><b>${esc(titleCaseCity(c.lokasi))}</b><span>Kode Kemenag ${esc(c.id)}</span></div>
            ${active ? `<span class="sh-active-txt">Aktif</span>` : ""}
          </button>`;
      })
      .join("");

    $$(".m-row", listEl).forEach((btn) => {
      btn.onclick = () => {
        city = {
          ...(city || {}),
          id: btn.dataset.cid,
          lokasi: btn.dataset.lok,
        };
        sv(CITY_KEY, city);
        monthCache.clear();
        syncedKeys.clear();
        todaySched = null;
        viewSched = null;
        toast(`Lokasi sholat: ${titleCaseCity(city.lokasi)}`);
        setSubTab("today");
        ensureToday(false);
      };
    });
  } catch (e) {
    listEl.innerHTML = `<div class="m-empty"><b>Gagal memuat kota</b><span>${esc(String(e))}</span></div>`;
  }
}

// ======================================================================
// Event Listeners & Inisialisasi
// ======================================================================

function bindEvents() {
  $$("#shTabs button").forEach((b) => {
    b.onclick = () => setSubTab(b.dataset.sv);
  });
  $("#shCityPill")?.addEventListener("click", () => setSubTab(subTab === "city" ? "today" : "city"));
  $("#shSkyPause")?.addEventListener("click", () => {
    skyPaused = !skyPaused;
    syncSkyPauseBtn();
    if (skyPaused) {
      stopSky();
      drawSky(true);
    } else if (S.open && S.tab === "sholat" && subTab === "today") {
      startSky();
    }
  });
  $("#shRefresh")?.addEventListener("click", () => {
    if (subTab === "month") ensureMonth(viewYear, viewMonth, true, true);
    else ensureToday(true);
  });

  // Navigasi bulan
  $("#shPrevM")?.addEventListener("click", () => stepMonth(-1));
  $("#shNextM")?.addEventListener("click", () => stepMonth(1));
  $("#shNowM")?.addEventListener("click", () => {
    const d = new Date();
    selectedDay = d.getDate();
    ensureMonth(d.getFullYear(), d.getMonth() + 1, false, true);
  });
  $$("#shMonthMode button").forEach((b) => {
    b.onclick = () => {
      monthMode = b.dataset.mm;
      $$("#shMonthMode button").forEach((x) => x.classList.toggle("on", x === b));
      moveInd("#shMonthMode", "#shModeInd");
      renderMonthView();
    };
  });

  // Pencarian kota & deteksi otomatis
  const cityIn = $("#shCityIn");
  cityIn?.addEventListener("input", () => {
    clearTimeout(cityTimer);
    const q = cityIn.value;
    $("#shCityClr").hidden = !q.trim();
    cityTimer = setTimeout(() => loadCities(q), 220);
  });
  $("#shCityClr")?.addEventListener("click", () => {
    if (cityIn) cityIn.value = "";
    $("#shCityClr").hidden = true;
    loadCities("");
    cityIn?.focus();
  });
  $("#shDetectBtn")?.addEventListener("click", async () => {
    city = null;
    monthCache.clear();
    syncedKeys.clear();
    todaySched = null;
    viewSched = null;
    toast("Mendeteksi lokasi otomatis…");
    await ensureCity();
    await ensureToday(false);
    renderQiblaCard();
    loadCities(cityIn?.value || "");
    toast(`Terdeteksi: ${titleCaseCity(city?.lokasi)}`);
  });

  $("#shNotifBtn")?.addEventListener("click", () => {
    notifOn = !notifOn;
    sv(NOTIF_KEY, notifOn);
    $("#shNotifSw")?.classList.toggle("on", notifOn);
    toast(notifOn ? "Pengingat waktu sholat aktif" : "Pengingat waktu sholat dimatikan");
  });
}

export function initSholat() {
  bindEvents();
  updateCityPill();

  const syncInds = () =>
    requestAnimationFrame(() => {
      moveInd("#shTabs", "#shTabInd");
      if (subTab === "month") moveInd("#shMonthMode", "#shModeInd");
    });

  on("tab", (t) => {
    if (t === "sholat") {
      syncInds();
      if (!todaySched) ensureToday(false);
      if (S.open && subTab === "today") startSky();
      updateCountdownUI();
    } else {
      stopSky();
    }
  });
  on("open", () => {
    if (S.tab === "sholat") {
      syncInds();
      if (!todaySched) ensureToday(false);
      if (subTab === "today") startSky();
      updateCountdownUI();
    }
  });
  on("close", () => {
    stopSky();
  });
  on("esc", (ev) => {
    if (S.tab === "sholat" && subTab !== "today") {
      setSubTab("today");
      ev.handled = true;
    }
  });

  // Muat jadwal hari ini diam-diam di latar belakang agar pengingat sholat & tab siap instan
  setTimeout(() => ensureToday(false), 1200);
  setInterval(() => {
    if (todaySched) updateCountdownUI();
  }, 1000);
}
