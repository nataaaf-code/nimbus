// Monitor & kontrol Multi-Agent ala T3 Code: Claude Code, OpenCode, Antigravity, Codex, Gemini CLI.
// Fitur: daftar sesi terpadu, filter provider, install & login di dalam notch, timeline live, gerbang izin,
// rename & hapus sesi terintegrasi, lampiran, model/effort/mode dinamis, dan fork.
import { $, $$, S, T, inv, listen, esc, icon, ago, dur, ntok, bytes, startOfDay, on, emit, setOpen, toast, stack, ld, sv, layout } from "./core.js";
import { md } from "./md.js";

const nav = stack("agents");
let snap = null;
let hooks = null;
let detail = null; // { id, provider }
let dVer = 0;
const openTools = new Set();
const expandedText = new Set();
let pendingRun = null; // { run, provider }
let provFilter = "all";
let newProv = ld("notch.newProv", "claude");
let searchQ = "";
let listRenameKey = null; // `${provider}:${id}`
let listDelKey = null;
let listDelTimer = null;
let menuDelConfirm = false;
let menuDelTimer = null;
let dFork = false;
let dAtts = [];
let nAtts = [];

// State halaman Providers (Install & Login di dalam notch)
let provCache = [];
let provErr = null;
let provLoading = false;
let keyEditProv = null;
const setupLogs = {}; // { [prov]: string[] }
const setupBusy = {}; // { [prov]: "install" | "login" | null }

const PROV_LBL = {
  claude: "Claude",
  opencode: "OpenCode",
  antigravity: "Antigravity",
  codex: "Codex",
  gemini: "Gemini",
};
const STATE_TXT = { running: "Berjalan", waiting: "Butuh izin", idle: "Siap", done: "Selesai" };
const STATE_CLS = { running: "run", waiting: "wait", idle: "idle", done: "done" };
const TOOL_IC = {
  Read: "file", Write: "edit", Edit: "edit", MultiEdit: "edit", NotebookEdit: "edit",
  Bash: "term", PowerShell: "term", shell: "term",
  Grep: "search", Glob: "search", WebFetch: "globe", WebSearch: "globe",
  TodoWrite: "list", Task: "bot", Agent: "bot",
};

const sKey = (s) => `${s?.provider || "claude"}:${s?.id || ""}`;
const sessByKey = (prov, id) => snap?.sessions.find((s) => (s.provider || "claude") === (prov || "claude") && s.id === id);
const curSess = () => (detail ? sessByKey(detail.provider, detail.id) : null);
const provInfo = (id) => (snap?.providers || provCache).find((p) => p.id === id) || null;
const short = (s, n = 60) => (s && s.length > n ? s.slice(0, n - 1) + "…" : s || "");
const fileBase = (p) => String(p || "").split(/[\\/]/).pop() || p;

// Jangan biarkan satu command backend yang macet membekukan seluruh UI.
function withTimeout(p, ms, msg) {
  let t;
  return Promise.race([
    p,
    new Promise((_, rej) => { t = setTimeout(() => rej(new Error(msg)), ms); }),
  ]).finally(() => clearTimeout(t));
}

// Preferensi model / effort / mode per sesi
function loadCfg(prov, id) {
  const all = ld("notch.sessCfg", {});
  return all[`${prov}:${id}`] || all[id] || { model: "default", effort: "default", mode: "default" };
}
function saveCfg(prov, id, patch) {
  if (!id) return;
  const all = ld("notch.sessCfg", {});
  const k = `${prov}:${id}`;
  all[k] = { ...(all[k] || { model: "default", effort: "default", mode: "default" }), ...patch };
  const keys = Object.keys(all);
  if (keys.length > 100) delete all[keys[0]];
  sv("notch.sessCfg", all);
}

function populateModelSelect(selEl, provId, currentVal) {
  const info = provInfo(provId);
  const models = info?.models?.length ? info.models : (provId === "claude" ? ["opus", "sonnet", "haiku"] : []);
  const want = currentVal || "default";
  const opts = ['<option value="default">Model: Default</option>'];
  const seen = new Set(["default"]);
  if (want && !seen.has(want) && !models.includes(want)) {
    seen.add(want);
    opts.push(`<option value="${esc(want)}">${esc(want)}</option>`);
  }
  for (const m of models) {
    if (!m || seen.has(m)) continue;
    seen.add(m);
    const label = m.replace(/^claude-/, "");
    opts.push(`<option value="${esc(m)}">${esc(label)}</option>`);
  }
  const html = opts.join("");
  if (selEl._h !== html) {
    selEl.innerHTML = html;
    selEl._h = html;
  }
  selEl.value = seen.has(want) ? want : "default";
}

// ======================================================================
// Polling
// ======================================================================
let pollT, busy = false, again = false;
async function poll() {
  if (busy) { again = true; return; }
  busy = true;
  clearTimeout(pollT);
  try {
    snap = await withTimeout(inv("ag_snapshot", { since: startOfDay() }), 20000, "Backend agent tidak merespons (timeout 20 detik)");
    if (Array.isArray(snap?.providers) && snap.providers.length) provCache = snap.providers;
    render();
  } catch (e) {
    if (!T || !snap) renderOffline(e);
  }
  busy = false;
  if (again) { again = false; return poll(); }
  const fast = S.open && S.tab === "agents";
  pollT = setTimeout(poll, fast ? 1000 : snap?.sessions.some((s) => s.state === "running") ? 1500 : 2500);
}
let kickT;
const kick = (ms = 90) => { clearTimeout(kickT); kickT = setTimeout(poll, ms); };

function render() {
  renderPill();
  renderBadge();
  updateProvidersBtn();
  if (S.open && S.tab === "agents") {
    if (nav.cur === "list") renderList();
    if (nav.cur === "detail") renderDetailHead();
    if (nav.cur === "providers") renderProviders();
    if (nav.cur === "new") renderNewProviderState();
  }
}

function renderOffline(e) {
  $("#sessList").innerHTML = `<div class="empty">${icon(T ? "warn" : "plug")}<b>${T ? "Gagal memuat sesi" : "Mode pratinjau"}</b><span>${esc(e)}</span></div>`;
}

function updateProvidersBtn() {
  const list = provCache.length ? provCache : snap?.providers || [];
  if (!list.length) return;
  const ready = list.filter((p) => p.installed && p.logged_in).length;
  $("#agProvCount").textContent = `${ready}/${list.length} siap`;
}

// ======================================================================
// Pill: alert izin, live activity, idle
// ======================================================================
function renderPill() {
  const pend = snap?.pending || [];
  const first = pend.slice().sort((a, b) => a.created - b.created)[0] || null;
  S.alert = first;
  if (first) {
    $("#alProj").textContent = first.project || "";
    $("#alTool").textContent = first.tool;
    $("#alSum").textContent = first.sum || first.tool;
    $("#alCount").textContent = pend.length > 1 ? `1/${pend.length}` : "";
  }
  const run = (snap?.sessions || []).filter((s) => s.state === "running" || s.state === "waiting");
  if (run.length) {
    const s = run.slice().sort((a, b) => b.last - a.last)[0];
    const pl = PROV_LBL[s.provider] || s.provider || "";
    S.agentLive = {
      dot: s.state === "waiting" ? "wait" : "run",
      proj: s.project || pl || "agent",
      act: s.activity || "Berjalan…",
      meta: run.length > 1 ? `+${run.length - 1}` : s.turn ? dur(Date.now() - s.turn) : "",
      id: s.id,
      provider: s.provider || "claude",
    };
  } else S.agentLive = null;
  const open = (snap?.sessions || []).filter((s) => s.state === "idle").length;
  S.idleDot = open > 0;
  emit("pill");
}

