// Hari ini: Google Calendar, Cuaca, Tugas, Catatan, Fokus (pomodoro), riwayat Clipboard.
import { $, $$, S, inv, esc, icon, ld, sv, on, emit, mmss, toast, T, layout } from "./core.js";

// ---------- segmen kanan ----------
let seg = ld("notch.side", "todo");
function setSeg(s) {
  seg = s; sv("notch.side", s);
  $$("#sideSeg button").forEach((b) => b.classList.toggle("on", b.dataset.s === s));
  $$(".side-v").forEach((v) => v.classList.toggle("on", v.dataset.s === s));
  if (s === "clip") drawClips();
}
$("#sideSeg").onclick = (e) => { const b = e.target.closest("button"); if (b) setSeg(b.dataset.s); };

// ---------- kalender ----------
let sel = 0, evs = [], dayEvs = [];
const day = (i) => { const d = new Date(); d.setHours(0, 0, 0, 0); d.setDate(d.getDate() + i); return d; };
const evStart = (x) => new Date(x.start.length === 10 ? x.start + "T00:00:00" : x.start);

async function loadCal() {
  let st = { has_url: false };
  try { st = await inv("ical_status"); } catch {}
  const connected = st.has_url;
  $("#calLock").hidden = connected;
  $("#calDisc").hidden = !connected;
  if (!connected) { drawCal(); return; }
  try { evs = await inv("ical_events", { min: day(0).toISOString(), max: day(7).toISOString() }); }
  catch (e) { $("#calEvs").innerHTML = `<div class="empty">${esc(e)}</div>`; return; }
  drawCal();
  checkNextEvent();
}

function drawCal() {
  $("#calDate").textContent = day(sel).toLocaleDateString("id-ID", { weekday: "long", day: "numeric", month: "long" });
  const has = (i) => evs.some((x) => { const t = evStart(x).getTime(); return t >= day(i).getTime() && t < day(i + 1).getTime(); });
  $("#calWeek").innerHTML = [0, 1, 2, 3, 4, 5, 6].map((i) => `<button class="${i === sel ? "on" : ""}${has(i) ? " has" : ""}" data-d="${i}">${day(i).toLocaleDateString("id-ID", { weekday: "short" })}<b>${day(i).getDate()}</b></button>`).join("");
  const t0 = day(sel).getTime();
  dayEvs = evs.filter((x) => { const t = evStart(x).getTime(); return t >= t0 && t < t0 + 864e5; });
  const nowT = Date.now();
  const nextIdx = sel === 0 ? dayEvs.findIndex((x) => evStart(x).getTime() >= nowT) : -1;
  $("#calEvs").innerHTML = dayEvs.length
    ? dayEvs.map((x, i) => `<div class="ev${i === nextIdx ? " nx" : ""}" data-i="${i}" role="button" tabindex="0"><time>${x.start.length === 10 ? "Seharian" : evStart(x).toLocaleTimeString("id-ID", { hour: "2-digit", minute: "2-digit" })}</time><span>${esc(x.title)}</span></div>`).join("")
    : `<div class="empty">${icon("cal")}Tidak ada acara</div>`;
}

$("#calEvs").addEventListener("click", (e) => {
  const el = e.target.closest("[data-i]");
  if (el) showEvDetail(dayEvs[+el.dataset.i]);
});

function showEvDetail(ev) {
  const start = evStart(ev);
  const end = ev.end ? new Date(ev.end.length === 10 ? ev.end + "T00:00:00" : ev.end) : null;
  const allDay = ev.start.length === 10;
  if (allDay) {
    $("#evDTime").textContent = "Seharian";
  } else {
    const fmt = { hour: "2-digit", minute: "2-digit" };
    const s = start.toLocaleTimeString("id-ID", fmt);
    const e2 = end ? " – " + end.toLocaleTimeString("id-ID", fmt) : "";
    $("#evDTime").textContent = s + e2;
  }
  $("#evDTitle").textContent = ev.title;
  const hasLoc = ev.location?.trim();
  $("#evDLoc").style.display = hasLoc ? "" : "none";
  if (hasLoc) $("#evDLocTxt").textContent = ev.location;
  const hasDesc = ev.description?.trim();
  $("#evDDesc").style.display = hasDesc ? "" : "none";
  if (hasDesc) $("#evDDesc").textContent = ev.description;
  const hasUrl = ev.url?.trim();
  $("#evDUrl").style.display = hasUrl ? "" : "none";
  if (hasUrl) $("#evDUrl").href = ev.url;
  $("#evDetail").classList.add("on");
}

