// Sistem: kecerahan, volume, Wi-Fi, Bluetooth, monitor CPU/RAM/jaringan, autostart, tautan pengaturan.
import { $, $$, S, inv, esc, icon, on, stack, bytes, toast, T, listen, press } from "./core.js";

const nav = stack("system");
let wOn = false, bOn = false, nets = [], pick = null;

// ---------- slider isi-penuh ----------
function slider(wrap, input, val, cmd) {
  const set = () => { $(wrap).style.setProperty("--v", input.value + "%"); $(val).textContent = input.value + "%"; };
  let t;
  input.addEventListener("input", () => {
    set();
    clearTimeout(t);
    t = setTimeout(() => inv(cmd, { level: +input.value }).catch((e) => toast(String(e), "err")), 30);
  });
  return { set: (v) => { input.value = v; set(); }, input };
}
const Br = slider("#sBrW", $("#sBr"), "#sBrV", "set_brightness");
const Vo = slider("#sVoW", $("#sVo"), "#sVoV", "set_volume");
async function hydrate() {
  try {
    const [v, b] = await inv("levels");
    Vo.set(Math.round(v * 100));
    if (b == null) { $("#sBrW").classList.add("off"); $("#sBrV").textContent = "N/A"; Br.input.disabled = true; }
    else { $("#sBrW").classList.remove("off"); Br.input.disabled = false; Br.set(b); }
  } catch {}
}
// Scroll roda mouse di slider = ±2
for (const [w, s] of [["#sBrW", Br], ["#sVoW", Vo]])
  $(w).addEventListener("wheel", (e) => { e.preventDefault(); if (s.input.disabled) return; s.input.value = Math.max(0, Math.min(100, +s.input.value + (e.deltaY < 0 ? 2 : -2))); s.input.dispatchEvent(new Event("input")); }, { passive: false });

// ---------- radio ----------
async function radios() {
  try {
    const r = await inv("radios");
    wOn = r.wifi_on; bOn = r.bt_on;
    $("#tWifi").classList.toggle("on", wOn);
    $("#tBt").classList.toggle("on", bOn);
    $("#tWifiS").textContent = !wOn ? "Mati" : r.wifi_ssid || "Tidak terhubung";
    $("#tBtS").textContent = !bOn ? "Mati" : r.bt_name ? r.bt_name + (r.bt_battery != null ? ` · ${r.bt_battery}%` : "") : "Nyala";
    $("#wfSw").classList.toggle("on", wOn);
    $("#btSw").classList.toggle("on", bOn);
  } catch {}
}
const toggleRadio = async (kind) => {
  try { await inv("set_radio", { kind, on: kind === "wifi" ? !wOn : !bOn }); } catch (e) { toast(String(e), "err"); }
  radios();
  setTimeout(() => { radios(); if (nav.cur === "wifi") loadWifi(); if (nav.cur === "bt") loadBt(); }, 1800);
};
press($("#tWifiBtn"), () => toggleRadio("wifi"), () => { nav.go("wifi"); loadWifi(); });
press($("#tBtBtn"), () => toggleRadio("bluetooth"), () => { nav.go("bt"); loadBt(); });
$("#tWifiMore").onclick = () => { nav.go("wifi"); loadWifi(); };
$("#tBtMore").onclick = () => { nav.go("bt"); loadBt(); };
$("#wfSw").onclick = () => toggleRadio("wifi");
$("#btSw").onclick = () => toggleRadio("bluetooth");
$("#wfRescan").onclick = () => loadWifi();

const bars = (n) => `<span class="bars">${[1, 2, 3, 4].map((i) => `<i class="${i <= n ? "f" : ""}" style="height:${i * 3}px"></i>`).join("")}</span>`;
async function loadWifi() {
  $("#wfStat").textContent = "Memindai…";
  $("#wfPw").hidden = true;
  try { nets = await inv("wifi_list"); } catch (e) { $("#wfList").innerHTML = `<div class="empty">${esc(e)}</div>`; $("#wfStat").textContent = ""; return; }
  const c = nets.find((x) => x.connected);
  $("#wfStat").textContent = c ? "Terhubung ke " + c.ssid : wOn ? `${nets.length} jaringan` : "Wi-Fi mati";
  $("#wfList").innerHTML = nets.map((x, i) => `<button class="net${x.connected ? " on" : ""}" data-i="${i}"><span class="n">${esc(x.ssid)}</span>${x.connected ? "<small>Terhubung</small>" : ""}${x.secure ? icon("lock") : ""}${bars(x.bars)}</button>`).join("") || `<div class="empty">${icon("wifi")}Tidak ada jaringan</div>`;
}
$("#wfList").onclick = (e) => { const b = e.target.closest(".net"); if (b) connect(nets[+b.dataset.i]); };
async function connect(x, pw) {
  if (!x || x.connected) return;
  $("#wfStat").textContent = "Menghubungkan ke " + x.ssid + "…";
  try {
    await inv("wifi_connect", { ssid: x.ssid, password: pw || null });
    $("#wfPw").hidden = true; pick = null;
    toast("Terhubung ke " + x.ssid);
    loadWifi(); radios();
  } catch (e) {
    if (x.secure && !pw) { pick = x; $("#wfPw").hidden = false; $("#wfPwI").focus(); $("#wfStat").textContent = "Kata sandi untuk " + x.ssid; }
    else $("#wfStat").textContent = "Gagal: " + e;
  }
}
const sendPw = () => { const v = $("#wfPwI").value; if (pick && v) { $("#wfPwI").value = ""; connect(pick, v); } };
$("#wfPwB").onclick = sendPw;
$("#wfPwI").onkeydown = (e) => e.key === "Enter" && sendPw();