function renderBadge() {
  const b = $("#tbAgents");
  const pend = snap?.pending.length || 0;
  const run = snap?.sessions.filter((s) => s.state === "running").length || 0;
  b.textContent = pend || run || "";
  b.className = "tbadge" + (pend || run ? " on" : "") + (pend ? " warn" : "");
}

// ======================================================================
// Daftar sesi (multi-provider)
// ======================================================================
function cardHtml(s) {
  const prov = s.provider || "claude";
  const key = sKey(s);
  const st = STATE_CLS[s.state];
  const p = s.proc;
  const tags = [`<span class="ptag p-${esc(prov)}">${esc(PROV_LBL[prov] || prov)}</span>`];
  if (s.pending) tags.push(`<span class="tag warn">${s.pending} izin</span>`);
  if (p?.host) tags.push(`<span class="tag">${esc(p.host)}</span>`);
  else if (s.managed) tags.push(`<span class="tag acc">Notch</span>`);
  if (s.subagent) tags.push(`<span class="tag">subagent</span>`);
  const act = s.state === "running" || s.state === "waiting" ? s.activity : s.state === "idle" ? s.note || "Menunggu instruksi" : s.prompt;
  const renaming = listRenameKey === key;
  const delConfirm = listDelKey === key;
  const titleHtml = renaming
    ? `<input class="rename-in" data-ren-in="${esc(key)}" value="${esc(s.title)}" autocomplete="off" />`
    : `<b>${esc(s.title)}</b>`;
  const actsHtml = renaming
    ? ""
    : `<span class="sess-acts${delConfirm ? " pin" : ""}">
        <button class="sess-act" data-ren="${esc(key)}" title="Ganti nama sesi">${icon("edit")}</button>
        <button class="sess-act del${delConfirm ? " confirm" : ""}" data-del="${esc(key)}" title="${delConfirm ? "Klik lagi untuk hapus permanen" : "Hapus sesi"}">${delConfirm ? "Yakin?" : icon("trash")}</button>
      </span>`;
  return `<span class="sdot ${st}"></span>
    <div class="sbody">
      <div class="sr1">${titleHtml}${actsHtml}<time>${ago(s.last)}</time></div>
      <div class="sr2">${tags.join("")}<span>${esc(s.project || "—")}</span>${s.branch ? `<span class="sep">·</span>${esc(s.branch)}` : ""}${s.tok?.out ? `<span class="sep">·</span>${ntok(s.tok.out)} keluar` : ""}</div>
      ${act ? `<div class="sact ${st}">${esc(short(act, 110))}</div>` : ""}
    </div>`;
}

function matchQuery(s, q) {
  const prov = s.provider || "claude";
  if (provFilter !== "all" && prov !== provFilter) return false;
  if (!q) return true;
  const hay = `${s.title || ""} ${s.project || ""} ${s.branch || ""} ${s.prompt || ""} ${s.cwd || ""} ${prov} ${PROV_LBL[prov] || ""}`.toLowerCase();
  return q.split(/\s+/).every((w) => hay.includes(w));
}

function renderList() {
  const box = $("#sessList");
  if (!snap) return;
  $("#agToday").textContent = `Hari ini · ${snap.today.sessions} sesi · ${ntok(snap.today.tokens)} token`;
  const q = searchQ.trim().toLowerCase();
  const filtered = snap.sessions.filter((s) => matchQuery(s, q));
  const groups = [
    ["waiting", "Butuh kamu"],
    ["running", "Berjalan"],
    ["idle", "Terbuka"],
    ["done", "Terbaru"],
  ];
  const want = [];
  for (const [st, label] of groups) {
    const ss = filtered.filter((s) => s.state === st).slice(0, st === "done" && !q && provFilter === "all" ? 25 : 60);
    if (!ss.length) continue;
    want.push({ key: "g:" + st, cls: "grp", html: label });
    for (const s of ss) want.push({ key: "s:" + sKey(s), cls: "sess " + s.state, html: cardHtml(s), id: s.id, prov: s.provider || "claude", tag: "div" });
  }
  const orphans = q || provFilter !== "all" ? [] : snap.procs.filter((p) => !p.session);
  if (orphans.length) {
    want.push({ key: "g:proc", cls: "grp", html: "Proses agent lain" });
    for (const p of orphans)
      want.push({
        key: "p:" + p.pid,
        cls: "proc",
        html: `${icon("bot")}<span><b>PID ${p.pid}</b> · ${esc(p.host)} · CPU ${p.cpu}% · ${bytes(p.mem)}${p.children.length ? " · " + esc(p.children.map((c) => c.name).join(", ")) : ""}</span><span class="grow"></span><button class="btn sm danger" data-kill="${p.pid}">Stop</button>`,
      });
  }
  const failed = q ? [] : snap.runs.filter((r) => r.done && r.error && Date.now() - r.started < 15 * 60e3);
  for (const r of failed)
    want.push({ key: "r:" + r.run, cls: "proc", html: `${icon("warn")}<span><b>Run gagal${r.provider ? ` (${esc(PROV_LBL[r.provider] || r.provider)})` : ""}</b> · ${esc(short(r.error, 160))}</span>` });
  if (!want.length) {
    const pi = provFilter !== "all" ? provInfo(provFilter) : null;
    if (pi && !pi.installed) {
      box.innerHTML = `<div class="empty">${icon("bot")}<b>${esc(pi.name)} belum terpasang</b><span>Install &amp; login langsung dari notch lewat menu Providers.</span><button class="btn pri sm" id="emptyInstallBtn">Buka Providers</button></div>`;
      $("#emptyInstallBtn").onclick = () => openProviders();
      return;
    }
    box.innerHTML = q || provFilter !== "all"
      ? `<div class="empty">${icon("search")}<b>Tidak ada sesi yang cocok</b><span>Coba kata kunci lain atau ubah filter provider.</span></div>`
      : `<div class="empty">${icon("spark")}<b>Belum ada sesi dalam 3 hari terakhir</b><span>Klik Sesi baru, atau buka Providers untuk install &amp; login agent lain.</span></div>`;
    return;
  }
  box.querySelector(".empty")?.remove();
  const have = new Map($$("[data-key]", box).map((el) => [el.dataset.key, el]));
  let prev = null;
  for (const w of want) {
    let el = have.get(w.key);
    if (!el) {
      el = document.createElement(w.tag || "div");
      el.dataset.key = w.key;
      if (w.id) {
        el.setAttribute("role", "button");
        el.tabIndex = 0;
        el.onclick = (e) => {
          if (e.target.closest(".sess-acts, input")) return;
          openDetail(w.id, w.prov);
        };
        el.onkeydown = (e) => {
          if (e.target === el && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); openDetail(w.id, w.prov); }
        };
      }
    }
    have.delete(w.key);
    if (el.className !== w.cls) el.className = w.cls;
    const sk = w.id ? `${w.prov}:${w.id}` : "";
    const isEditingThis = w.id && listRenameKey === sk && el.querySelector("[data-ren-in]") === document.activeElement;
    if (!isEditingThis && el._h !== w.html) { el.innerHTML = w.html; el._h = w.html; }
    const next = prev ? prev.nextSibling : box.firstChild;
    if (next !== el) box.insertBefore(el, next);
    prev = el;
  }
  have.forEach((el) => el.remove());
  $$("[data-kill]", box).forEach((b) => (b.onclick = (e) => { e.stopPropagation(); killPid("claude", +b.dataset.kill); }));
  $$("[data-ren]", box).forEach((b) => (b.onclick = (e) => { e.stopPropagation(); startListRename(b.dataset.ren); }));
  $$("[data-del]", box).forEach((b) => (b.onclick = (e) => { e.stopPropagation(); handleListDelete(b.dataset.del); }));
  const renIn = box.querySelector("[data-ren-in]");
  if (renIn && !renIn._bound) {
    renIn._bound = true;
    const key = renIn.dataset.renIn;
    const [prov, ...rest] = key.split(":");
    const sid = rest.join(":");
    renIn.onclick = (e) => e.stopPropagation();
    renIn.onkeydown = async (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { e.preventDefault(); await commitRename(prov, sid, renIn.value); listRenameKey = null; renderList(); }
      if (e.key === "Escape") { listRenameKey = null; renderList(); }
    };
    renIn.onblur = () => { if (listRenameKey === key) { listRenameKey = null; renderList(); } };
    renIn.focus();
    renIn.select();
  }
}

