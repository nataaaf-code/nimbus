// Hari ini: Google Calendar, Tugas, Catatan, Fokus (pomodoro), riwayat Clipboard.
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
let sel = 0, evs = [];
const day = (i) => { const d = new Date(); d.setHours(0, 0, 0, 0); d.setDate(d.getDate() + i); return d; };
const evStart = (x) => new Date(x.start.length === 10 ? x.start + "T00:00:00" : x.start);
async function loadCal() {
  let st = { connected: false, has_creds: false };
  try { st = await inv("gcal_status"); } catch {}
  const lock = $("#calLock");
  lock.hidden = st.connected;
  if (!st.connected) {
    $("#calLockT").textContent = T ? (st.has_creds ? "Google Calendar belum terhubung" : "Masukkan kredensial OAuth Google") : "Jalankan sebagai aplikasi Windows";
    $("#calForm").hidden = st.has_creds;
    $("#gcSave").hidden = st.has_creds;
    $("#gcLogin").hidden = !st.has_creds;
    drawCal();
    return;
  }
  try { evs = await inv("gcal_events", { min: day(0).toISOString(), max: day(7).toISOString() }); }
  catch (e) { $("#calEvs").innerHTML = `<div class="empty">${esc(e)}</div>`; return; }
  drawCal();
}
function drawCal() {
  $("#calDate").textContent = day(sel).toLocaleDateString("id-ID", { weekday: "long", day: "numeric", month: "long" });
  const has = (i) => evs.some((x) => { const t = evStart(x).getTime(); return t >= day(i).getTime() && t < day(i + 1).getTime(); });
  $("#calWeek").innerHTML = [0, 1, 2, 3, 4, 5, 6].map((i) => `<button class="${i === sel ? "on" : ""}${has(i) ? " has" : ""}" data-d="${i}">${day(i).toLocaleDateString("id-ID", { weekday: "short" })}<b>${day(i).getDate()}</b></button>`).join("");
  const t0 = day(sel).getTime();
  const r = evs.filter((x) => { const t = evStart(x).getTime(); return t >= t0 && t < t0 + 864e5; });
  const nowT = Date.now();
  const nextIdx = sel === 0 ? r.findIndex((x) => evStart(x).getTime() >= nowT) : -1;
  $("#calEvs").innerHTML = r.length
    ? r.map((x, i) => `<div class="ev${i === nextIdx ? " nx" : ""}"><time>${x.start.length === 10 ? "Seharian" : evStart(x).toLocaleTimeString("id-ID", { hour: "2-digit", minute: "2-digit" })}</time><span>${esc(x.title)}</span></div>`).join("")
    : `<div class="empty">${icon("cal")}Tidak ada acara</div>`;
}
$("#calWeek").onclick = (e) => { const b = e.target.closest("[data-d]"); if (b) { sel = +b.dataset.d; drawCal(); } };
$("#gcLogin").onclick = async () => {
  const b = $("#gcLogin"); b.textContent = "Menunggu login di browser…"; b.disabled = true;
  try { await inv("gcal_login"); loadCal(); } catch (e) { toast(String(e), "err"); }
  b.textContent = "Hubungkan Google"; b.disabled = false;
};
$("#gcSave").onclick = async () => {
  const id = $("#gcId").value.trim(), secret = $("#gcSec").value.trim();
  if (!id || !secret) return toast("Isi Client ID dan Client Secret", "warn");
  const b = $("#gcSave"); b.disabled = true; b.textContent = "Menunggu login di browser…";
  try { await inv("gcal_save_creds", { id, secret }); await inv("gcal_login"); loadCal(); } catch (e) { toast(String(e), "err"); }
  b.disabled = false; b.textContent = "Simpan & login";
};
$("#gcFolder").onclick = () => inv("gcal_open_folder").catch((e) => toast(String(e), "err"));

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

on("tab", (t) => { if (t === "today") { loadCal(); pollClip(); } });
on("open", () => { if (S.tab === "today") loadCal(); pollClip(); });

export function initToday() {
  setSeg(seg);
  drawTd();
  drawF();
  drawCal();
  setInterval(pollClip, 2000);
}
