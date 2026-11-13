// Monitor & kontrol Claude Code: daftar sesi, timeline, gerbang izin, sesi baru, hook.
import { $, $$, S, T, inv, listen, esc, icon, ago, dur, ntok, bytes, startOfDay, on, emit, layout, setOpen, toast, stack, ld, sv } from "./core.js";
import { md } from "./md.js";

const nav = stack("agents");
let snap = null;
let hooks = null;
let detail = null; // id sesi yang sedang dibuka
let dVer = 0;
const openTools = new Set();
const expandedText = new Set();
let pendingRun = null; // run_id yang baru dibuat, untuk auto-buka sesinya
let newModel = "default", newMode = "default";

const STATE_TXT = { running: "Berjalan", waiting: "Butuh izin", idle: "Siap", done: "Selesai" };
const STATE_CLS = { running: "run", waiting: "wait", idle: "idle", done: "done" };
const TOOL_IC = { Read: "file", Write: "edit", Edit: "edit", MultiEdit: "edit", NotebookEdit: "edit", Bash: "term", PowerShell: "term", Grep: "search", Glob: "search", WebFetch: "globe", WebSearch: "globe", TodoWrite: "list", Task: "bot", Agent: "bot" };
const sessById = (id) => snap?.sessions.find((s) => s.id === id);
const short = (s, n = 60) => (s && s.length > n ? s.slice(0, n - 1) + "…" : s || "");