function splitKey(k) {
  const i = k.indexOf(":");
  return i < 0 ? ["claude", k] : [k.slice(0, i), k.slice(i + 1)];
}
function startListRename(key) {
  listRenameKey = key;
  listDelKey = null;
  renderList();
}

async function handleListDelete(key) {
  const [prov, id] = splitKey(key);
  const s = sessByKey(prov, id);
  if (s && (s.pid || s.managed_live)) return toast("Hentikan proses sesi dulu sebelum menghapus", "warn");
  if (listDelKey !== key) {
    listDelKey = key;
    clearTimeout(listDelTimer);
    listDelTimer = setTimeout(() => { if (listDelKey === key) { listDelKey = null; renderList(); } }, 2800);
    renderList();
    return;
  }
  listDelKey = null;
  clearTimeout(listDelTimer);
  await deleteSessionById(prov, id);
}

async function commitRename(provider, id, rawTitle) {
  const title = String(rawTitle || "").trim();
  if (!title) return;
  try {
    await inv("ag_rename", { provider, id, title });
    const s = sessByKey(provider, id);
    if (s) s.title = title;
    toast("Nama sesi diperbarui");
    kick(80);
  } catch (e) { toast(String(e), "err"); }
}

async function deleteSessionById(provider, id) {
  try {
    await inv("ag_delete", { provider, id });
    if (snap) snap.sessions = snap.sessions.filter((x) => !((x.provider || "claude") === provider && x.id === id));
    if (detail && detail.provider === provider && detail.id === id) { detail = null; nav.root(); }
    toast("Sesi dihapus", "warn");
    render();
    kick(80);
  } catch (e) { toast(String(e), "err"); }
}

$("#agSearch").oninput = (e) => {
  searchQ = e.target.value;
  $("#agSearchClr").hidden = !searchQ;
  renderList();
};
$("#agSearchClr").onclick = () => {
  searchQ = "";
  $("#agSearch").value = "";
  $("#agSearchClr").hidden = true;
  renderList();
  $("#agSearch").focus();
};
$("#agProvFilter").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  provFilter = b.dataset.p;
  $$("#agProvFilter button").forEach((x) => x.classList.toggle("on", x === b));
  renderList();
});

async function killPid(provider, pid) {
  try { await inv("ag_kill", { provider, pid }); toast("Proses agent dihentikan", "warn"); } catch (e) { toast(String(e), "err"); }
  kick(50);
}

// ======================================================================
// Providers & Auth (Install + Login langsung di dalam notch)
// ======================================================================
async function openProviders(refresh = false) {
  nav.go("providers");
  provErr = null;
  provLoading = true;
  const refBtn = $("#pvRefreshBtn");
  if (refBtn) refBtn.disabled = true;
  renderProviders();
  try {
    const list = await withTimeout(
      inv("ag_providers", { refresh }),
      25000,
      "Cek provider tidak merespons (timeout 25 detik)",
    );
    if (Array.isArray(list)) provCache = list;
  } catch (e) {
    provErr = String(e);
    toast(provErr, "err");
  } finally {
    provLoading = false;
    if (refBtn) refBtn.disabled = false;
    renderProviders();
    updateProvidersBtn();
  }
}

function renderProviders() {
  const box = $("#pvList");
  const list = provCache.length ? provCache : snap?.providers || [];
  if (!list.length) {
    if (provErr && !provLoading) {
      box.innerHTML = `<div class="empty">${icon("warn")}<b>Gagal memuat provider</b><span>${esc(provErr)}</span><button class="btn pri sm" id="pvRetryBtn">Coba lagi</button></div>`;
      $("#pvRetryBtn").onclick = () => openProviders(false);
    } else if (provLoading) {
      box.innerHTML = `<div class="empty">${icon("bot")}<b>Memeriksa provider…</b><span>Mendeteksi CLI dan status login.</span></div>`;
    } else {
      box.innerHTML = `<div class="empty">${icon("warn")}<b>Belum ada data provider</b><span>Cek ulang untuk mendeteksi CLI yang terpasang.</span><button class="btn pri sm" id="pvRetryBtn">Cek sekarang</button></div>`;
      $("#pvRetryBtn").onclick = () => openProviders(true);
    }
    return;
  }
  // Jangan timpa DOM saat user sedang mengetik API key
  if (keyEditProv && box.querySelector(`[data-pv-key-in="${keyEditProv}"]`) === document.activeElement) return;

  box.innerHTML = list.map((p) => {
    const busyOp = setupBusy[p.id];
    const checking = !!p.checking;
    const logs = setupLogs[p.id] || [];
    const stCls = checking ? "check" : p.installed && p.logged_in ? "ok" : p.installed ? "warn" : "";
    const stTxt = busyOp === "install"
      ? "Memasang…"
      : busyOp === "login"
        ? "Login…"
        : checking
          ? p.checked_at ? "Memeriksa ulang…" : "Memeriksa…"
          : p.installed && p.logged_in
            ? `Siap${p.account ? " · " + p.account : ""}`
            : p.installed
              ? "Perlu Login"
              : "Belum Terpasang";
    const stTitle = checking && p.checked_at ? "Data terakhir tetap ditampilkan sampai pemeriksaan selesai." : "";
    const subTxt = p.installed
      ? `${p.bin}${p.models?.length ? ` · ${p.models.length} model` : ""}`
      : `Perintah install: ${p.install_hint}`;
    const acts = [];
    if (!p.installed) {
      acts.push(`<button class="btn sm pri" data-pv-install="${esc(p.id)}" ${busyOp ? "disabled" : ""}>Install CLI</button>`);
    } else {
      acts.push(`<button class="btn sm ${p.logged_in ? "" : "pri"}" data-pv-login="${esc(p.id)}" ${busyOp ? "disabled" : ""}>${p.logged_in ? "Login Ulang" : "Login Akun"}</button>`);
      if (p.supports_device_login) {
        acts.push(`<button class="btn sm" data-pv-dev="${esc(p.id)}" ${busyOp ? "disabled" : ""}>Device Code</button>`);
      }
    }
    acts.push(`<button class="btn sm" data-pv-key="${esc(p.id)}">${p.has_api_key ? "Ubah API Key" : "API Key"}</button>`);
    if (p.installed && p.id === "opencode") {
      acts.push(`<button class="btn sm" data-pv-models="${esc(p.id)}">Sync Model</button>`);
    }
    if (p.logged_in || p.has_api_key) {
      acts.push(`<button class="btn sm danger" data-pv-logout="${esc(p.id)}" ${busyOp ? "disabled" : ""}>Logout</button>`);
    }
    const keyRow = keyEditProv === p.id
      ? `<div class="pv-keyrow">
          <input class="field" type="password" data-pv-key-in="${esc(p.id)}" placeholder="Tempel ${esc(p.api_key_env)} di sini…" autocomplete="off" />
          <button class="btn sm pri" data-pv-key-save="${esc(p.id)}">Simpan</button>
          <button class="btn sm ghost" data-pv-key-cancel>Batal</button>
        </div>`
      : "";
    const logBox = logs.length ? `<pre class="pv-log" data-pv-log="${esc(p.id)}">${esc(logs.join("\n"))}</pre>` : "";
    return `<div class="pv-card">
      <div class="pv-head">
        <span class="ptag p-${esc(p.id)}">${esc(PROV_LBL[p.id] || p.id)}</span>
        <b>${esc(p.name)}</b>
        ${p.version ? `<span class="pv-ver">${esc(short(p.version, 28))}</span>` : ""}
        <span class="pv-st ${stCls}"${stTitle ? ` title="${esc(stTitle)}"` : ""}>${esc(short(stTxt, 38))}</span>
      </div>
      <div class="pv-sub" title="${esc(subTxt)}">${esc(subTxt)}</div>
      <div class="pv-acts">${acts.join("")}</div>
      ${keyRow}
      ${logBox}
    </div>`;
  }).join("");

  $$("[data-pv-install]", box).forEach((b) => (b.onclick = () => startInstall(b.dataset.pvInstall)));
  $$("[data-pv-login]", box).forEach((b) => (b.onclick = () => startOAuthLogin(b.dataset.pvLogin, null)));
  $$("[data-pv-dev]", box).forEach((b) => (b.onclick = () => startOAuthLogin(b.dataset.pvDev, "device")));
  $$("[data-pv-key]", box).forEach((b) => (b.onclick = () => {
    keyEditProv = keyEditProv === b.dataset.pvKey ? null : b.dataset.pvKey;
    renderProviders();
    box.querySelector(`[data-pv-key-in="${keyEditProv}"]`)?.focus();
  }));
  $$("[data-pv-key-cancel]", box).forEach((b) => (b.onclick = () => { keyEditProv = null; renderProviders(); }));
  $$("[data-pv-key-save]", box).forEach((b) => (b.onclick = () => submitApiKey(b.dataset.pvKeySave)));
  $$("[data-pv-key-in]", box).forEach((inp) => (inp.onkeydown = (e) => {
    if (e.key === "Enter") { e.preventDefault(); submitApiKey(inp.dataset.pvKeyIn); }
    if (e.key === "Escape") { e.stopPropagation(); keyEditProv = null; renderProviders(); }
  }));
  $$("[data-pv-logout]", box).forEach((b) => (b.onclick = async () => {
    try {
      const list = await withTimeout(
        inv("ag_logout_provider", { provider: b.dataset.pvLogout }),
        60000,
        "Logout tidak merespons (timeout 60 detik)",
      );
      if (Array.isArray(list)) provCache = list;
      toast("Logout berhasil", "warn");
      renderProviders();
      updateProvidersBtn();
    } catch (e) { toast(String(e), "err"); }
  }));
  $$("[data-pv-models]", box).forEach((b) => (b.onclick = async () => {
    b.disabled = true;
    try {
      const models = await withTimeout(
        inv("ag_refresh_models", { provider: b.dataset.pvModels }),
        90000,
        "Sinkronisasi model tidak merespons (timeout 90 detik)",
      );
      toast(`${Array.isArray(models) ? models.length : 0} model disinkronkan`);
      await openProviders(true);
    } catch (e) { toast(String(e), "err"); }
    b.disabled = false;
  }));
  // Auto-scroll log setup ke bawah
  $$(".pv-log", box).forEach((pre) => { pre.scrollTop = pre.scrollHeight; });
  const refBtn = $("#pvRefreshBtn");
  if (refBtn) refBtn.disabled = provLoading || list.some((p) => p.checking);
}

