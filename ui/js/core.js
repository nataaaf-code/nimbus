// Inti: helper, jembatan Tauri, mesin state notch (mode, ukuran, hit-test), event bus.
export const $ = (s, r = document) => r.querySelector(s);
export const $$ = (s, r = document) => [...r.querySelectorAll(s)];
export const T = window.__TAURI__;
export const inv = (c, a) => (T ? T.core.invoke(c, a) : Promise.reject("Jalankan sebagai aplikasi Windows"));
export const listen = (ev, fn) => (T ? T.event.listen(ev, fn) : Promise.resolve(() => {}));
export const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => "&#" + c.charCodeAt(0) + ";");
export const ld = (k, d) => { try { return JSON.parse(localStorage.getItem(k)) ?? d; } catch { return d; } };
export const sv = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch {} };
export const icon = (id, cls = "") => `<svg class="i ${cls}"><use href="#i-${id}"/></svg>`;
export const mmss = (s) => { s = Math.max(0, Math.round(s)); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); };
export const ago = (ms) => {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 10) return "baru saja";
  if (s < 60) return Math.round(s) + " dtk";
  if (s < 3600) return Math.round(s / 60) + " mnt";
  if (s < 86400) return Math.round(s / 3600) + " jam";
  return Math.round(s / 86400) + " hr";
};
export const dur = (ms) => {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return s + "d";
  if (s < 3600) return Math.floor(s / 60) + "m " + (s % 60) + "d";
  return Math.floor(s / 3600) + "j " + Math.floor((s % 3600) / 60) + "m";
};
export const ntok = (v) => {
  v = Number(v) || 0;
  if (v >= 1e6) return (v / 1e6).toLocaleString("id-ID", { maximumFractionDigits: 1 }) + " jt";
  if (v >= 1e3) return (v / 1e3).toLocaleString("id-ID", { maximumFractionDigits: 1 }) + " rb";
  return String(v);
};
export const bytes = (v) => (v >= 1e9 ? (v / 1e9).toFixed(1) + " GB" : v >= 1e6 ? (v / 1e6).toFixed(1) + " MB" : v >= 1e3 ? Math.round(v / 1e3) + " KB" : Math.round(v) + " B");
export const startOfDay = () => { const d = new Date(); d.setHours(0, 0, 0, 0); return d.getTime(); };

// ---------- event bus ----------
const subs = {};
export const on = (ev, fn) => (subs[ev] ||= []).push(fn);
export const emit = (ev, d) => (subs[ev] || []).forEach((f) => { try { f(d); } catch (e) { console.error(ev, e); } });

// ---------- state notch ----------
const B = document.body;
const shell = $("#shell");
export const S = {
  open: false,
  tab: ld("notch.tab", "agents"),
  hover: false,
  browser: false,
  toast: null, // {text, kind, until}
  live: null, // {dot, proj, act, meta}
  alert: null, // izin pertama yang menunggu
  idleText: "",
  idleDot: false,
};
const SIZES = {
  line: [150, 5, 3],
  idle: [200, 34, 17],
  hover: [224, 38, 19],
  live: [400, 38, 19],
  toast: [380, 38, 19],
  alert: [480, 126, 26],
};
const TAB_H = { agents: 480, media: 330, system: 330, today: 410 };

export function mode() {
  if (S.open) return "open";
  if (S.alert) return "alert";
  if (S.toast && S.toast.until > Date.now()) return "toast";
  if (S.live) return "live";
  if (S.browser && !S.hover) return "line";
  return S.hover ? "hover" : "idle";
}

let lastKey = "";
export function layout() {
  const m = mode();
  const [w, h, r] = m === "open" ? [680, TAB_H[S.tab] || 420, 28] : SIZES[m];
  B.dataset.mode = m;
  const lay = m === "hover" || m === "line" ? "idle" : m;
  $$(".lay").forEach((l) => l.classList.toggle("on", l.dataset.lay === lay));
  shell.style.setProperty("--w", w + "px");
  shell.style.setProperty("--h", h + "px");
  shell.style.setProperty("--r", r + "px");
  document.documentElement.style.setProperty("--r", r + "px");
  const key = `${m}:${w}:${h}`;
  if (key !== lastKey) {
    lastKey = key;
    syncHit(w, h, m);
  }
}

let hitTimer;
function syncHit(w, h, m) {
  if (!T) return;
  const W = window.innerWidth;
  // Area hit: notch (plus sedikit margin bawah saat garis tipis supaya mudah di-hover).
  const hh = m === "line" ? 14 : h;
  const ww = m === "line" ? 220 : w;
  const send = () => inv("set_hit", { rects: [[(W - ww) / 2, 0, ww, hh]], open: S.open }).catch(() => {});
  clearTimeout(hitTimer);
  // Saat mengecil, tunggu animasi selesai agar kursor tidak "jatuh" ke aplikasi di bawah di tengah animasi.
  send();
  hitTimer = setTimeout(send, 600);
}

export function setOpen(v, tab) {
  if (tab) setTab(tab);
  if (S.open === v) return layout();
  S.open = v;
  if (v) S.toast = null;
  else $("#dMenu")?.classList.remove("on");
  layout();
  emit(v ? "open" : "close");
}
export function setTab(t) {
  if (!TAB_H[t]) return;
  S.tab = t;
  sv("notch.tab", t);
  $$(".view").forEach((v) => v.classList.toggle("on", v.dataset.view === t));
  const btn = $(`#tabs button[data-tab="${t}"]`);
  $$("#tabs button").forEach((b) => b.classList.toggle("on", b === btn));
  if (btn) {
    const ind = $("#tabInd");
    ind.style.width = btn.offsetWidth + "px";
    ind.style.transform = `translateX(${btn.offsetLeft}px)`;
  }
  layout();
  emit("tab", t);
}
export function toast(text, kind = "ok", ms = 3800) {
  S.toast = { text, kind, until: Date.now() + ms };
  const ic = $("#toastIcon");
  ic.className = "ticon " + kind;
  ic.innerHTML = icon(kind === "ok" ? "check" : kind === "err" ? "x" : "warn");
  $("#toastTxt").textContent = text;
  layout();
  setTimeout(layout, ms + 20);
}

// ---------- stack halaman (sub-halaman di dalam view) ----------
export function stack(view) {
  const root = $(`.view[data-view="${view}"]`);
  const pages = $$(".page", root);
  let hist = [pages[0]?.dataset.page];
  const show = (name, back) => {
    pages.forEach((p) => {
      const on = p.dataset.page === name;
      p.classList.toggle("on", on);
      p.classList.toggle("left", !on && hist.includes(p.dataset.page));
    });
    emit(`page:${view}`, name);
  };
  if (pages.length) show(hist[0]);
  $$("[data-back]", root).forEach((b) => (b.onclick = () => api.back()));
  const api = {
    get cur() { return hist[hist.length - 1]; },
    go(name) { if (api.cur === name) return; hist.push(name); show(name, false); },
    back() { if (hist.length < 2) return false; hist.pop(); show(api.cur, true); return true; },
    root() { hist = [hist[0]]; show(hist[0], true); },
  };
  return api;
}

// ---------- long press ----------
export function press(el, onShort, onLong, ms = 420) {
  let t, fired = false;
  el.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    fired = false;
    t = setTimeout(() => { fired = true; onLong(); }, ms);
  });
  const cancel = () => clearTimeout(t);
  el.addEventListener("pointerleave", cancel);
  el.addEventListener("pointerup", () => { cancel(); if (!fired) onShort(); });
}