$("#evDetailClose").onclick = () => $("#evDetail").classList.remove("on");

$("#calWeek").onclick = (e) => { const b = e.target.closest("[data-d]"); if (b) { sel = +b.dataset.d; drawCal(); } };

// Tutorial: salin alamat ke clipboard agar bisa dibuka di browser
$("#gcOpenCal").onclick = async () => {
  try { await navigator.clipboard.writeText("https://calendar.google.com"); toast("URL disalin — tempel di browser"); }
  catch { toast("Buka calendar.google.com di browser", "warn"); }
};

// Simpan URL ICS
$("#gcSaveUrl").onclick = async () => {
  const url = $("#gcIcsUrl").value.trim();
  if (!url) return toast("Tempel URL ICS dari Google Calendar", "warn");
  const b = $("#gcSaveUrl"); b.disabled = true; b.textContent = "Menyimpan…";
  try {
    await inv("ical_save_url", { url });
    evs = await inv("ical_events", { min: day(0).toISOString(), max: day(7).toISOString() });
    $("#calLock").hidden = true;
    $("#calDisc").hidden = false;
    drawCal();
    checkNextEvent();
    toast("Google Calendar terhubung", "ok");
  } catch (e) { toast(String(e), "err"); }
  b.disabled = false; b.textContent = "Simpan";
};

// Enter di input URL juga simpan
$("#gcIcsUrl").onkeydown = (e) => { if (e.key === "Enter") $("#gcSaveUrl").click(); };

// Putuskan kalender
$("#calDisc").onclick = async () => {
  await inv("ical_clear").catch(() => {});
  evs = [];
  drawCal();
  S.calLive = null; emit("pill");
  $("#calLock").hidden = false;
  $("#calDisc").hidden = true;
  toast("Kalender diputuskan");
};

// ---------- tugas ----------
let tds = ld("tds", []);
function drawTd() {
  const d = tds.filter((x) => x[1]).length;
  $("#tdCount").textContent = tds.length ? `${d} dari ${tds.length} selesai` : "Belum ada tugas";
  $("#tdBar").style.width = (tds.length ? (d / tds.length) * 100 : 0) + "%";
  $("#tdList").innerHTML = tds.map((x, i) => `<div class="todo${x[1] ? " d" : ""}" data-i="${i}" role="button" tabindex="0"><span class="cb">${icon("check")}</span><span class="tx">${esc(x[0])}</span><button class="del" data-del="${i}" title="Hapus">${icon("x")}</button></div>`).join("");
}
$("#tdList").onclick = (e) => {
  const del = e.target.closest("[data-del]");
  if (del) { tds.splice(+del.dataset.del, 1); sv("tds", tds); return drawTd(); }
  const t = e.target.closest(".todo");
  if (t) { tds[+t.dataset.i][1] ^= 1; sv("tds", tds); drawTd(); }
};
const addT = () => { const v = $("#tdIn").value.trim(); if (!v) return; tds.push([v, 0]); sv("tds", tds); $("#tdIn").value = ""; drawTd(); $("#tdList").scrollTop = 1e6; };
$("#tdAdd").onclick = addT;
$("#tdIn").onkeydown = (e) => e.key === "Enter" && addT();

// ---------- catatan ----------
const nt = $("#ntA");
nt.value = ld("note", "");
let ntT;
const ntCount = () => ($("#ntC").textContent = nt.value.length + " karakter");
ntCount();
nt.oninput = () => { ntCount(); $("#ntS").textContent = "Menyimpan…"; clearTimeout(ntT); ntT = setTimeout(() => { sv("note", nt.value); $("#ntS").textContent = "Tersimpan"; }, 500); };
$("#ntSend").onclick = () => { const v = nt.value.trim(); if (v) emit("newSession", { prompt: v }); };