async function startInstall(provider) {
  setupBusy[provider] = "install";
  setupLogs[provider] = [`Menjalankan instalasi ${PROV_LBL[provider] || provider}…`];
  if (nav.cur === "providers") renderProviders();
  try {
    await inv("ag_install_provider", { provider });
  } catch (e) {
    setupBusy[provider] = null;
    setupLogs[provider].push(`Error: ${e}`);
    toast(String(e), "err");
    if (nav.cur === "providers") renderProviders();
  }
}

async function startOAuthLogin(provider, mode) {
  setupBusy[provider] = "login";
  setupLogs[provider] = [`Memulai login ${PROV_LBL[provider] || provider}…`];
  if (nav.cur === "providers") renderProviders();
  try {
    await inv("ag_login_oauth", { provider, mode });
  } catch (e) {
    setupBusy[provider] = null;
    setupLogs[provider].push(`Error: ${e}`);
    toast(String(e), "err");
    if (nav.cur === "providers") renderProviders();
  }
}

async function submitApiKey(provider) {
  const inp = $(`[data-pv-key-in="${provider}"]`);
  const key = inp?.value.trim() || "";
  if (!key) return toast("Masukkan API key dulu", "warn");
  try {
    const list = await withTimeout(
      inv("ag_save_key", { provider, key }),
      60000,
      "Menyimpan API key tidak merespons (timeout 60 detik)",
    );
    if (Array.isArray(list)) provCache = list;
    keyEditProv = null;
    toast(`API Key ${PROV_LBL[provider] || provider} disimpan`);
    renderProviders();
    updateProvidersBtn();
    renderNewProviderState();
  } catch (e) { toast(String(e), "err"); }
}

$("#agProvidersBtn").onclick = () => openProviders(false);
$("#pvRefreshBtn").onclick = () => openProviders(true);

listen("agent-setup", (e) => {
  const p = e.payload || {};
  const id = p.provider;
  if (!id) return;
  const arr = (setupLogs[id] ||= []);
  if (p.line) {
    arr.push(p.line);
    if (arr.length > 80) arr.shift();
  }
  if (p.done) {
    setupBusy[id] = null;
    if (p.ok) toast(`${PROV_LBL[id] || id}: ${p.op === "install" ? "terpasang" : "siap"}`);
    else if (p.error) toast(String(p.error), "err");
    inv("ag_providers", { refresh: false }).then((list) => {
      if (Array.isArray(list)) provCache = list;
      updateProvidersBtn();
      if (nav.cur === "providers") renderProviders();
      if (nav.cur === "new") renderNewProviderState();
    }).catch(() => {});
  }
  if (nav.cur === "providers") renderProviders();
});

// ======================================================================
// Lampiran & Textarea helper
// ======================================================================
function resizeBox(ta) {
  if (!ta) return;
  ta.style.height = "auto";
  ta.style.height = Math.min(96, Math.max(22, ta.scrollHeight)) + "px";
}

function renderAttRow(sel, list, onChange) {
  const row = $(sel);
  if (!row) return;
  row.hidden = !list.length;
  row.innerHTML = list.map((p, i) => `<span class="att-chip" title="${esc(p)}"><span>${esc(fileBase(p))}</span><button type="button" data-rm="${i}" title="Hapus lampiran">${icon("x")}</button></span>`).join("");
  $$("[data-rm]", row).forEach((b) => (b.onclick = (e) => {
    e.stopPropagation();
    list.splice(+b.dataset.rm, 1);
    onChange();
  }));
}
const renderDetailAtts = () => renderAttRow("#dAtts", dAtts, renderDetailAtts);
const renderNewAtts = () => renderAttRow("#nAtts", nAtts, renderNewAtts);

function addUniquePaths(list, paths) {
  let added = 0;
  for (const p of paths) {
    if (p && !list.includes(p)) { list.push(p); added++; }
  }
  return added;
}

async function pickFilesFor(list, redraw) {
  if (!T) return toast("Picker file hanya tersedia di aplikasi Windows", "warn");
  S.modal = true;
  try {
    const picked = await inv("cc_pick_files");
    if (Array.isArray(picked) && addUniquePaths(list, picked) > 0) redraw();
  } catch (e) {
    toast(String(e), "err");
  } finally {
    S.modal = false;
    inv("focus_window").catch(() => {});
  }
}