// ======================================================================
// Polling
// ======================================================================
let pollT, busy = false, again = false;
async function poll() {
  if (busy) { again = true; return; }
  busy = true;
  clearTimeout(pollT);
  try {
    snap = await inv("cc_snapshot", { since: startOfDay() });
    render();
  } catch (e) {
    if (!T) renderOffline(e);
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
  if (S.open && S.tab === "agents") {
    if (nav.cur === "list") renderList();
    if (nav.cur === "detail") renderDetailHead();
  }
}

function renderOffline(e) {
  $("#sessList").innerHTML = `<div class="empty">${icon("plug")}<b>Mode pratinjau</b><span>${esc(e)}</span></div>`;
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
    S.agentLive = {
      dot: s.state === "waiting" ? "wait" : "run",
      proj: s.project || "claude",
      act: s.activity || "Berjalan…",
      meta: run.length > 1 ? `+${run.length - 1}` : s.turn ? dur(Date.now() - s.turn) : "",
      id: s.id,
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
// Daftar sesi (keyed, tanpa kedip)
// ======================================================================
function cardHtml(s) {
  const st = STATE_CLS[s.state];
  const p = s.proc;
  const tags = [];
  if (s.pending) tags.push(`<span class="tag warn">${s.pending} izin</span>`);
  if (p?.host) tags.push(`<span class="tag">${esc(p.host)}</span>`);
  else if (s.managed) tags.push(`<span class="tag acc">Notch</span>`);
  if (s.subagent) tags.push(`<span class="tag">subagent</span>`);
  const act = s.state === "running" || s.state === "waiting" ? s.activity : s.state === "idle" ? s.note || "Menunggu instruksi" : s.prompt;
  return `<span class="sdot ${st}"></span>
    <div class="sbody">
      <div class="sr1"><b>${esc(s.title)}</b><time>${ago(s.last)}</time></div>
      <div class="sr2">${esc(s.project || "—")}${s.branch ? `<span class="sep">·</span>${esc(s.branch)}` : ""}<span class="sep">·</span>${ntok(s.tok.out)} keluar ${tags.join("")}</div>
      ${act ? `<div class="sact ${st}">${esc(short(act, 110))}</div>` : ""}
    </div>`;
}

function renderList() {
  const box = $("#sessList");
  if (!snap) return;
  $("#agToday").textContent = `Hari ini · ${snap.today.sessions} sesi · ${ntok(snap.today.tokens)} token`;
  const groups = [
    ["waiting", "Butuh kamu"],
    ["running", "Berjalan"],
    ["idle", "Terbuka"],
    ["done", "Terbaru"],
  ];
  const want = [];
  for (const [st, label] of groups) {
    const ss = snap.sessions.filter((s) => s.state === st).slice(0, st === "done" ? 15 : 50);
    if (!ss.length) continue;
    want.push({ key: "g:" + st, cls: "grp", html: label });
    for (const s of ss) want.push({ key: "s:" + s.id, cls: "sess " + s.state, html: cardHtml(s), id: s.id, tag: "button" });
  }
  const orphans = snap.procs.filter((p) => !p.session);
  if (orphans.length) {
    want.push({ key: "g:proc", cls: "grp", html: "Proses Claude Code lain" });
    for (const p of orphans)
      want.push({
        key: "p:" + p.pid,
        cls: "proc",
        html: `${icon("bot")}<span><b>PID ${p.pid}</b> · ${esc(p.host)} · CPU ${p.cpu}% · ${bytes(p.mem)}${p.children.length ? " · " + esc(p.children.map((c) => c.name).join(", ")) : ""}</span><span class="grow"></span><button class="btn sm danger" data-kill="${p.pid}">Stop</button>`,
      });
  }
  const failed = snap.runs.filter((r) => r.done && r.error && Date.now() - r.started < 15 * 60e3);
  for (const r of failed)
    want.push({ key: "r:" + r.run, cls: "proc", html: `${icon("warn")}<span><b>Run gagal</b> · ${esc(short(r.error, 160))}</span>` });
  if (!want.length) {
    box.innerHTML = `<div class="empty">${icon("spark")}<b>Belum ada sesi Claude Code dalam 3 hari terakhir</b><span>Mulai sesi baru, atau jalankan Claude Code di terminal/VS Code.</span></div>`;
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
      if (w.id) el.onclick = () => openDetail(w.id);
    }
    have.delete(w.key);
    if (el.className !== w.cls) el.className = w.cls;
    if (el._h !== w.html) { el.innerHTML = w.html; el._h = w.html; }
    const next = prev ? prev.nextSibling : box.firstChild;
    if (next !== el) box.insertBefore(el, next);
    prev = el;
  }
  have.forEach((el) => el.remove());
  $$("[data-kill]", box).forEach((b) => (b.onclick = (e) => { e.stopPropagation(); killPid(+b.dataset.kill); }));
}

async function killPid(pid) {
  try { await inv("cc_kill", { pid }); toast("Proses Claude Code dihentikan", "warn"); } catch (e) { toast(String(e), "err"); }
  kick(50);
}

// ======================================================================
// Detail sesi
// ======================================================================
function openDetail(id) {
  if (detail !== id) {
    detail = id;
    dVer = 0;
    openTools.clear();
    expandedText.clear();
    $("#dTl").innerHTML = "";
    $("#dPerms").innerHTML = "";
    $("#dInput").value = "";
  }
  nav.go("detail");
  renderDetailHead();
  loadTimeline(true);
}

function renderDetailHead() {
  const s = sessById(detail);
  if (!s) return;
  $("#dTitle").textContent = s.title;
  const p = s.proc;
  $("#dSub").textContent = [s.cwd, s.branch && "⎇ " + s.branch, p?.host || (s.managed ? "Notch" : ""), s.entry].filter(Boolean).join("  ·  ");
  const pill = $("#dState");
  pill.className = "spill " + STATE_CLS[s.state];
  pill.textContent = STATE_TXT[s.state];
  $("#dStop").disabled = !s.pid;
  const chips = [];
  if (s.model) chips.push(["Model", s.model.replace(/^claude-/, "")]);
  chips.push(["Token", `${ntok(s.tok.in)} masuk · ${ntok(s.tok.out)} keluar`]);
  if (s.tok.cache) chips.push(["Cache", ntok(s.tok.cache)]);
  if (s.start) chips.push(["Durasi", dur((s.state === "done" ? s.last : Date.now()) - s.start)]);
  if (s.mode) chips.push(["Mode", s.mode]);
  if (p) chips.push([`PID ${p.pid}`, `CPU ${p.cpu}% · ${bytes(p.mem)}`]);
  if (p?.children.length) chips.push(["Menjalankan", p.children.map((c) => c.name.replace(/\.exe$/i, "")).slice(0, 4).join(", ")]);
  const metaH = chips.map(([k, v]) => `<span class="chip">${esc(k)} <b>${esc(v)}</b></span>`).join("");
  if ($("#dMeta")._h !== metaH) { $("#dMeta").innerHTML = metaH; $("#dMeta")._h = metaH; }
  renderPerms(s);
  // composer
  const c = $(".composer"), inp = $("#dInput"), hint = $("#dHint");
  let can = true, h = "";
  if (s.managed_live) { can = false; h = "Sedang berjalan dari notch… Stop untuk menghentikan."; }
  else if (s.pid) { can = false; h = `Sesi ini sedang terbuka di ${p?.host || "aplikasi lain"}. Kirim dari sana, atau Stop dulu lalu lanjutkan di sini.`; }
  else h = "Melanjutkan sesi ini secara headless (claude -p --resume). Izin akan ditanyakan di notch.";
  c.classList.toggle("off", !can);
  inp.disabled = !can;
  $("#dSend").disabled = !can;
  hint.textContent = h;
  if ((s.ver || 0) !== dVer) loadTimeline();
}

async function loadTimeline(first) {
  const id = detail;
  if (!id) return;
  let r;
  try { r = await inv("cc_session", { id, ver: first ? 0 : dVer }); } catch (e) { $("#dTl").innerHTML = `<div class="empty">${esc(e)}</div>`; return; }
  if (!r || id !== detail) return;
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
  const s = sessById(detail);
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
  await killPid(s.pid);
};
$("#dMoreBtn").onclick = (e) => { e.stopPropagation(); $("#dMenu").classList.toggle("on"); };
document.addEventListener("click", () => $("#dMenu").classList.remove("on"));
$("#dMenu").onclick = async (e) => {
  const a = e.target.closest("[data-act]")?.dataset.act;
  const s = sessById(detail);
  $("#dMenu").classList.remove("on");
  if (!a || !s) return;
  if (a === "copy") {
    try { await navigator.clipboard.writeText(s.id); toast("ID sesi disalin"); } catch { toast("Clipboard tidak tersedia", "err"); }
    return;
  }
  if (a === "terminal" && s.pid) return toast(`Sesi masih terbuka di ${s.proc?.host || "aplikasi lain"}`, "warn");
  try {
    await inv("cc_open", { kind: a, cwd: s.cwd, session: s.id });
    if (a !== "folder") setOpen(false);
  } catch (err) { toast(String(err), "err"); }
};
async function sendFollowUp() {
  const s = sessById(detail);
  const v = $("#dInput").value.trim();
  if (!s || !v || $("#dInput").disabled) return;
  $("#dInput").value = "";
  $("#dHint").textContent = "Memulai…";
  try {
    pendingRun = await inv("cc_run", { cwd: s.cwd, prompt: v, resume: s.id, model: null, mode: $("#dMode").value });
    kick(200);
  } catch (e) {
    $("#dInput").value = v;
    $("#dHint").textContent = String(e);
    toast(String(e), "err");
  }
}
$("#dSend").onclick = sendFollowUp;
$("#dInput").onkeydown = (e) => { if (e.key === "Enter") sendFollowUp(); };

// ======================================================================
// Izin
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
  const ps = (snap?.pending || []).filter((p) => p.session === s.id);
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
$("#alOpen").onclick = (e) => { e.stopPropagation(); const a = S.alert; if (!a) return; setOpen(true, "agents"); openDetail(a.session); };

// ======================================================================
// Sesi baru
// ======================================================================
function openNew(prompt) {
  nav.root();
  nav.go("new");
  $("#nErr").textContent = "";
  if (prompt != null) $("#nPrompt").value = prompt;
  if (!$("#nCwd").value) $("#nCwd").value = ld("notch.lastCwd", "");
  const recent = [...new Set((snap?.sessions || []).map((s) => s.cwd).filter(Boolean))].slice(0, 8);
  $("#nRecent").innerHTML = recent.map((c) => `<button class="chip" title="${esc(c)}" data-cwd="${esc(c)}">${icon("folder")}${esc(c.split(/[\\/]/).filter(Boolean).pop() || c)}</button>`).join("");
  setTimeout(() => (prompt ? $("#nCwd") : $("#nPrompt")).focus(), 250);
}
$("#nRecent").onclick = (e) => { const c = e.target.closest("[data-cwd]"); if (c) $("#nCwd").value = c.dataset.cwd; };
const segPick = (sel, set) => $(sel).addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  $$(`${sel} button`).forEach((x) => x.classList.toggle("on", x === b));
  set(b.dataset.v);
});
segPick("#nModel", (v) => (newModel = v));
segPick("#nMode", (v) => (newMode = v));
$("#nStart").onclick = async () => {
  const cwd = $("#nCwd").value.trim(), prompt = $("#nPrompt").value.trim();
  if (!cwd || !prompt) return ($("#nErr").textContent = "Isi folder dan perintah dulu.");
  $("#nStart").disabled = true;
  $("#nErr").textContent = "";
  try {
    pendingRun = await inv("cc_run", { cwd, prompt, resume: null, model: newModel, mode: newMode });
    sv("notch.lastCwd", cwd);
    $("#nPrompt").value = "";
    nav.root();
    toast("Sesi Claude Code dimulai");
    kick(300);
  } catch (e) { $("#nErr").textContent = String(e); }
  $("#nStart").disabled = false;
};
$("#agNewBtn").onclick = () => openNew();
on("newSession", ({ prompt }) => { setOpen(true, "agents"); openNew(prompt); });