// ---------- fokus ----------
const today0 = new Date().toDateString();
let fd = ld("fd", { d: today0, n: 0 });
if (fd.d !== today0) fd = { d: today0, n: 0 };
let fmode = 25, fleft = 1500, frun = false, fend = 0;
function drawF() {
  $("#fcTime").textContent = mmss(fleft);
  $("#fcRing").style.strokeDashoffset = 326.7 * (1 - fleft / (fmode * 60));
  $("#fcStart").textContent = frun ? "Jeda" : fleft < fmode * 60 ? "Lanjut" : "Mulai";
  $("#fcN").textContent = `Sesi fokus hari ini: ${fd.n}`;
  S.focusLive = frun ? { dot: "focus", proj: fmode === 25 ? "Fokus" : "Istirahat", act: "", meta: mmss(fleft) } : null;
  emit("pill");
}
$("#fcStart").onclick = () => { frun = !frun; if (frun) fend = Date.now() + fleft * 1000; drawF(); };
$("#fcReset").onclick = () => { frun = false; fleft = fmode * 60; drawF(); };
$("#fcMode").onclick = (e) => {
  const b = e.target.closest("[data-m]"); if (!b) return;
  fmode = +b.dataset.m; frun = false; fleft = fmode * 60;
  $$("#fcMode button").forEach((x) => x.classList.toggle("on", x === b));
  drawF();
};
setInterval(() => {
  if (!frun) return;
  fleft = Math.max(0, Math.round((fend - Date.now()) / 1000));
  if (fleft <= 0) {
    frun = false;
    if (fmode === 25) { fd.n++; sv("fd", fd); }
    toast(fmode === 25 ? "Sesi fokus selesai — waktunya istirahat" : "Istirahat selesai", "ok", 6000);
    fleft = fmode * 60;
  }
  drawF();
}, 1000);

// ---------- clipboard ----------
let clips = ld("notch.clipboard", []);
const clipSafe = (v) => v && v.length <= 4000 && !/password|passwd|token|secret|api[_ -]?key|bearer\s/i.test(v);
function drawClips() {
  $("#clList").innerHTML = clips.length
    ? clips.map((v, i) => `<button class="clip" data-c="${i}" title="Klik untuk menyalin">${esc(v.replace(/\s+/g, " ").slice(0, 200))}</button>`).join("")
    : `<div class="empty">${icon("copy")}Belum ada riwayat</div>`;
}
$("#clList").onclick = async (e) => {
  const b = e.target.closest("[data-c]"); if (!b) return;
  try { await navigator.clipboard.writeText(clips[+b.dataset.c]); toast("Disalin ke clipboard"); } catch { toast("Clipboard tidak tersedia", "err"); }
};
async function pollClip() {
  if (!navigator.clipboard?.readText) return;
  try {
    const v = (await navigator.clipboard.readText()).trim();
    if (clipSafe(v) && clips[0] !== v) {
      clips = [v, ...clips.filter((x) => x !== v)].slice(0, 20);
      sv("notch.clipboard", clips);
      if (seg === "clip") drawClips();
    }
  } catch {}
}

// ---------- cuaca ----------
function wxIcon(code) {
  if (code === 0) return "☀";
  if (code <= 3) return "⛅";
  if (code <= 48) return "🌫";
  if (code <= 67) return "🌧";
  if (code <= 77) return "❄";
  if (code <= 82) return "🌦";
  if (code <= 86) return "❄";
  return "⛈";
}

async function loadWeather() {
  if (!T) return;
  try {
    const wx = await inv("get_weather");
    $("#wxIcon").textContent = wxIcon(wx.code);
    $("#wxTemp").textContent = Math.round(wx.temp) + "°C";
    $("#wxDesc").textContent = wx.desc;
    $("#wxCity").textContent = wx.city;
    $("#wxStrip").classList.add("loaded");
  } catch { /* silent — cuaca opsional */ }
}

$("#wxRefresh").onclick = async () => {
  $("#wxStrip").classList.remove("loaded");
  await loadWeather();
};

// ---------- next event di live pill ----------
function checkNextEvent() {
  const now = Date.now();
  const twoH = 2 * 3600_000;
  const next = evs.find(x => {
    const t = evStart(x).getTime();
    return t > now && t - now < twoH;
  });
  if (next) {
    const mins = Math.round((evStart(next).getTime() - now) / 60_000);
    S.calLive = {
      dot: "cal",
      proj: next.title.length > 28 ? next.title.slice(0, 27) + "…" : next.title,
      act: "Acara",
      meta: mins <= 1 ? "sekarang" : `${mins} mnt lagi`,
    };
  } else {
    S.calLive = null;
  }
  emit("pill");
}

on("tab", (t) => { if (t === "today") { loadCal(); loadWeather(); pollClip(); } });
on("open", () => { if (S.tab === "today") { loadCal(); loadWeather(); } pollClip(); });

export function initToday() {
  setSeg(seg);
  drawTd();
  drawF();
  drawCal();
  loadWeather();
  setInterval(pollClip, 2000);
  setInterval(checkNextEvent, 60_000); // refresh next-event pill tiap menit
}