function blobToBase64(blob) {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(String(r.result || "").replace(/^data:[^;]*;base64,/, ""));
    r.onerror = () => rej(new Error("Gagal membaca file"));
    r.readAsDataURL(blob);
  });
}

async function saveBrowserFiles(files, list, redraw) {
  if (!T || !files?.length) return;
  let saved = 0;
  for (const f of files) {
    if (f.size > 25 * 1024 * 1024) { toast(`${f.name} melebihi 25 MB`, "warn"); continue; }
    try {
      const b64 = await blobToBase64(f);
      const path = await inv("cc_save_attachment", { name: f.name || "gambar.png", dataB64: b64 });
      if (addUniquePaths(list, [path]) > 0) saved++;
    } catch (e) { toast(String(e), "err"); }
  }
  if (saved > 0) { redraw(); toast(`${saved} lampiran ditambahkan`); }
}

function bindPasteAndDrop(zoneEl, inputEl, getList, redraw) {
  inputEl.addEventListener("paste", async (e) => {
    const items = [...(e.clipboardData?.items || [])];
    const files = items.filter((it) => it.kind === "file").map((it) => it.getAsFile()).filter(Boolean);
    if (!files.length) return;
    e.preventDefault();
    await saveBrowserFiles(files, getList(), redraw);
  });
  zoneEl.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types?.includes("Files")) { e.preventDefault(); zoneEl.classList.add("drag"); }
  });
  zoneEl.addEventListener("dragleave", () => zoneEl.classList.remove("drag"));
  zoneEl.addEventListener("drop", async (e) => {
    zoneEl.classList.remove("drag");
    const files = [...(e.dataTransfer?.files || [])];
    if (!files.length) return;
    e.preventDefault();
    await saveBrowserFiles(files, getList(), redraw);
  });
}

// ======================================================================
// Detail sesi
// ======================================================================
function openDetail(id, provider = "claude") {
  const prov = provider || "claude";
  if (!detail || detail.id !== id || detail.provider !== prov) {
    detail = { id, provider: prov };
    dVer = 0;
    openTools.clear();
    expandedText.clear();
    $("#dTl").innerHTML = "";
    $("#dPerms").innerHTML = "";
    $("#dInput").value = "";
    resizeBox($("#dInput"));
    dAtts = [];
    renderDetailAtts();
    dFork = false;
    $("#dFork").classList.remove("on");
    cancelDetailRename();
    resetMenuDel();
    const cfg = loadCfg(prov, id);
    populateModelSelect($("#dModel"), prov, cfg.model || "default");
    $("#dEffort").value = cfg.effort || "default";
    $("#dMode").value = cfg.mode || "default";
  }
  nav.go("detail");
  renderDetailHead();
  loadTimeline(true);
}

function renderDetailHead() {
  const s = curSess();
  if (!s) return;
  const prov = s.provider || detail?.provider || "claude";
  const dp = $("#dProv");
  dp.className = `ptag p-${prov}`;
  dp.textContent = PROV_LBL[prov] || prov;
  if ($("#dRenameIn").hidden) $("#dTitle").textContent = s.title;
  const p = s.proc;
  $("#dSub").textContent = [s.cwd, s.branch && "⎇ " + s.branch, p?.host || (s.managed ? "Notch" : ""), s.entry].filter(Boolean).join("  ·  ");
  const pill = $("#dState");
  pill.className = "spill " + STATE_CLS[s.state];
  pill.textContent = STATE_TXT[s.state];
  $("#dStop").disabled = !s.pid;
  const chips = [];
  if (s.model) chips.push(["Model", s.model.replace(/^claude-/, "")]);
  if (s.tok?.in || s.tok?.out) chips.push(["Token", `${ntok(s.tok.in)} masuk · ${ntok(s.tok.out)} keluar`]);
  if (s.tok?.cache) chips.push(["Cache", ntok(s.tok.cache)]);
  if (s.start) chips.push(["Durasi", dur((s.state === "done" ? s.last : Date.now()) - s.start)]);
  if (s.mode) chips.push(["Mode", s.mode]);
  if (p) chips.push([`PID ${p.pid}`, `CPU ${p.cpu}% · ${bytes(p.mem)}`]);
  else if (s.pid) chips.push(["PID", String(s.pid)]);
  if (p?.children?.length) chips.push(["Menjalankan", p.children.map((c) => c.name.replace(/\.exe$/i, "")).slice(0, 4).join(", ")]);
  const metaH = chips.map(([k, v]) => `<span class="chip">${esc(k)} <b>${esc(v)}</b></span>`).join("");
  if ($("#dMeta")._h !== metaH) { $("#dMeta").innerHTML = metaH; $("#dMeta")._h = metaH; }
  renderPerms(s);
  const pi = provInfo(prov);
  $("#dFork").hidden = pi ? !pi.supports_fork : prov !== "claude" && prov !== "opencode";

  // composer
  const c = $(".composer"), inp = $("#dInput"), hint = $("#dHint");
  let can = true, h = "";
  if (s.managed_live) { can = false; h = "Sedang berjalan dari notch… Stop untuk menghentikan."; }
  else if (s.pid) { can = false; h = `Sesi ini sedang terbuka di ${p?.host || "aplikasi lain"}. Kirim dari sana, atau Stop dulu lalu lanjutkan di sini.`; }
  else if (pi && !pi.installed) { can = false; h = `${pi.name} belum terpasang. Buka Providers untuk install.`; }
  else if (dFork) h = "Mode Fork aktif — pesan berikutnya akan membuat sesi cabang baru.";
  else h = `Melanjutkan sesi ${PROV_LBL[prov] || prov} secara headless dari notch.`;
  c.classList.toggle("off", !can);
  inp.disabled = !can;
  $("#dAttach").disabled = !can;
  $("#dModel").disabled = !can;
  $("#dEffort").disabled = !can;
  $("#dMode").disabled = !can;
  $("#dFork").disabled = !can;
  $("#dSend").disabled = !can;
  hint.textContent = h;
  if ((s.ver || 0) !== dVer) loadTimeline();
}

function startDetailRename() {
  const s = curSess();
  if (!s) return;
  const inp = $("#dRenameIn");
  $("#dTitle").hidden = true;
  $("#dRenameBtn").hidden = true;
  inp.hidden = false;
  inp.value = s.title || "";
  inp.focus();
  inp.select();
}
function cancelDetailRename() {
  $("#dRenameIn").hidden = true;
  $("#dTitle").hidden = false;
  $("#dRenameBtn").hidden = false;
}
$("#dRenameBtn").onclick = startDetailRename;
$("#dTitle").ondblclick = startDetailRename;
$("#dRenameIn").onkeydown = async (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    const cur = detail, val = $("#dRenameIn").value;
    cancelDetailRename();
    if (cur) await commitRename(cur.provider, cur.id, val);
    renderDetailHead();
  }
  if (e.key === "Escape") { e.stopPropagation(); cancelDetailRename(); }
};
$("#dRenameIn").onblur = cancelDetailRename;

function resetMenuDel() {
  menuDelConfirm = false;
  clearTimeout(menuDelTimer);
  const b = $("#dMenuDel");
  if (b) { b.classList.remove("confirm"); b.querySelector("span").textContent = "Hapus sesi"; }
}

async function loadTimeline(first) {
  const cur = detail;
  if (!cur) return;
  let r;
  try {
    r = await withTimeout(
      inv("ag_session", { provider: cur.provider, id: cur.id, ver: first ? 0 : dVer }),
      30000,
      "Timeline sesi tidak merespons (timeout 30 detik)",
    );
  } catch (e) { if (first) $("#dTl").innerHTML = `<div class="empty">${esc(e)}</div>`; return; }
  if (!r || !detail || cur.id !== detail.id || cur.provider !== detail.provider) return;
  dVer = r.ver;
  renderTimeline(r.items, first);
}