// ======================================================================
// Hook & pengaturan
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
      ? r.allow.map((a) => `<div class="mi"><code>${esc(a.tools.join(", "))}</code><span>${esc(sessById(a.session)?.project || a.session.slice(0, 8))}</span></div>`).join("")
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
    const s = sessById(p.session);
    if (!S.open || detail !== p.session) toast(`${s?.project || "Claude"} ${p.error ? "berhenti karena error" : "selesai"}`, p.error ? "err" : "ok");
  } else if (p.kind === "run") {
    kick(100);
    if (p.state === "init" && p.run === pendingRun && p.session) {
      pendingRun = null;
      setTimeout(() => { if (S.open && S.tab === "agents") openDetail(p.session); }, 400);
    }
    if (p.state === "done" && p.exit !== 0 && p.exit != null) toast("Sesi headless keluar dengan kode " + p.exit, "err");
  } else if (p.kind === "notify" && p.type === "permission_prompt") {
    kick(30);
  } else kick();
});

// ======================================================================
// Navigasi & siklus hidup
// ======================================================================
on("tab", (t) => { if (t === "agents") { poll(); if (nav.cur === "settings") renderSettings(); } });
on("open", () => { if (S.tab === "agents") poll(); });
on(`page:agents`, (p) => { if (p === "list") renderList(); });
on("esc", (ev) => { if (S.tab === "agents" && nav.back()) ev.handled = true; });
on("openAgentLive", () => {
  const id = S.alert?.session || S.agentLive?.id;
  setOpen(true, "agents");
  if (id) openDetail(id);
});

// Tik waktu relatif + pembaruan timeline sesi yang sedang dibuka
setInterval(() => {
  if (S.open && S.tab === "agents" && nav.cur === "detail" && detail) loadTimeline();
}, 1200);

export function initAgents() {
  poll();
  loadHooks();
}
