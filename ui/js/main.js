// Perakitan: navigasi tab, pill (idle/live), event jendela, hotkey, jam.
import { $, $$, S, T, inv, listen, on, emit, layout, setOpen, setTab } from "./core.js";
import { initAgents } from "./agents.js";
import { initMedia } from "./media.js";
import { initSystem } from "./system.js";
import { initToday } from "./today.js";

const B = document.body;
if (T) B.classList.add("app");

// ---------- pill ----------
function renderPill() {
  const L = S.agentLive || S.calLive || S.focusLive || null;
  S.live = L;
  if (L) {
    $("#liveDot").className = "ldot " + L.dot;
    $("#liveProj").textContent = L.proj;
    $("#liveAct").textContent = L.act;
    $("#liveMeta").textContent = L.meta;
  }
  $("#idleDot").classList.toggle("on", !!S.idleDot);
  layout();
}
on("pill", renderPill);

// ---------- jam ----------
function clock() {
  const d = new Date();
  const hm = d.toLocaleTimeString("id-ID", { hour: "2-digit", minute: "2-digit" }).replace(".", ":");
  $("#clkT").textContent = hm;
  $("#clkD").textContent = d.toLocaleDateString("id-ID", { weekday: "short", day: "numeric", month: "short" });
  $("#idleTxt").textContent = hm;
}
clock();
setInterval(clock, 5000);

// ---------- interaksi notch ----------
$$("#tabs button").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); setTab(b.dataset.tab); }));
$('.lay[data-lay="idle"]').onclick = () => setOpen(true);
$('.lay[data-lay="live"]').onclick = () => {
  if (S.agentLive) emit("openAgentLive");
  else setOpen(true, "today");
};
$('.lay[data-lay="toast"]').onclick = () => setOpen(true);
$('.lay[data-lay="alert"]').onclick = (e) => { if (!e.target.closest("button")) emit("openAgentLive"); };

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  const ev = { handled: false };
  if (S.open) emit("esc", ev);
  if (!ev.handled) setOpen(false);
});

// Tanpa Tauri (pratinjau di browser): hover lewat mouse biasa.
const shell = $("#shell");
shell.addEventListener("mouseenter", () => { if (!T) { S.hover = true; layout(); } });
shell.addEventListener("mouseleave", () => { if (!T) { S.hover = false; layout(); } });
document.addEventListener("mousedown", (e) => { if (!T && S.open && !shell.contains(e.target)) setOpen(false); });

if (T) {
  listen("hover", (e) => { S.hover = !!e.payload; if (!S.open) layout(); });
  listen("click-outside", () => S.open && !S.modal && setOpen(false));
  listen("window-blur", () => { if (S.open && !S.hover && !S.modal) setOpen(false); });
  listen("browser-under-notch", (e) => { S.browser = !!e.payload; layout(); });
  listen("global-hotkey", () => {
    if (S.open) setOpen(false);
    else { setOpen(true); inv("focus_window").catch(() => {}); }
  });
  listen("tray-show", () => setOpen(true));
}

// ---------- mulai ----------
setTab(S.tab);
initAgents();
initMedia();
initSystem();
initToday();
renderPill();
// Indikator tab perlu diukur setelah font termuat.
document.fonts?.ready.then(() => setTab(S.tab));