function toolBody(it) {
  return previewInput(it.name, it.input) + (it.out ? `<span class="lbl">Output</span><pre>${esc(it.out)}</pre>` : "");
}

function itemHtml(it) {
  if (it.k === "user") return `<span class="who">Kamu</span>${esc(it.text)}`;
  if (it.k === "note") return esc(it.text);
  if (it.k === "text") {
    const long = it.text.length > 900 && !expandedText.has(it.seq);
    return `<div class="md">${md(it.text)}</div>${long ? '<button class="more">Tampilkan semua</button>' : ""}`;
  }
  const st = it.st === "run" ? "run" : it.st === "err" ? "err" : "ok";
  const t = it.end ? ` title="${dur(it.end - it.ts)}"` : "";
  return `<button class="tool-h"${t}><span class="ti">${icon(TOOL_IC[it.name] || "spark")}</span><b>${esc(it.name)}</b><code>${esc(it.sum)}</code><span class="st ${st}">${st === "ok" ? icon("check") : st === "err" ? icon("x") : ""}</span></button><div class="tool-b">${openTools.has(it.seq) ? toolBody(it) : ""}</div>`;
}
function itemCls(it) {
  let c = "it " + it.k;
  if (it.k === "tool") c += (it.st === "err" ? " err" : "") + (openTools.has(it.seq) ? " open" : "");
  if (it.k === "text" && it.text.length > 900 && !expandedText.has(it.seq)) c += " clamp";
  return c;
}

let items = [];
function renderTimeline(list, first) {
  const box = $("#dTl");
  const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
  items = list;
  const have = new Map($$(".it", box).map((el) => [+el.dataset.seq, el]));
  const keep = new Set(list.map((i) => i.seq));
  have.forEach((el, seq) => { if (!keep.has(seq)) el.remove(); });
  let prev = null;
  for (const it of list) {
    let el = have.get(it.seq);
    const h = itemHtml(it), c = itemCls(it);
    if (!el) {
      el = document.createElement("div");
      el.dataset.seq = it.seq;
      if (first) el.style.animation = "none";
    }
    if (el._h !== h) { el.innerHTML = h; el._h = h; }
    if (el.className !== c) el.className = c;
    const next = prev ? prev.nextSibling : box.firstChild;
    if (next !== el) box.insertBefore(el, next);
    prev = el;
  }
  if (!list.length) box.innerHTML = `<div class="empty">Belum ada aktivitas</div>`;
  if (first || atBottom) { box.scrollTop = box.scrollHeight; $("#dJump").hidden = true; }
  else if (!first) $("#dJump").hidden = false;
}

$("#dTl").addEventListener("click", (e) => {
  const el = e.target.closest(".it");
  if (!el) return;
  const seq = +el.dataset.seq;
  const it = items.find((i) => i.seq === seq);
  if (!it) return;
  if (e.target.closest(".tool-h")) {
    openTools.has(seq) ? openTools.delete(seq) : openTools.add(seq);
  } else if (e.target.closest(".more")) {
    expandedText.add(seq);
  } else return;
  el.innerHTML = itemHtml(it);
  el._h = el.innerHTML;
  el.className = itemCls(it);
});
$("#dTl").addEventListener("scroll", () => {
  const b = $("#dTl");
  if (b.scrollHeight - b.scrollTop - b.clientHeight < 80) $("#dJump").hidden = true;
});
$("#dJump").onclick = () => { const b = $("#dTl"); b.scrollTo({ top: b.scrollHeight, behavior: "smooth" }); $("#dJump").hidden = true; };

// ---------- aksi detail ----------
$("#dStop").onclick = async () => {
  const s = curSess();
  if (!s?.pid) return;
  const b = $("#dStop");
  if (!b.classList.contains("confirm")) {
    b.classList.add("confirm");
    b.innerHTML = `${icon("stop")}Yakin?`;
    setTimeout(() => { b.classList.remove("confirm"); b.innerHTML = `${icon("stop")}Stop`; }, 2500);
    return;
  }
  b.classList.remove("confirm");
  b.innerHTML = `${icon("stop")}Stop`;
  await killPid(s.provider || "claude", s.pid);
};
$("#dMoreBtn").onclick = (e) => { e.stopPropagation(); resetMenuDel(); $("#dMenu").classList.toggle("on"); };
document.addEventListener("click", () => { $("#dMenu").classList.remove("on"); resetMenuDel(); });
$("#dMenu").onclick = async (e) => {
  const a = e.target.closest("[data-act]")?.dataset.act;
  const s = curSess();
  if (!a || !s) return;
  const prov = s.provider || "claude";
  if (a === "delete") {
    e.stopPropagation();
    if (s.pid || s.managed_live) { $("#dMenu").classList.remove("on"); return toast("Hentikan proses sesi dulu sebelum menghapus", "warn"); }
    if (!menuDelConfirm) {
      menuDelConfirm = true;
      const b = $("#dMenuDel");
      b.classList.add("confirm");
      b.querySelector("span").textContent = "Yakin hapus permanen?";
      clearTimeout(menuDelTimer);
      menuDelTimer = setTimeout(resetMenuDel, 3000);
      return;
    }
    $("#dMenu").classList.remove("on");
    resetMenuDel();
    await deleteSessionById(prov, s.id);
    return;
  }
  $("#dMenu").classList.remove("on");
  if (a === "rename") { startDetailRename(); return; }
  if (a === "copy") {
    try { await navigator.clipboard.writeText(s.id); toast("ID sesi disalin"); } catch { toast("Clipboard tidak tersedia", "err"); }
    return;
  }
  if (a === "terminal" && s.pid) return toast(`Sesi masih terbuka di ${s.proc?.host || "aplikasi lain"}`, "warn");
  try {
    await inv("ag_open", { provider: prov, kind: a, cwd: s.cwd || ".", session: s.id });
    if (a !== "folder") setOpen(false);
  } catch (err) { toast(String(err), "err"); }
};

$("#dAttach").onclick = () => pickFilesFor(dAtts, renderDetailAtts);
$("#dModel").onchange = (e) => detail && saveCfg(detail.provider, detail.id, { model: e.target.value });
$("#dEffort").onchange = (e) => detail && saveCfg(detail.provider, detail.id, { effort: e.target.value });
$("#dMode").onchange = (e) => detail && saveCfg(detail.provider, detail.id, { mode: e.target.value });
$("#dFork").onclick = () => {
  dFork = !dFork;
  $("#dFork").classList.toggle("on", dFork);
  renderDetailHead();
};
bindPasteAndDrop($("#dCompWrap"), $("#dInput"), () => dAtts, renderDetailAtts);

async function sendFollowUp() {
  const s = curSess();
  const v = $("#dInput").value.trim();
  if (!s || (!v && !dAtts.length) || $("#dInput").disabled) return;
  const prov = s.provider || "claude";
  const sentAtts = [...dAtts];
  const fork = dFork;
  $("#dInput").value = "";
  resizeBox($("#dInput"));
  dAtts = [];
  renderDetailAtts();
  dFork = false;
  $("#dFork").classList.remove("on");
  $("#dHint").textContent = fork ? "Membuat fork sesi…" : "Memulai…";
  try {
    const runId = await inv("ag_run", {
      provider: prov,
      cwd: s.cwd || ld("notch.lastCwd", ""),
      prompt: v,
      resume: s.id,
      model: $("#dModel").value,
      effort: $("#dEffort").value,
      mode: $("#dMode").value,
      name: null,
      fork,
      attachments: sentAtts,
    });
    pendingRun = { run: runId, provider: prov };
    kick(180);
  } catch (e) {
    $("#dInput").value = v;
    resizeBox($("#dInput"));
    dAtts = sentAtts;
    renderDetailAtts();
    dFork = fork;
    $("#dFork").classList.toggle("on", dFork);
    $("#dHint").textContent = String(e);
    toast(String(e), "err");
  }
}
$("#dSend").onclick = sendFollowUp;
$("#dInput").oninput = () => resizeBox($("#dInput"));
$("#dInput").onkeydown = (e) => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendFollowUp(); }
};