let bts = [];
async function loadBt() {
  $("#btStat").textContent = "Memuat perangkat…";
  try { bts = await inv("bt_list"); } catch (e) { $("#btList").innerHTML = `<div class="empty">${esc(e)}</div>`; $("#btStat").textContent = ""; return; }
  $("#btStat").textContent = bOn ? `${bts.filter((d) => d.connected).length} terhubung` : "Bluetooth mati";
  $("#btList").innerHTML = bts.map((d, i) => `<button class="net${d.connected ? " on" : ""}" data-i="${i}"><span class="n">${esc(d.name)}</span><small>${d.connected ? "Terhubung" + (d.battery != null ? ` · ${d.battery}%` : "") : "Tidak terhubung"}</small></button>`).join("") || `<div class="empty">${icon("bt")}Tidak ada perangkat tersimpan</div>`;
}
$("#btList").onclick = async (e) => {
  const b = e.target.closest(".net"); if (!b) return;
  const d = bts[+b.dataset.i];
  if (d.connected) return inv("open_settings", { uri: "ms-settings:bluetooth" }).catch(() => {});
  $("#btStat").textContent = "Menghubungkan ke " + d.name + "…";
  try { await inv("bt_connect", { name: d.name }); setTimeout(() => { radios(); loadBt(); }, 2000); }
  catch (err) { $("#btStat").textContent = "Gagal: " + err; }
};

// ---------- monitor ----------
const hist = { cpu: [], net: [] };
let lastNet = null;
function spark(id, arr, max) {
  const m = Math.max(max || 0, ...arr, 1);
  const pts = arr.map((v, i) => `${(i / 29) * 60},${18 - (v / m) * 16 - 1}`).join(" ");
  $(`#${id} polyline`).setAttribute("points", pts);
}
async function stats() {
  try {
    const s = await inv("system_stats");
    const now = performance.now();
    hist.cpu = [...hist.cpu, s.cpu].slice(-30);
    $("#sCpu").textContent = Math.round(s.cpu) + "%";
    spark("spCpu", hist.cpu, 100);
    const mem = s.memory_total ? (s.memory_used / s.memory_total) * 100 : 0;
    $("#sRam").textContent = Math.round(mem) + "%";
    $("#sRamBar").style.width = mem + "%";
    $("#sRam").title = `${bytes(s.memory_used)} / ${bytes(s.memory_total)}`;
    if (lastNet) {
      const dt = (now - lastNet.t) / 1000;
      const rx = Math.max(0, (s.net_rx - lastNet.rx) / dt), tx = Math.max(0, (s.net_tx - lastNet.tx) / dt);
      hist.net = [...hist.net, rx + tx].slice(-30);
      $("#sNet").textContent = `↓${bytes(rx)}/s`;
      $("#sNet").title = `↓ ${bytes(rx)}/s  ↑ ${bytes(tx)}/s`;
      spark("spNet", hist.net);
    }
    lastNet = { rx: s.net_rx, tx: s.net_tx, t: now };
  } catch { $("#sCpu").textContent = "—"; }
}

// ---------- autostart ----------
let auto = false;
const drawAuto = () => $("#sAutoSw").classList.toggle("on", auto);
async function toggleAuto() {
  try { await inv("set_autostart", { enabled: !auto }); auto = !auto; drawAuto(); toast(auto ? "Autostart aktif" : "Autostart dimatikan"); }
  catch (e) { toast(String(e), "err"); }
}
$("#sAuto").onclick = toggleAuto;
$$(".links [data-uri]").forEach((b) => (b.onclick = () => inv("open_settings", { uri: b.dataset.uri }).catch(() => {})));

on("esc", (ev) => { if (S.tab === "system" && nav.back()) ev.handled = true; });
on("tab", (t) => { if (t === "system") { hydrate(); radios(); stats(); } });
on("open", () => { if (S.tab === "system") { hydrate(); radios(); } });

export function initSystem() {
  if (!T) return;
  inv("get_autostart").then((v) => { auto = !!v; drawAuto(); }).catch(() => {});
  listen("tray-autostart", toggleAuto);
  radios();
  setInterval(() => { if (S.open && S.tab === "system") stats(); }, 2000);
  setInterval(() => { if (S.open && S.tab === "system" && nav.cur === "main") radios(); }, 8000);
}