// ======================================================================
// Izin (Claude Code hook gate)
// ======================================================================
function diffHtml(o, n) {
  const ol = String(o ?? "").split("\n"), nl = String(n ?? "").split("\n");
  const cut = (a) => (a.length > 40 ? a.slice(0, 40).concat(["…"]) : a);
  return `<pre class="diff">${cut(ol).map((l) => `<span class="del">− ${esc(l)}</span>`).join("")}${cut(nl).map((l) => `<span class="add">+ ${esc(l)}</span>`).join("")}</pre>`;
}
function previewInput(tool, inp) {
  if (!inp || typeof inp !== "object") return "";
  const path = inp.file_path || inp.notebook_path;
  const lbl = (t) => `<span class="lbl">${t}</span>`;
  switch (tool) {
    case "Bash":
    case "PowerShell":
    case "shell":
      return `${inp.description ? `<span class="sub">${esc(inp.description)}</span>` : ""}<pre>${esc(inp.command)}</pre>`;
    case "Edit":
      return `${lbl(esc(path))}${diffHtml(inp.old_string, inp.new_string)}`;
    case "MultiEdit":
      return `${lbl(esc(path))}${(inp.edits || []).slice(0, 5).map((e) => diffHtml(e.old_string, e.new_string)).join("")}`;
    case "Write":
      return `${lbl(esc(path))}<pre>${esc(String(inp.content ?? "").split("\n").slice(0, 40).join("\n"))}</pre>`;
    case "Read":
      return `${lbl(esc(path))}`;
    case "WebFetch":
      return `<pre>${esc(inp.url)}${inp.prompt ? "\n\n" + esc(inp.prompt) : ""}</pre>`;
    default:
      return `<pre>${esc(JSON.stringify(inp, null, 2))}</pre>`;
  }
}
function permHtml(p, withProj) {
  const extra = [
    `<button class="btn sm ghost" data-d="allow_session">Izinkan ${esc(p.tool)} di sesi ini</button>`,
    p.rules ? `<button class="btn sm ghost" data-d="always">Selalu izinkan (aturan)</button>` : "",
    `<button class="btn sm ghost danger" data-d="deny_stop">Tolak &amp; hentikan</button>`,
    p.managed ? "" : `<button class="btn sm ghost" data-d="pass">Jawab di terminal</button>`,
  ].join("");
  return `<div class="perm" data-pid="${p.id}">
    <div class="perm-h">${icon("lock")}<b>${esc(p.tool)}</b>${withProj ? `<span class="sub">${esc(p.project)}</span>` : ""}<time>${ago(p.created)}</time></div>
    ${previewInput(p.tool, p.input)}
    <div class="perm-a"><button class="btn sm ghost" data-more>Lainnya</button><span class="grow"></span><button class="btn sm danger" data-d="deny">Tolak</button><button class="btn sm pri" data-d="allow">Izinkan</button></div>
    <div class="perm-a" data-extra hidden>${extra}</div>
  </div>`;
}
function renderPerms(s) {
  const box = $("#dPerms");
  const ps = (s.provider || "claude") === "claude" ? (snap?.pending || []).filter((p) => p.session === s.id) : [];
  const ids = ps.map((p) => p.id).join(",");
  if (box.dataset.ids === ids) {
    $$(".perm time", box).forEach((t, i) => (t.textContent = ago(ps[i].created)));
    return;
  }
  box.dataset.ids = ids;
  box.innerHTML = ps.map((p) => permHtml(p, false)).join("");
}
async function decide(id, action) {
  try {
    await inv("cc_decide", { id, action, note: null });
    if (snap) snap.pending = snap.pending.filter((p) => p.id !== id);
    $$(`.perm[data-pid="${id}"]`).forEach((el) => el.remove());
    $("#dPerms").dataset.ids = "";
    renderPill();
    layout();
  } catch (e) { toast(String(e), "err"); }
  kick(60);
}
$("#dPerms").addEventListener("click", (e) => {
  const card = e.target.closest(".perm");
  if (!card) return;
  if (e.target.closest("[data-more]")) { const x = card.querySelector("[data-extra]"); x.hidden = !x.hidden; return; }
  const d = e.target.closest("[data-d]")?.dataset.d;
  if (d) decide(+card.dataset.pid, d);
});
$("#alAllow").onclick = (e) => { e.stopPropagation(); S.alert && decide(S.alert.id, "allow"); };
$("#alDeny").onclick = (e) => { e.stopPropagation(); S.alert && decide(S.alert.id, "deny"); };
$("#alOpen").onclick = (e) => { e.stopPropagation(); const a = S.alert; if (!a) return; setOpen(true, "agents"); openDetail(a.session, "claude"); };

// ======================================================================
// Sesi baru (multi-provider seperti T3 Code)
// ======================================================================
function renderNewProviderState() {
  $$("#nProv button").forEach((b) => b.classList.toggle("on", b.dataset.v === newProv));
  populateModelSelect($("#nModelSel"), newProv, $("#nModelSel").value || "default");
  const info = provInfo(newProv);
  const warn = $("#nProvWarn");
  if (!info || (info.installed && info.logged_in)) {
    warn.hidden = true;
    return;
  }
  warn.hidden = false;
  if (!info.installed) {
    $("#nProvWarnT").textContent = `${info.name} belum terpasang`;
    $("#nProvWarnS").textContent = `Install langsung di notch (${info.install_hint})`;
    $("#nProvAct").textContent = "Install";
    $("#nProvAct").onclick = () => { openProviders(false); startInstall(newProv); };
  } else {
    $("#nProvWarnT").textContent = `${info.name} belum login`;
    $("#nProvWarnS").textContent = `Login akun atau masukkan ${info.api_key_env}`;
    $("#nProvAct").textContent = "Login";
    $("#nProvAct").onclick = () => { openProviders(false); startOAuthLogin(newProv, null); };
  }
}

function openNew(prompt) {
  nav.root();
  nav.go("new");
  $("#nErr").textContent = "";
  $("#nName").value = "";
  nAtts = [];
  renderNewAtts();
  if (prompt != null) $("#nPrompt").value = prompt;
  if (!$("#nCwd").value) $("#nCwd").value = ld("notch.lastCwd", "");
  const recent = [...new Set((snap?.sessions || []).map((s) => s.cwd).filter(Boolean))].slice(0, 8);
  $("#nRecent").innerHTML = recent.map((c) => `<button class="chip" title="${esc(c)}" data-cwd="${esc(c)}">${icon("folder")}${esc(c.split(/[\\/]/).filter(Boolean).pop() || c)}</button>`).join("");
  renderNewProviderState();
  setTimeout(() => (prompt ? $("#nCwd") : $("#nPrompt")).focus(), 250);
}
$("#nProv").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  newProv = b.dataset.v;
  sv("notch.newProv", newProv);
  $("#nModelSel")._h = "";
  $("#nModelSel").value = "default";
  renderNewProviderState();
});
$("#nProvMgr").onclick = () => openProviders(false);
$("#nRecent").onclick = (e) => { const c = e.target.closest("[data-cwd]"); if (c) $("#nCwd").value = c.dataset.cwd; };
$("#nBrowse").onclick = async () => {
  if (!T) return toast("Picker folder hanya tersedia di aplikasi Windows", "warn");
  S.modal = true;
  try {
    const dir = await inv("cc_pick_folder", { title: "Pilih folder proyek" });
    if (dir) $("#nCwd").value = dir;
  } catch (e) { toast(String(e), "err"); }
  finally { S.modal = false; inv("focus_window").catch(() => {}); }
};
$("#nAttach").onclick = () => pickFilesFor(nAtts, renderNewAtts);
bindPasteAndDrop($(`.page[data-page="new"] .form`), $("#nPrompt"), () => nAtts, renderNewAtts);

$("#nStart").onclick = async () => {
  const cwd = $("#nCwd").value.trim(), prompt = $("#nPrompt").value.trim(), name = $("#nName").value.trim();
  if (!cwd || (!prompt && !nAtts.length)) return ($("#nErr").textContent = "Isi folder dan perintah/lampiran dulu.");
  $("#nStart").disabled = true;
  $("#nErr").textContent = "";
  try {
    const runId = await inv("ag_run", {
      provider: newProv,
      cwd,
      prompt,
      resume: null,
      model: $("#nModelSel").value,
      effort: $("#nEffortSel").value,
      mode: $("#nModeSel").value,
      name: name || null,
      fork: false,
      attachments: nAtts,
    });
    pendingRun = { run: runId, provider: newProv };
    sv("notch.lastCwd", cwd);
    $("#nPrompt").value = "";
    $("#nName").value = "";
    nAtts = [];
    renderNewAtts();
    nav.root();
    toast(`Sesi ${PROV_LBL[newProv] || newProv} dimulai`);
    kick(250);
  } catch (e) { $("#nErr").textContent = String(e); }
  $("#nStart").disabled = false;
};
$("#agNewBtn").onclick = () => openNew();
on("newSession", ({ prompt }) => { setOpen(true, "agents"); openNew(prompt); });

// ======================================================================
// Hook & pengaturan (Claude Code)
// ======================================================================
async function loadHooks(action = "status") {
  try {
    hooks = await inv("cc_hooks", { action });
    if (action === "install") toast("Hook Claude Code terpasang");
    if (action === "uninstall") toast("Hook dicopot", "warn");
  } catch (e) {
    hooks = null;
    if (action !== "status") toast(String(e), "err");
  }
  const ok = hooks?.installed && hooks?.server_ok;
  $("#hookDot").className = "hookdot " + (ok ? "ok" : hooks ? "warn" : "");
  $("#hookBanner").hidden = !hooks || hooks.installed;
  if (nav.cur === "settings") renderSettings();
}
async function renderSettings() {
  const h = hooks;
  const row = (k, v, c = "") => `<span>${k}</span><b class="${c}">${v}</b>`;
  $("#hkInfo").innerHTML = h
    ? [
        row("Server hook", h.server_ok ? `Aktif · 127.0.0.1:${h.port}` : esc(h.server_error), h.server_ok ? "ok" : "bad"),
        row("Hook", h.installed ? "Terpasang" : h.stale ? "Token lama — pasang ulang" : "Belum terpasang", h.installed ? "ok" : "bad"),
        row("settings.json", esc(h.settings)),
        row("Claude Code", h.bin ? esc(h.bin) : "Tidak ditemukan (set NOTCH_CLAUDE_BIN)", h.bin ? "" : "bad"),
      ].join("")
    : row("Status", "Tidak tersedia di luar aplikasi", "bad");
  $("#hkRemove").disabled = !h || (!h.installed && !h.stale);
  try {
    const r = await inv("cc_history");
    $("#hkAllow").innerHTML = r.allow.length
      ? r.allow.map((a) => `<div class="mi"><code>${esc(a.tools.join(", "))}</code><span>${esc(sessByKey("claude", a.session)?.project || a.session.slice(0, 8))}</span></div>`).join("")
      : `<div class="mi">Tidak ada</div>`;
    const DL = { allow: "Diizinkan", always: "Selalu", allow_session: "Diizinkan (sesi)", deny: "Ditolak", deny_stop: "Ditolak + stop", pass: "Ke terminal" };
    $("#hkHist").innerHTML = r.history.length
      ? r.history.map((x) => `<div class="mi"><span class="tag ${String(x.d).startsWith("deny") ? "warn" : "ok"}">${esc(DL[x.d] || x.d)}</span><b>${esc(x.tool)}</b><code>${esc(x.sum)}</code><span>${ago(x.ts)}</span></div>`).join("")
      : `<div class="mi">Belum ada keputusan</div>`;
  } catch {}
}
$("#agSettingsBtn").onclick = () => { nav.go("settings"); loadHooks(); renderSettings(); };
$("#hookInstallQuick").onclick = () => loadHooks("install");
$("#hkInstall").onclick = () => loadHooks("install");
$("#hkRemove").onclick = () => loadHooks("uninstall");
$("#hkClearAllow").onclick = async () => { await inv("cc_clear_allow").catch(() => {}); renderSettings(); };

// ======================================================================
// Event dari backend
// ======================================================================
listen("cc-event", (e) => {
  const p = e.payload || {};
  if (p.kind === "permission") {
    kick(10);
  } else if (p.kind === "stop") {
    kick(50);
    const prov = p.provider || "claude";
    const s = sessByKey(prov, p.session);
    if (!S.open || !detail || detail.id !== p.session || detail.provider !== prov) {
      toast(`${s?.project || PROV_LBL[prov] || "Agent"} ${p.error ? "berhenti karena error" : "selesai"}`, p.error ? "err" : "ok");
    }
  } else if (p.kind === "run") {
    kick(100);
    const prov = p.provider || "claude";
    if (p.state === "init" && pendingRun && p.run === pendingRun.run && p.session) {
      const targetProv = pendingRun.provider || prov;
      pendingRun = null;
      setTimeout(() => { if (S.open && S.tab === "agents") openDetail(p.session, targetProv); }, 350);
    }
    if (p.state === "done" && p.exit !== 0 && p.exit != null) toast(`Sesi ${PROV_LBL[prov] || prov} keluar dengan kode ${p.exit}`, "err");
  } else if (p.kind === "providers") {
    inv("ag_providers", { refresh: false }).then((list) => {
      if (Array.isArray(list)) provCache = list;
      updateProvidersBtn();
      if (S.open && S.tab === "agents") {
        if (nav.cur === "providers") renderProviders();
        if (nav.cur === "new") renderNewProviderState();
      }
    }).catch(() => {});
    kick(50);
  } else if (p.kind === "notify" && p.type === "permission_prompt") {
    kick(30);
  } else kick();
});

// ======================================================================
// Navigasi & siklus hidup
// ======================================================================
on("tab", (t) => { if (t === "agents") { poll(); if (nav.cur === "settings") renderSettings(); if (nav.cur === "providers") openProviders(false); } });
on("open", () => { if (S.tab === "agents") poll(); });
on(`page:agents`, (p) => { if (p === "list") renderList(); if (p === "providers") renderProviders(); });
on("esc", (ev) => {
  if (S.tab !== "agents") return;
  if (!$("#dRenameIn").hidden) { cancelDetailRename(); ev.handled = true; return; }
  if (listRenameKey) { listRenameKey = null; renderList(); ev.handled = true; return; }
  if (keyEditProv) { keyEditProv = null; renderProviders(); ev.handled = true; return; }
  if (nav.back()) ev.handled = true;
});
on("openAgentLive", () => {
  const id = S.alert?.session || S.agentLive?.id;
  const prov = S.alert ? "claude" : S.agentLive?.provider || "claude";
  setOpen(true, "agents");
  if (id) openDetail(id, prov);
});

setInterval(() => {
  if (S.open && S.tab === "agents" && nav.cur === "detail" && detail) loadTimeline();
}, 1200);

export function initAgents() {
  poll();
  loadHooks();
}
