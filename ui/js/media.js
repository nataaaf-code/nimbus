// Media: Pemutar YouTube Music langsung lewat InnerTube + fallback Windows SMTC,
// Lirik Sinkron (LRCLIB + InnerTube), Album Explorer, Antrean, Pencarian, & WASAPI Spectrum.
import { $, $$, S, inv, icon, esc, mmss, on, listen, toast, setTabHeight, T } from "./core.js";

const B = document.body;
const mediaView = $('.view[data-view="media"]');
const audio = $("#mAudio") || new Audio();
audio.preload = "metadata";

let md = null; // state tampilan: pemutar lokal atau fallback SMTC
let smtc = null; // snapshot Windows SMTC saat pemutar lokal idle
let pos = 0; // anchor detik SMTC
let at = 0; // performance.now() saat anchor di-set
let lastShownSec = -1;
let lastShownDur = -1;
let shownCover = "";
let shownTitle = "";
let trackKey = "";
let subView = "player"; // "player" | "lyrics" | "album" | "queue" | "search" | "setup"

let lastPollAt = 0;
let progAnim = null;

// Seek drag & lock state
let seeking = false;
let seekVal = 0;

// Volume drag & lock guard
let volDragging = false;
let volLockUntil = 0;

// Lyrics state
const LYRIC_LEAD_SEC = 0.35; // muncul 0.35 detik lebih awal dari kalimat asli di musik
const LYRIC_OFFSETS_KEY = "nimbus.media.lyricOffsets";
let lyrOffsets = {};
try {
  const savedOff = JSON.parse(localStorage.getItem(LYRIC_OFFSETS_KEY) || "{}");
  if (savedOff && typeof savedOff === "object") lyrOffsets = savedOff;
} catch {}
let lyrData = null; // LyricsResult
let lyrKey = "";
let lyrLoading = false;
let lyrActiveIdx = -1;
let lyrActiveList = -1;
let lyrActiveCue = false;
let lyrUserScrollUntil = 0;
let lyrLiveIdx = -2;
let lyrLivePos = 0;
let lyrLiveEl = null;
let lyrLiveText = "";
let lyrSpecScale = 0.9;
let lyrGlowE = 0;

// Album state
let albData = null; // AlbumDetail
let albKey = ""; // trackKey atau "id:<browseId>"
let albCustom = false; // true jika dibuka dari hasil pencarian album
let albLoading = false;

// Queue & Search state
let searchFilter = "songs";
let searchTimer = null;
let searchSongs = [];

// ======================================================================
// Pemutar Lokal (InnerTube) + Fallback Windows SMTC
// ======================================================================

const PREFS_KEY = "nimbus.media.prefs";
const local = {
  queue: [],
  index: -1,
  playing: false,
  loading: false,
  error: "",
  volume: 1,
  muted: false,
  shuffle: false,
  repeat: "NONE",
};
let playToken = 0;
let retriedStream = false;

try {
  const saved = JSON.parse(localStorage.getItem(PREFS_KEY) || "{}");
  if (Number.isFinite(saved.volume)) local.volume = Math.min(1, Math.max(0, saved.volume));
  if (typeof saved.muted === "boolean") local.muted = saved.muted;
  if (typeof saved.shuffle === "boolean") local.shuffle = saved.shuffle;
  if (["NONE", "ALL", "ONE"].includes(saved.repeat)) local.repeat = saved.repeat;
} catch {}

function savePrefs() {
  try {
    localStorage.setItem(
      PREFS_KEY,
      JSON.stringify({
        volume: local.volume,
        muted: local.muted,
        shuffle: local.shuffle,
        repeat: local.repeat,
      }),
    );
  } catch {}
}

function parseDuration(text) {
  if (!text) return 0;
  const parts = String(text).split(":").map(Number);
  if (parts.some((n) => !Number.isFinite(n))) return 0;
  return parts.reduce((acc, n) => acc * 60 + n, 0);
}

function currentTrack() {
  return local.index >= 0 ? local.queue[local.index] || null : null;
}

function isLocal() {
  return !!currentTrack();
}

function emptyState() {
  return {
    source: "none",
    connected: false,
    auth_needed: false,
    has_track: false,
    title: "",
    artist: "",
    album: null,
    video_id: null,
    playlist_id: null,
    url: null,
    playing: false,
    pos: 0,
    dur: 0,
    cover: null,
    volume: null,
    muted: false,
    shuffle: null,
    repeat: null,
    like_state: null,
  };
}

function buildLocalState() {
  const t = currentTrack();
  const dur = !local.loading && Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : t?.dur || 0;
  return {
    source: "ytm",
    connected: true,
    auth_needed: false,
    has_track: !!t,
    title: t?.title || "",
    artist: t?.artist || "",
    album: t?.album || null,
    video_id: t?.video_id || null,
    playlist_id: null,
    url: null,
    playing: !!local.playing,
    pos: !local.loading && Number.isFinite(audio.currentTime) ? audio.currentTime : 0,
    dur,
    cover: t?.cover || null,
    volume: Math.round(local.volume * 100),
    muted: local.muted,
    shuffle: local.shuffle,
    repeat: local.repeat,
    like_state: "INDIFFERENT",
    loading: local.loading,
    error: local.error,
  };
}

function curPos() {
  if (seeking) return seekVal;
  if (isLocal()) return local.loading ? 0 : Math.max(0, audio.currentTime || 0);
  const d = md?.dur || 0;
  const p = md?.playing ? pos + (performance.now() - at) / 1000 : pos;
  return d > 0 ? Math.min(d, Math.max(0, p)) : Math.max(0, p);
}

function currentTrackKey(m) {
  if (!m || !m.has_track) return "";
  return m.video_id ? `v:${m.video_id}` : `t:${m.title || ""}|${m.artist || ""}`;
}

function handleTrackChangeIfNeeded(newPos) {
  const nk = currentTrackKey(md);
  if (nk === trackKey) return false;
  trackKey = nk;
  lyrActiveIdx = -1;
  lyrActiveCue = false;
  resetLiveLyric();
  pos = Number.isFinite(newPos) ? Math.max(0, newPos) : 0;
  at = performance.now();
  if (nk) {
    ensureLyrics(false);
    if (subView === "album" && !albCustom) ensureTrackAlbum();
    if (subView === "queue") loadQueue();
  } else {
    lyrData = null;
    lyrKey = "";
    renderLyrics();
  }
  return true;
}

function refresh() {
  md = isLocal() ? buildLocalState() : smtc || emptyState();
  handleTrackChangeIfNeeded(md.pos || 0);
  draw();
}

function normalizeTrack(t, albumFallback = null) {
  return {
    video_id: t.video_id || t.videoId || "",
    title: t.title || "",
    artist: t.artist || "",
    album: t.album || albumFallback || null,
    cover: t.cover || null,
    dur: Number.isFinite(t.dur) ? t.dur : parseDuration(t.duration),
  };
}

function rowActivate(el, fn) {
  el.tabIndex = 0;
  el.setAttribute("role", "button");
  el.addEventListener("keydown", (e) => {
    if (e.target !== el) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn(e);
    }
  });
}

function setQueue(tracks, index = 0) {
  local.queue = tracks.map((t) => normalizeTrack(t));
  local.index = Math.max(0, Math.min(index, local.queue.length - 1));
  local.error = "";
  if (!local.queue.length) {
    stopLocal();
    return;
  }
  playAt(local.index);
}

function stopLocal() {
  playToken++;
  local.index = -1;
  local.queue = [];
  local.playing = false;
  local.loading = false;
  local.error = "";
  audio.pause();
  audio.removeAttribute("src");
  audio.load();
  refresh();
  if (subView === "queue") loadQueue();
  poll();
}

async function playAt(index, autoplay = true, isRetry = false) {
  if (index < 0 || index >= local.queue.length) return;
  local.index = index;
  local.loading = true;
  local.error = "";
  if (!isRetry) retriedStream = false;
  audio.pause();
  const t = currentTrack();
  const token = ++playToken;
  refresh();
  try {
    const info = await inv("ytm_stream", { videoId: t.video_id });
    if (token !== playToken) return;
    if (!t.title && info.title) t.title = info.title;
    if (!t.artist && info.artist) t.artist = info.artist;
    if (!t.cover && info.cover) t.cover = info.cover;
    if (info.duration > 0) t.dur = info.duration;
    audio.src = info.url;
    audio.volume = local.volume;
    audio.muted = local.muted;
    if (autoplay) await audio.play();
    else audio.pause();
    if (token !== playToken) return;
    local.loading = false;
    local.playing = !audio.paused;
    refresh();
  } catch (e) {
    if (token !== playToken) return;
    local.loading = false;
    local.playing = false;
    local.error = "Gagal memutar lagu ini";
    refresh();
    toast(String(e), "err", 5000);
  }
}

async function togglePlay() {
  if (isLocal()) {
    if (audio.paused) {
      try {
        await audio.play();
      } catch {
        // Stream kedaluwarsa: ambil URL baru lalu lanjut dari posisi terakhir.
        const resumeAt = audio.currentTime || 0;
        await playAt(local.index, false);
        if (isLocal()) {
          try {
            audio.currentTime = resumeAt;
          } catch {}
          audio.play().catch(() => {});
        }
      }
    } else {
      audio.pause();
    }
    return;
  }
  if (!T) return;
  try {
    await inv("media_ctl", { op: "toggle" });
    setTimeout(poll, 220);
  } catch (e) {
    toast(String(e), "err", 3000);
  }
}

function nextIndex() {
  if (!local.queue.length) return -1;
  if (local.shuffle && local.queue.length > 1) {
    let n = local.index;
    while (n === local.index) n = Math.floor(Math.random() * local.queue.length);
    return n;
  }
  return (local.index + 1) % local.queue.length;
}

function prevIndex() {
  if (!local.queue.length) return -1;
  if (local.shuffle && local.queue.length > 1) {
    let n = local.index;
    while (n === local.index) n = Math.floor(Math.random() * local.queue.length);
    return n;
  }
  return (local.index - 1 + local.queue.length) % local.queue.length;
}

async function nextTrack() {
  if (isLocal()) {
    playAt(nextIndex());
    return;
  }
  if (!T) return;
  try {
    await inv("media_ctl", { op: "next" });
    setTimeout(poll, 220);
  } catch (e) {
    toast(String(e), "err", 3000);
  }
}

async function prevTrack() {
  if (isLocal()) {
    playAt(prevIndex());
    return;
  }
  if (!T) return;
  try {
    await inv("media_ctl", { op: "prev" });
    setTimeout(poll, 220);
  } catch (e) {
    toast(String(e), "err", 3000);
  }
}

function onEnded() {
  if (local.repeat === "ONE") {
    audio.currentTime = 0;
    audio.play().catch(() => {});
    return;
  }
  const atEnd = local.index >= local.queue.length - 1;
  if (atEnd && local.repeat === "NONE" && !local.shuffle) {
    local.playing = false;
    refresh();
    return;
  }
  playAt(nextIndex());
}

function addNext(track) {
  const t = normalizeTrack(track);
  if (!t.video_id) return;
  if (local.index < 0) {
    local.queue = [t];
    local.index = 0;
    playAt(0);
    return;
  }
  local.queue.splice(local.index + 1, 0, t);
  refresh();
  if (subView === "queue") loadQueue();
  toast("Ditambahkan ke putar berikutnya");
}

function removeQueueAt(index) {
  if (index < 0 || index >= local.queue.length) return;
  const wasCurrent = index === local.index;
  local.queue.splice(index, 1);
  if (!local.queue.length) {
    stopLocal();
    return;
  }
  if (index < local.index) local.index--;
  if (wasCurrent) {
    local.index = Math.min(local.index, local.queue.length - 1);
    playAt(local.index);
  } else {
    refresh();
    if (subView === "queue") loadQueue();
  }
}

function clearQueue() {
  if (!local.queue.length) return;
  const t = currentTrack();
  local.queue = t ? [t] : [];
  local.index = t ? 0 : -1;
  refresh();
  if (subView === "queue") loadQueue();
}

audio.addEventListener("timeupdate", () => {
  if (isLocal()) tick();
});

audio.addEventListener("play", () => {
  local.playing = true;
  refresh();
  updateProgLoop();
});

audio.addEventListener("pause", () => {
  local.playing = false;
  refresh();
  updateProgLoop();
});

audio.addEventListener("ended", onEnded);

audio.addEventListener("loadedmetadata", () => {
  if (!isLocal()) return;
  refresh();
  if (!lyrFetchedWithDur && !lyrLoading && !lyrData?.synced && Number.isFinite(audio.duration) && audio.duration > 5) {
    ensureLyrics(true);
  }
});

audio.addEventListener("error", async () => {
  if (!isLocal() || local.loading || !audio.currentSrc) return;
  if (!retriedStream) {
    retriedStream = true;
    const resumeAt = audio.currentTime || 0;
    await playAt(local.index, false, true);
    if (isLocal()) {
      try {
        audio.currentTime = resumeAt;
      } catch {}
      audio.play().catch(() => {});
    }
    return;
  }
  local.playing = false;
  local.error = "Stream audio tidak tersedia";
  refresh();
  toast("Stream audio tidak tersedia. Coba lagu lain.", "err", 4000);
});

// ======================================================================
// Smooth 60fps Progress Loop (hanya jalan saat tab Media terbuka & main)
// ======================================================================

function progStep() {
  if (!S.open || S.tab !== "media" || (!md?.playing && !seeking)) {
    progAnim = null;
    return;
  }
  tick();
  progAnim = requestAnimationFrame(progStep);
}

function updateProgLoop() {
  const need = S.open && S.tab === "media" && (!!md?.playing || seeking);
  if (need && !progAnim) {
    progAnim = requestAnimationFrame(progStep);
  } else if (!need && progAnim) {
    cancelAnimationFrame(progAnim);
    progAnim = null;
  }
}

// ======================================================================
// Sub-navigasi Tab Media & Animasi UI Minimalis
// ======================================================================

const EQ_HTML = `<span class="m-eq" aria-hidden="true"><i></i><i></i><i></i></span>`;
const DOTS_HTML = `<span class="m-dots" aria-hidden="true"><i></i><i></i><i></i></span>`;

function moveSegInd(wrapSel, indSel) {
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

function replayAnim(el, cls) {
  if (!el) return;
  el.classList.remove(cls);
  void el.offsetWidth;
  el.classList.add(cls);
}

function syncVolFill(el) {
  if (!el) return;
  const pct = local.muted ? 0 : Math.max(0, Math.min(100, Number(el.value) || 0));
  el.style.setProperty("--vol", pct + "%");
}

function setSubView(name) {
  subView = name;
  $$("#mTabs button").forEach((b) => b.classList.toggle("on", b.dataset.mv === name));
  $("#ytmPill")?.classList.toggle("on", name === "setup");
  $$(".m-pane").forEach((p) => p.classList.toggle("on", p.dataset.mp === name));
  requestAnimationFrame(() => {
    moveSegInd("#mTabs", "#mTabInd");
    if (name === "search") moveSegInd("#mSearchFilter", "#mFilterInd");
  });

  setTabHeight("media", name === "player" ? 274 : 320);

  if (name === "player") {
    if (S.open && S.tab === "media") startSpec();
    syncLyricHighlight(true);
  } else {
    stopSpec();
  }

  if (name === "lyrics") {
    ensureLyrics(false);
    syncLyricHighlight(true);
  } else if (name === "album") {
    if (!albCustom) ensureTrackAlbum();
  } else if (name === "queue") {
    loadQueue();
  } else if (name === "search") {
    setTimeout(() => $("#mSearchIn")?.focus(), 40);
  } else if (name === "setup") {
    loadYtmStatus();
  }
}

// ======================================================================
// Polling & Render Pemutar Utama
// ======================================================================

async function poll() {
  if (!T || isLocal()) return;
  lastPollAt = Date.now();
  let next = null;
  try {
    next = await inv("ytm_state");
  } catch {
    next = null;
  }
  smtc = next && next.has_track ? next : null;
  md = smtc || emptyState();
  handleTrackChangeIfNeeded(md.pos || 0);
  if (smtc) {
    pos = smtc.pos || 0;
    at = performance.now();
  }
  draw();
}

function draw() {
  const has = !!md?.has_track;
  const isYtm = !!md?.connected;
  mediaView?.classList.toggle("ytm-on", isYtm);

  // Status pill
  const pill = $("#ytmPill");
  const pillTxt = $("#ytmPillTxt");
  if (pill && pillTxt) {
    pill.classList.toggle("ok", isYtm && !local.error);
    pill.classList.toggle("warn", !!local.error);
    pillTxt.textContent = isYtm ? (local.loading ? "Menyiapkan…" : local.playing ? "Nimbus · Memutar" : "Pemutar Nimbus") : "Media Windows";
  }

  // Title, artist, album
  const title = has ? md.title || "Tanpa judul" : "Tidak ada media";
  let artist;
  if (!has) {
    artist = T ? "Cari lagu di tab Cari, atau putar media lain di Windows" : "Jalankan sebagai aplikasi Windows";
  } else if (local.loading) {
    artist = "Menyiapkan audio…";
  } else if (local.error) {
    artist = local.error;
  } else {
    artist = md.artist || (isYtm ? "YouTube Music" : "Media Windows");
  }

  if (title !== shownTitle) {
    const hadTitle = !!shownTitle;
    shownTitle = title;
    $("#mTitle").textContent = title;
    if (hadTitle) replayAnim($(".m-t", mediaView), "swap");
  }
  $("#mArtist").textContent = artist;

  const albBtn = $("#mAlbumBtn");
  const albTxt = $("#mAlbumTxt");
  if (albBtn && albTxt) {
    if (has && md.album) {
      albTxt.textContent = md.album;
      albBtn.hidden = false;
    } else if (has) {
      albTxt.textContent = "Lihat album";
      albBtn.hidden = false;
    } else {
      albBtn.hidden = true;
    }
  }

  // Play/pause buttons
  const playHtml = icon(md?.playing ? "pause" : "play");
  $("#mPlay").innerHTML = playHtml;
  const lyrPlay = $("#mLyrPlay");
  if (lyrPlay) lyrPlay.innerHTML = playHtml;
  B.classList.toggle("playing", !!md?.playing);

  // Cover art (player + lyrics mini-bar + flat ambient wash)
  const art = $("#mArt");
  const lyrArt = $("#mLyrArt");
  const bgEl = $("#mBg");
  const cover = has ? md?.cover || "" : "";
  if (cover !== shownCover) {
    const hadCover = !!shownCover;
    shownCover = cover;
    const bg = cover ? `url("${cover}")` : "";
    art.style.backgroundImage = bg;
    art.classList.toggle("cover", !!cover);
    if (lyrArt) lyrArt.style.backgroundImage = bg;
    if (bgEl) bgEl.style.backgroundImage = bg;
    mediaView?.classList.toggle("has-cover", !!cover);
    if (hadCover || cover) replayAnim(art, "swap");
  }
  const artTxt = art.querySelector(".art-ov span");
  if (artTxt) artTxt.textContent = has ? md?.album || md?.artist || "" : "";

  const lt = $("#mLyrTitle");
  const la = $("#mLyrArtist");
  if (lt) lt.textContent = title;
  if (la) la.textContent = artist;

  // Kontrol pemutar lokal
  if (isYtm) {
    $("#mShuffle")?.classList.toggle("on", !!local.shuffle);
    const repBtn = $("#mRepeat");
    if (repBtn) {
      const mode = local.repeat || "NONE";
      repBtn.classList.toggle("on", mode === "ALL" || mode === "ONE");
      repBtn.innerHTML = icon(mode === "ONE" ? "repeat1" : "repeat");
      repBtn.title = mode === "ONE" ? "Ulangi: Satu lagu" : mode === "ALL" ? "Ulangi: Semua" : "Ulangi: Mati";
    }

    const muteBtn = $("#mMute");
    if (muteBtn) {
      muteBtn.innerHTML = icon(local.muted || local.volume === 0 ? "mute" : "vol");
      muteBtn.classList.toggle("on", !!local.muted);
    }
    const volIn = $("#mVol");
    if (volIn) {
      if (!volDragging && performance.now() > volLockUntil) {
        volIn.value = String(Math.round(local.volume * 100));
      }
      syncVolFill(volIn);
    }
  }

  // Highlight lagu aktif bila sedang melihat Album
  if (subView === "album" && albData) highlightActiveAlbumTrack();

  tick();
  updateProgLoop();
}

function tick() {
  const d = md?.dur || 0;
  const p = curPos();
  const pct = d > 0 ? Math.min(100, Math.max(0, (p / d) * 100)) : 0;
  const seekEl = $("#mSeekTrack");

  const pFloor = Math.floor(p);
  if (pFloor !== lastShownSec) {
    lastShownSec = pFloor;
    $("#mPos").textContent = mmss(pFloor);
    seekEl?.setAttribute("aria-valuenow", String(pFloor));
  }
  const dFloor = Math.floor(d);
  if (dFloor !== lastShownDur) {
    lastShownDur = dFloor;
    $("#mDur").textContent = mmss(dFloor);
    seekEl?.setAttribute("aria-valuemax", String(dFloor));
  }

  $("#mBar").style.width = pct.toFixed(3) + "%";
  const knob = $("#mKnob");
  if (knob) knob.style.left = pct.toFixed(3) + "%";

  syncLyricHighlight(false);
}

// ======================================================================
// Seek Bar Interaktif
// ======================================================================

const seekTrack = $("#mSeekTrack");
if (seekTrack) {
  const pctFromEv = (e) => {
    const r = seekTrack.getBoundingClientRect();
    if (!r.width) return 0;
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  };
  seekTrack.addEventListener("pointerdown", (e) => {
    if (!isLocal() || !md?.dur) return;
    seeking = true;
    seekTrack.classList.add("drag");
    seekTrack.setPointerCapture(e.pointerId);
    seekVal = pctFromEv(e) * md.dur;
    tick();
    updateProgLoop();
  });
  seekTrack.addEventListener("pointermove", (e) => {
    if (!seeking || !md?.dur) return;
    seekVal = pctFromEv(e) * md.dur;
    tick();
  });
  const endSeek = (e) => {
    if (!seeking) return;
    seeking = false;
    seekTrack.classList.remove("drag");
    try {
      seekTrack.releasePointerCapture(e.pointerId);
    } catch {}
    const target = Math.max(0, Math.min(md?.dur || 0, Math.round(seekVal)));
    pos = target;
    at = performance.now();
    if (isLocal()) {
      try {
        audio.currentTime = target;
      } catch {}
    }
    tick();
    updateProgLoop();
  };
  seekTrack.addEventListener("pointerup", endSeek);
  seekTrack.addEventListener("pointercancel", endSeek);
  seekTrack.addEventListener("keydown", (e) => {
    if (!isLocal() || !md?.dur) return;
    const step = e.shiftKey ? 30 : 5;
    let target = null;
    if (e.key === "ArrowRight" || e.key === "ArrowUp") target = curPos() + step;
    else if (e.key === "ArrowLeft" || e.key === "ArrowDown") target = curPos() - step;
    else if (e.key === "Home") target = 0;
    else if (e.key === "End") target = md.dur;
    if (target === null) return;
    e.preventDefault();
    target = Math.max(0, Math.min(md.dur, target));
    pos = target;
    at = performance.now();
    try {
      audio.currentTime = target;
    } catch {}
    tick();
    updateProgLoop();
  });
}

// ======================================================================
// Tombol Kontrol Playback & Volume
// ======================================================================

$("#mPrev").onclick = prevTrack;
$("#mPlay").onclick = togglePlay;
$("#mLyrPlay").onclick = () => $("#mPlay").click();
$("#mNext").onclick = nextTrack;
$("#mShuffle").onclick = () => {
  local.shuffle = !local.shuffle;
  savePrefs();
  refresh();
};
$("#mRepeat").onclick = () => {
  local.repeat = local.repeat === "ALL" ? "ONE" : local.repeat === "ONE" ? "NONE" : "ALL";
  savePrefs();
  refresh();
};
$("#mMute").onclick = () => {
  local.muted = !local.muted;
  audio.muted = local.muted;
  savePrefs();
  refresh();
};

const volEl = $("#mVol");
if (volEl) {
  const endVol = () => {
    volDragging = false;
    volLockUntil = performance.now() + 700;
  };
  volEl.addEventListener("pointerdown", () => {
    volDragging = true;
  });
  volEl.addEventListener("pointerup", endVol);
  volEl.addEventListener("pointercancel", endVol);
  volEl.addEventListener("change", endVol);
  volEl.addEventListener("input", () => {
    const v = Math.max(0, Math.min(100, Number(volEl.value)));
    volLockUntil = performance.now() + 700;
    local.volume = v / 100;
    local.muted = v === 0;
    audio.volume = local.volume;
    audio.muted = local.muted;
    syncVolFill(volEl);
    savePrefs();
    refresh();
  });
}

// Klik cover atau badge album → langsung buka detail Album
$("#mArt").onclick = () => {
  if (!md?.has_track) return;
  albCustom = false;
  setSubView("album");
};
$("#mAlbumBtn").onclick = () => {
  if (!md?.has_track) return;
  albCustom = false;
  setSubView("album");
};
$("#mLyrHit").onclick = () => setSubView("lyrics");
$("#mLyrSlow")?.addEventListener("click", (e) => {
  e.stopPropagation();
  nudgeLyricOffset(-0.5);
});
$("#mLyrFast")?.addEventListener("click", (e) => {
  e.stopPropagation();
  nudgeLyricOffset(0.5);
});
$("#mLyrPreview")?.addEventListener(
  "wheel",
  (e) => {
    if (!md?.has_track || !getLiveLyricLines(lyrData)) return;
    e.preventDefault();
    nudgeLyricOffset(e.deltaY > 0 ? -0.25 : 0.25);
  },
  { passive: false },
);

// ======================================================================
// Lirik Sinkron (YouTube Music Timed Lyrics + LRCLIB) + Pengatur Timing
// ======================================================================

let lyrFetchedWithDur = false;

function getLyricOffset() {
  if (!trackKey) return 0;
  const v = Number(lyrOffsets[trackKey]);
  return Number.isFinite(v) ? v : 0;
}

function fmtOffset(v) {
  const r = Math.round(v * 100) / 100;
  if (Math.abs(r) < 0.01) return "0.0s";
  const sign = r > 0 ? "+" : "−";
  const abs = Math.abs(r);
  return sign + (abs % 1 === 0 ? abs.toFixed(1) : abs.toFixed(2).replace(/0$/, "")) + "s";
}

function renderLyricOffset() {
  const bar = $("#mLyrSync");
  const valEl = $("#mLyrOffVal");
  const hasLyr = !!(md?.has_track && getLiveLyricLines(lyrData));
  if (bar) bar.hidden = !hasLyr;
  if (!valEl) return;
  const v = getLyricOffset();
  valEl.textContent = fmtOffset(v);
  valEl.classList.toggle("zero", Math.abs(v) < 0.01);
}

function setLyricOffset(val, silent = false) {
  if (!trackKey) return;
  const clamped = Math.max(-60, Math.min(60, Math.round(val * 100) / 100));
  if (Math.abs(clamped) < 0.01) {
    delete lyrOffsets[trackKey];
  } else {
    lyrOffsets[trackKey] = clamped;
    const keys = Object.keys(lyrOffsets);
    if (keys.length > 120) delete lyrOffsets[keys[0]];
  }
  try {
    localStorage.setItem(LYRIC_OFFSETS_KEY, JSON.stringify(lyrOffsets));
  } catch {}
  renderLyricOffset();
  syncLyricHighlight(true);
  if (!silent) {
    toast(`Timing lirik: ${fmtOffset(clamped)}`, "ok", 1400);
  }
}

function nudgeLyricOffset(delta) {
  setLyricOffset(getLyricOffset() + delta);
}

function getLiveLyricLines(data) {
  if (!data?.found || data.instrumental || !data.synced || !data.lines?.length) return null;
  return { synced: true, lines: data.lines };
}

function resetLiveLyric() {
  const prev = $("#mLyrPreview");
  const stage = $("#mLyrStage");
  if (stage) stage.innerHTML = "";
  if (prev) prev.hidden = true;
  $("#mSpecWrap")?.classList.remove("has-lyr");
  lyrLiveIdx = -2;
  lyrLivePos = 0;
  lyrLiveEl = null;
  lyrLiveText = "";
}

function updateLiveLyric(lines, idx, singPos, forceInstant) {
  const prev = $("#mLyrPreview");
  const stage = $("#mLyrStage");
  if (!prev || !stage) return;

  const line = idx >= 0 ? lines[idx] : null;
  const text = (line?.text || "").trim();
  const isIdle = !text;
  const displayText = isIdle ? "♪  ·  ·  ·" : text;
  const phase = isIdle ? "idle" : singPos >= line.time ? "sing" : "cue";

  let lp = 0;
  if (phase === "sing") {
    const start = line.time;
    const hasEnd = Number.isFinite(line.end) && line.end > start;
    const nextStart = lines[idx + 1]?.time ?? (md?.dur || start + 4.5);
    const rawSpan = hasEnd ? line.end - start : nextStart - start - 0.12;
    const span = Math.max(0.45, Math.min(hasEnd ? 20 : 12, rawSpan));
    lp = Math.max(0, Math.min(100, ((singPos - start) / span) * 100));
  }

  prev.hidden = false;
  $("#mSpecWrap")?.classList.add("has-lyr");

  if (idx !== lyrLiveIdx || displayText !== lyrLiveText || !lyrLiveEl) {
    const prevIdx = lyrLiveIdx;
    let dir = 0;
    if (prevIdx !== -2 && !forceInstant) {
      if (idx >= 0 && prevIdx >= 0) dir = idx >= prevIdx ? 1 : -1;
      else dir = singPos >= lyrLivePos ? 1 : -1;
    }

    if (lyrLiveEl) {
      const oldEl = lyrLiveEl;
      oldEl.classList.remove("in-up", "in-down", "in-fade");
      oldEl.classList.add(dir < 0 ? "out-down" : "out-up");
      const removeOld = () => {
        if (oldEl.parentNode) oldEl.remove();
      };
      oldEl.addEventListener("animationend", removeOld, { once: true });
      setTimeout(removeOld, 360);
      while (stage.children.length > 3) {
        stage.firstElementChild?.remove();
      }
    }

    const item = document.createElement("span");
    item.className = "m-lyr-item " + (dir > 0 ? "in-up" : dir < 0 ? "in-down" : "in-fade");
    item.dataset.phase = phase;

    const tx = document.createElement("span");
    tx.className = "m-lyr-tx";
    tx.textContent = displayText;
    tx.style.setProperty("--lp", lp.toFixed(1) + "%");

    const fill = document.createElement("span");
    fill.className = "m-lyr-fill";
    fill.setAttribute("aria-hidden", "true");
    fill.textContent = displayText;
    tx.appendChild(fill);

    item.appendChild(tx);
    stage.appendChild(item);
    lyrLiveEl = item;
    lyrLiveIdx = idx;
    lyrLivePos = singPos;
    lyrLiveText = displayText;
  } else if (lyrLiveEl) {
    if (lyrLiveEl.dataset.phase !== phase) {
      lyrLiveEl.dataset.phase = phase;
    }
    const tx = lyrLiveEl.firstElementChild;
    if (tx) {
      tx.style.setProperty("--lp", phase === "sing" ? lp.toFixed(1) + "%" : "0%");
    }
  }
}

async function ensureLyrics(force = false) {
  if (!T || !md?.has_track) {
    lyrData = null;
    lyrFetchedWithDur = false;
    resetLiveLyric();
    renderLyrics();
    return;
  }
  const key = currentTrackKey(md);
  if (!force && key === lyrKey && (lyrData || lyrLoading)) return;
  lyrKey = key;
  lyrLoading = true;
  lyrData = null;
  lyrActiveIdx = -1;
  lyrActiveList = -1;
  lyrActiveCue = false;
  lyrFetchedWithDur = !!(md.dur && md.dur > 5);
  resetLiveLyric();
  renderLyrics();

  try {
    const res = await inv("ytm_lyrics", {
      title: md.title || "",
      artist: md.artist || "",
      album: md.album || null,
      duration: md.dur > 0 ? md.dur : null,
      videoId: md.video_id || null,
      force,
    });
    if (lyrKey === key) {
      lyrData = res;
      lyrLoading = false;
      renderLyrics();
      syncLyricHighlight(true);
    }
  } catch (e) {
    if (lyrKey === key) {
      lyrLoading = false;
      lyrData = { found: false, synced: false, instrumental: false, source: "", lines: [], plain: null };
      renderLyrics();
    }
  }
}

function renderLyrics() {
  const box = $("#mLyrList");
  const src = $("#mLyrSrc");
  if (!box) return;
  renderLyricOffset();

  if (!md?.has_track) {
    if (src) src.textContent = "Lirik";
    resetLiveLyric();
    box.innerHTML = `<div class="m-empty"><b>Belum ada lagu diputar</b><span>Putar lagu dari tab Cari untuk melihat lirik otomatis.</span></div>`;
    return;
  }
  if (lyrLoading) {
    if (src) src.textContent = "Mencari…";
    resetLiveLyric();
    box.innerHTML = `<div class="m-empty">${DOTS_HTML}<b>Mencari lirik sinkron…</b><span>Memeriksa YouTube Music &amp; LRCLIB</span></div>`;
    return;
  }
  if (!lyrData || !lyrData.found) {
    if (src) src.textContent = "Tidak ada";
    resetLiveLyric();
    box.innerHTML = `<div class="m-empty"><b>Lirik tidak ditemukan</b><span>Coba tekan refresh atau lagu ini mungkin belum memiliki lirik.</span></div>`;
    return;
  }
  if (src) src.textContent = lyrData.source || "Lirik";

  if (lyrData.instrumental) {
    resetLiveLyric();
    box.innerHTML = `<div class="m-empty"><b>Instrumental</b><span>Lagu ini tidak memiliki vokal / lirik.</span></div>`;
    return;
  }
  if (lyrData.synced && lyrData.lines?.length) {
    box.innerHTML = lyrData.lines
      .map((l, i) => {
        const text = (l.text || "").trim();
        return `<button class="lyr-line${text ? "" : " gap"}" data-idx="${i}" data-t="${l.time}">${esc(text || "♪  ·  ·  ·")}</button>`;
      })
      .join("");
    $$(".lyr-line", box).forEach((el) => {
      el.onclick = () => {
        if (!isLocal()) return;
        const t = parseFloat(el.dataset.t);
        if (!Number.isFinite(t)) return;
        const target = Math.max(0, Math.min(md?.dur || t, t - getLyricOffset()));
        pos = target;
        at = performance.now();
        try {
          audio.currentTime = target;
        } catch {}
        tick();
        updateProgLoop();
      };
    });
    return;
  }
  if (lyrData.plain) {
    resetLiveLyric();
    box.innerHTML = `<div class="lyr-plain">${esc(lyrData.plain)}</div>`;
  }
}

function syncLyricHighlight(forceScroll) {
  const pack = md?.has_track ? getLiveLyricLines(lyrData) : null;
  if (!pack || !pack.lines.length) {
    resetLiveLyric();
    return;
  }
  const singPos = curPos() + getLyricOffset();
  const leadPos = singPos + LYRIC_LEAD_SEC;
  const lines = pack.lines;
  let idx = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].time <= leadPos) idx = i;
    else break;
  }

  let liveIdx = idx;
  if (idx >= 0) {
    const cur = lines[idx];
    const next = lines[idx + 1];
    const hasEnd = Number.isFinite(cur.end) && cur.end > cur.time;
    if (!(cur.text || "").trim()) {
      liveIdx = -1;
    } else if (hasEnd && singPos > cur.end + 0.35 && (!next || next.time - cur.end >= 1.5)) {
      liveIdx = -1;
    }
  }

  // Live lyrics melayang di tengah atas spectrum (selalu diperbarui mulus 60fps)
  updateLiveLyric(lines, liveIdx, singPos, forceScroll);

  const listActive = liveIdx >= 0 ? liveIdx : idx >= 0 && !(lines[idx].text || "").trim() ? idx : -1;
  const isCue = liveIdx >= 0 && singPos < lines[liveIdx].time;
  const stateChanged = idx !== lyrActiveIdx || listActive !== lyrActiveList || isCue !== lyrActiveCue;
  if (!stateChanged && !forceScroll) return;
  const activeChanged = listActive !== lyrActiveList;
  lyrActiveIdx = idx;
  lyrActiveList = listActive;
  lyrActiveCue = isCue;

  const box = $("#mLyrList");
  if (!box || subView !== "lyrics") return;
  const els = $$(".lyr-line", box);
  const pastLimit = listActive >= 0 ? listActive : idx + 1;
  els.forEach((el, i) => {
    const active = i === listActive;
    el.classList.toggle("active", active);
    el.classList.toggle("cue", active && isCue);
    el.classList.toggle("past", i < pastLimit);
  });
  if ((activeChanged || forceScroll) && listActive >= 0 && els[listActive] && (forceScroll || Date.now() > lyrUserScrollUntil)) {
    els[listActive].scrollIntoView({ behavior: forceScroll ? "auto" : "smooth", block: "center" });
  }
}

$$("#mLyrSync .m-sync-btn").forEach((b) => {
  b.onclick = () => nudgeLyricOffset(Number(b.dataset.d) || 0);
});
$("#mLyrOffVal")?.addEventListener("click", () => setLyricOffset(0));

$("#mLyrList")?.addEventListener(
  "wheel",
  () => {
    lyrUserScrollUntil = Date.now() + 4500;
  },
  { passive: true },
);
$("#mLyrRefresh").onclick = () => ensureLyrics(true);

// ======================================================================
// Album Explorer (InnerTube)
// ======================================================================

async function ensureTrackAlbum() {
  if (!T || !md?.has_track) {
    albData = null;
    renderAlbum("Putar lagu terlebih dahulu untuk melihat albumnya.");
    return;
  }
  const key = currentTrackKey(md);
  if (key === albKey && (albData || albLoading)) {
    highlightActiveAlbumTrack();
    return;
  }
  albKey = key;
  albCustom = false;
  albLoading = true;
  albData = null;
  renderAlbum("Memuat informasi album dari YouTube Music…");

  try {
    const res = await inv("ytm_track_album", {
      videoId: md.video_id || null,
      title: md.title || "",
      artist: md.artist || "",
      album: md.album || null,
    });
    if (albKey === key) {
      albData = res;
      albLoading = false;
      renderAlbum();
    }
  } catch (e) {
    if (albKey === key) {
      albLoading = false;
      renderAlbum(String(e));
    }
  }
}

async function openAlbumById(browseId) {
  if (!T || !browseId) return;
  albCustom = true;
  albKey = "id:" + browseId;
  albLoading = true;
  albData = null;
  setSubView("album");
  renderAlbum("Memuat daftar lagu album…");
  try {
    const res = await inv("ytm_album", { browseId });
    if (albKey === "id:" + browseId) {
      albData = res;
      albLoading = false;
      renderAlbum();
    }
  } catch (e) {
    albLoading = false;
    renderAlbum(String(e));
  }
}

function renderAlbum(msg = "") {
  const art = $("#mAlbArt");
  const title = $("#mAlbTitle");
  const sub = $("#mAlbSub");
  const tracks = $("#mAlbTracks");
  const fromNow = $("#mAlbFromNow");
  const playAll = $("#mAlbPlayAll");
  const openBtn = $("#mAlbOpen");
  if (!tracks) return;

  if (fromNow) fromNow.hidden = !(albCustom && md?.has_track);

  if (!albData) {
    if (art) {
      art.style.backgroundImage = "";
      art.classList.remove("cover");
    }
    if (title) title.textContent = albLoading ? "Memuat album…" : "Album";
    if (sub) sub.textContent = msg || "—";
    if (playAll) playAll.disabled = true;
    if (openBtn) openBtn.disabled = true;
    tracks.innerHTML = `<div class="m-empty">${albLoading ? DOTS_HTML : ""}<b>${esc(albLoading ? "Mengambil data album…" : "Belum ada album")}</b><span>${esc(msg || "Pilih lagu atau cari album di tab Cari.")}</span></div>`;
    return;
  }

  if (art) {
    art.style.backgroundImage = albData.cover ? `url("${albData.cover}")` : "";
    art.classList.toggle("cover", !!albData.cover);
  }
  if (title) title.textContent = albData.title || "Album";
  if (sub) {
    const parts = [albData.artist, albData.year, albData.meta].filter(Boolean);
    sub.textContent = parts.join(" · ");
  }
  if (playAll) playAll.disabled = !albData.tracks?.length;
  if (openBtn) openBtn.disabled = !albData.url;

  if (!albData.tracks?.length) {
    tracks.innerHTML = `<div class="m-empty"><b>Tidak ada daftar lagu</b></div>`;
    return;
  }

  tracks.innerHTML = albData.tracks
    .map((t, i) => {
      const isNow = md?.video_id && t.video_id === md.video_id;
      return `
        <div class="m-row${isNow ? " now" : ""}" style="--ri:${Math.min(i, 14)}" data-vid="${esc(t.video_id)}">
          <span class="m-row-idx">${isNow ? EQ_HTML : t.index}</span>
          <div class="m-row-b">
            <b>${esc(t.title)}</b>
            <span>${esc(t.artist)}</span>
          </div>
          <span class="m-row-dur">${esc(t.duration)}</span>
          <div class="m-row-acts">
            <button class="m-row-btn" data-add="${esc(t.video_id)}" title="Putar berikutnya">${icon("plus")}</button>
          </div>
        </div>`;
    })
    .join("");

  $$(".m-row", tracks).forEach((row) => {
    const play = () => {
      const idx = albData.tracks.findIndex((t) => t.video_id === row.dataset.vid);
      const list = albData.tracks.map((t) => normalizeTrack(t, albData.title));
      setQueue(list, idx);
    };
    rowActivate(row, play);
    row.onclick = (e) => {
      const add = e.target.closest("[data-add]");
      if (add) {
        e.stopPropagation();
        const t = albData.tracks.find((x) => x.video_id === add.dataset.add);
        if (t) addNext({ ...t, album: albData.title, cover: albData.cover });
        return;
      }
      play();
    };
  });
}

function highlightActiveAlbumTrack() {
  $$("#mAlbTracks .m-row").forEach((row, i) => {
    const isNow = !!(md?.video_id && row.dataset.vid === md.video_id);
    const wasNow = row.classList.contains("now");
    row.classList.toggle("now", isNow);
    const idx = row.querySelector(".m-row-idx");
    if (idx && isNow !== wasNow) idx.innerHTML = isNow ? EQ_HTML : String(i + 1);
  });
}

$("#mAlbPlayAll").onclick = () => {
  if (!albData?.tracks?.length) return;
  toast(`Memutar album: ${albData.title}`);
  setQueue(
    albData.tracks.map((t) => normalizeTrack(t, albData.title)),
    0,
  );
};
$("#mAlbOpen").onclick = () => {
  if (albData?.url) inv("ytm_open_url", { url: albData.url }).catch((e) => toast(String(e), "err"));
};
$("#mAlbFromNow").onclick = () => {
  albCustom = false;
  albKey = "";
  ensureTrackAlbum();
};

// ======================================================================
// Antrean (Queue)
// ======================================================================

function loadQueue() {
  const list = $("#mQueueList");
  const meta = $("#mQueueMeta");
  if (!list) return;
  const items = local.queue;
  if (meta) meta.textContent = `${items.length} lagu`;
  if (!items.length) {
    list.innerHTML = `<div class="m-empty"><b>Antrean kosong</b><span>Putar lagu dari tab Cari atau dari sebuah album.</span></div>`;
    return;
  }
  list.innerHTML = items
    .map((q, i) => {
      const now = i === local.index;
      return `
      <div class="m-row${now ? " now" : ""}" style="--ri:${Math.min(i, 14)}" data-qidx="${i}">
        <span class="m-row-idx">${now ? EQ_HTML : i + 1}</span>
        <div class="m-row-art" style="${q.cover ? `background-image:url('${esc(q.cover)}')` : ""}"></div>
        <div class="m-row-b">
          <b>${esc(q.title)}</b>
          <span>${esc(q.artist)}</span>
        </div>
        <span class="m-row-dur">${q.dur ? mmss(Math.floor(q.dur)) : ""}</span>
        <div class="m-row-acts">
          <button class="m-row-btn del" data-qdel="${i}" title="Hapus dari antrean">${icon("x")}</button>
        </div>
      </div>`;
    })
    .join("");

  $$(".m-row", list).forEach((row) => {
    const jump = () => playAt(Number(row.dataset.qidx));
    rowActivate(row, jump);
    row.onclick = (e) => {
      const del = e.target.closest("[data-qdel]");
      if (del) {
        e.stopPropagation();
        removeQueueAt(Number(del.dataset.qdel));
        return;
      }
      jump();
    };
  });
}

$("#mQueueRef").onclick = loadQueue;
$("#mQueueClr").onclick = clearQueue;

// ======================================================================
// Pencarian YouTube Music (Lagu & Album)
// ======================================================================

async function runSearch() {
  const input = $("#mSearchIn");
  const resEl = $("#mSearchRes");
  if (!input || !resEl) return;
  const q = input.value.trim();
  $("#mSearchClr").hidden = !q;
  if (!q) {
    resEl.innerHTML = `<div class="m-empty"><b>Cari lagu atau album</b><span>Ketik judul lagu, artis, atau nama album untuk memutar langsung di Nimbus.</span></div>`;
    return;
  }
  resEl.innerHTML = `<div class="m-empty">${DOTS_HTML}<b>Mencari "${esc(q)}"…</b></div>`;
  try {
    const data = await inv("ytm_search", { query: q, filter: searchFilter });
    if (input.value.trim() !== q) return;
    if (searchFilter === "songs") {
      searchSongs = data.songs || [];
      if (!searchSongs.length) {
        resEl.innerHTML = `<div class="m-empty"><b>Tidak ada lagu ditemukan</b></div>`;
        return;
      }
      resEl.innerHTML = searchSongs
        .map(
          (s, i) => `
          <div class="m-row" style="--ri:${Math.min(i, 14)}" data-vid="${esc(s.video_id)}">
            <div class="m-row-art" style="${s.cover ? `background-image:url('${esc(s.cover)}')` : ""}"></div>
            <div class="m-row-b">
              <b>${esc(s.title)}</b>
              <span>${esc(s.artist)}${s.album ? ` · ${esc(s.album)}` : ""}</span>
            </div>
            <span class="m-row-dur">${esc(s.duration)}</span>
            <div class="m-row-acts">
              ${s.album_id ? `<button class="m-row-btn" data-alb="${esc(s.album_id)}" title="Lihat album">${icon("disc")}</button>` : ""}
              <button class="m-row-btn" data-add="${esc(s.video_id)}" title="Putar berikutnya">${icon("plus")}</button>
            </div>
          </div>`,
        )
        .join("");

      $$(".m-row", resEl).forEach((row) => {
        const play = () => {
          const idx = searchSongs.findIndex((s) => s.video_id === row.dataset.vid);
          setQueue(
            searchSongs.map((s) => normalizeTrack(s)),
            Math.max(0, idx),
          );
        };
        rowActivate(row, play);
        row.onclick = (e) => {
          const alb = e.target.closest("[data-alb]");
          if (alb) {
            e.stopPropagation();
            openAlbumById(alb.dataset.alb);
            return;
          }
          const add = e.target.closest("[data-add]");
          if (add) {
            e.stopPropagation();
            const s = searchSongs.find((x) => x.video_id === add.dataset.add);
            if (s) addNext(s);
            return;
          }
          play();
        };
      });
    } else {
      if (!data.albums?.length) {
        resEl.innerHTML = `<div class="m-empty"><b>Tidak ada album ditemukan</b></div>`;
        return;
      }
      resEl.innerHTML = data.albums
        .map(
          (a, i) => `
          <div class="m-row" style="--ri:${Math.min(i, 14)}" data-bid="${esc(a.browse_id)}">
            <div class="m-row-art" style="${a.cover ? `background-image:url('${esc(a.cover)}')` : ""}"></div>
            <div class="m-row-b">
              <b>${esc(a.title)}</b>
              <span>${esc([a.artist, a.year].filter(Boolean).join(" · "))}</span>
            </div>
            <span class="m-row-btn">${icon("chev")}</span>
          </div>`,
        )
        .join("");

      $$(".m-row", resEl).forEach((row) => {
        const open = () => openAlbumById(row.dataset.bid);
        rowActivate(row, open);
        row.onclick = open;
      });
    }
  } catch (e) {
    resEl.innerHTML = `<div class="m-empty"><b>Gagal mencari</b><span>${esc(String(e))}</span></div>`;
  }
}

$("#mSearchIn")?.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(runSearch, 420);
});
$("#mSearchIn")?.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    clearTimeout(searchTimer);
    runSearch();
  }
});
$("#mSearchClr").onclick = () => {
  $("#mSearchIn").value = "";
  runSearch();
  $("#mSearchIn").focus();
};
$$("#mSearchFilter button").forEach((b) => {
  b.onclick = () => {
    searchFilter = b.dataset.f;
    $$("#mSearchFilter button").forEach((x) => x.classList.toggle("on", x === b));
    moveSegInd("#mSearchFilter", "#mFilterInd");
    runSearch();
  };
});

// ======================================================================
// Status Pemutar Nimbus
// ======================================================================

function loadYtmStatus() {
  const badge = $("#ytmStBadge");
  if (badge) {
    badge.className = "pv-st ok";
    badge.textContent = "Aktif";
  }
  const kv = $("#ytmKv");
  if (!kv) return;
  const t = currentTrack();
  kv.innerHTML = `
    <span>Pemutar</span><b class="ok">Langsung di Nimbus</b>
    <span>Sumber audio</span><b>YouTube Music · InnerTube</b>
    <span>Lirik</span><b>YouTube Music + LRCLIB</b>
    <span>Status</span><b>${t ? esc(t.title) : "Siap"}</b>
    <span>Antrean</span><b>${local.queue.length} lagu</b>
  `;
}

// Tab clicks
$$("#mTabs button").forEach((b) => {
  b.onclick = () => setSubView(b.dataset.mv);
});
$("#ytmPill").onclick = () => setSubView(subView === "setup" ? "player" : "setup");

// ======================================================================
// Spektrum audio — data real dari WASAPI loopback (seamless di dasar notch)
// ======================================================================
const N = 52;
const bars = new Float32Array(N); // nilai tampilan utama
const miniBars = new Float32Array(16); // nilai tampilan mini di notch

let liveBars = null; // payload dari Rust [f32;52]
let unlisten = null;

// rAF handles
let anim = null,
  miniAnim = null,
  pulseAnim = null;

// Canvas utama
let sCtx = null,
  sW = 0,
  sH = 0;

// Canvas mini
let mCtx = null,
  mW = 0,
  mH = 0;

// --- Simulasi fallback (pratinjau browser tanpa Tauri) ---
let sPh = 0,
  sE = 0,
  sBt = 0;
function simStep() {
  sPh += 0.016;
  sBt = (sBt + 1) % 26;
  const onBeat = sBt < 3,
    nearBeat = sBt < 7;
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
      v = sE * 0.5 * (0.25 + 0.75 * Math.random()) * (onBeat ? 1.0 : 0.6);
    }
    bars[i] += (Math.max(0, Math.min(1, v)) - bars[i]) * (v > bars[i] ? 0.42 : 0.1);
  }
}

function barColor(t, alpha) {
  const warm = Math.round(210 + t * 20);
  const cool = Math.round(215 + t * 15);
  const r = warm,
    g = warm,
    b = cool;
  return { r, g, b, css: `rgba(${r},${g},${b},${alpha.toFixed(2)})` };
}

function drawSpec() {
  anim = requestAnimationFrame(drawSpec);
  resizeSpec();
  if (!sCtx) return;
  sCtx.clearRect(0, 0, sW, sH);

  if (!T) {
    simStep();
  } else {
    for (let i = 0; i < N; i++) {
      const v = liveBars ? (liveBars[i] ?? 0) : 0;
      bars[i] += (v - bars[i]) * (v > bars[i] ? 0.65 : 0.12);
    }
  }

  const dpr = devicePixelRatio || 1;
  const gap = Math.max(2, Math.round(3 * dpr));
  const bw = (sW - gap * (N - 1)) / N;
  const baseY = sH - 2 * dpr;

  const lyrEl = $("#mLyrPreview");
  const hasLiveLyr = !!(lyrEl && !lyrEl.hidden);
  const targetScale = hasLiveLyr ? 0.54 : 0.9;
  lyrSpecScale += (targetScale - lyrSpecScale) * 0.1;
  const maxH = baseY * lyrSpecScale;

  if (hasLiveLyr) {
    let e = 0;
    for (let i = 4; i < 24; i++) e += bars[i];
    const targetGlow = md?.playing ? Math.min(1, (e / 20) * 1.4) : 0;
    lyrGlowE += (targetGlow - lyrGlowE) * (targetGlow > lyrGlowE ? 0.25 : 0.08);
    lyrEl.style.setProperty("--lyr-glow", lyrGlowE.toFixed(3));
  }

  for (let i = 0; i < N; i++) {
    const v = bars[i];
    const x = i * (bw + gap);
    if (v < 0.004) {
      sCtx.fillStyle = "rgba(255,255,255,0.09)";
      const dh = 1.5 * dpr;
      sCtx.beginPath();
      if (sCtx.roundRect) sCtx.roundRect(x, baseY - dh, bw, dh, dh / 2);
      else sCtx.rect(x, baseY - dh, bw, dh);
      sCtx.fill();
      continue;
    }
    const h = Math.max(2.5 * dpr, v * maxH);
    const ty = baseY - h;
    const t = i / (N - 1);
    const base = barColor(t, 1);
    const peak = Math.max(0, (v - 0.32) / 0.68);
    const r = Math.round(base.r + (255 - base.r) * peak * 0.65);
    const g = Math.round(base.g + (108 - base.g) * peak * 0.65);
    const b = Math.round(base.b + (74 - base.b) * peak * 0.65);
    const alpha = 0.38 + v * 0.56;

    const grad = sCtx.createLinearGradient(0, ty, 0, baseY);
    grad.addColorStop(0, `rgba(${r},${g},${b},${alpha.toFixed(2)})`);
    grad.addColorStop(0.68, `rgba(${r},${g},${b},${(alpha * 0.28).toFixed(2)})`);
    grad.addColorStop(1, `rgba(${r},${g},${b},0)`);
    sCtx.fillStyle = grad;

    const cr = Math.min(bw / 2, 3 * dpr);
    sCtx.beginPath();
    if (sCtx.roundRect) sCtx.roundRect(x, ty, bw, h, [cr, cr, 0, 0]);
    else sCtx.rect(x, ty, bw, h);
    sCtx.fill();
  }
}

function resizeSpec() {
  const c = $("#mSpec");
  if (!c) return;
  const dpr = devicePixelRatio || 1;
  const nw = Math.round(c.offsetWidth * dpr);
  const nh = Math.round(c.offsetHeight * dpr);
  if (!nw || !nh) {
    sCtx = null;
    return;
  }
  if (sCtx && nw === sW && nh === sH) return;
  sW = nw;
  sH = nh;
  c.width = sW;
  c.height = sH;
  sCtx = c.getContext("2d");
}

function startSpec() {
  if (subView !== "player") return;
  if (!anim) drawSpec();
  ensureSpecCapture();
}

function stopSpec() {
  if (anim) {
    cancelAnimationFrame(anim);
    anim = null;
  }
}

// ---- Mini spectrum di notch ----
function drawMini() {
  if (!mCtx || !mW || !mH) {
    miniAnim = null;
    return;
  }
  miniAnim = requestAnimationFrame(drawMini);
  mCtx.clearRect(0, 0, mW, mH);

  for (let i = 0; i < 16; i++) {
    const src = Math.round((i * (N - 1)) / 15);
    const v = liveBars ? (liveBars[src] ?? 0) : bars[src];
    miniBars[i] += (v - miniBars[i]) * (v > miniBars[i] ? 0.6 : 0.14);
  }

  const dpr = devicePixelRatio || 1;
  const gap = Math.max(1, Math.round(1 * dpr));
  const bw = (mW - gap * 15) / 16;

  for (let i = 0; i < 16; i++) {
    const v = miniBars[i];
    if (v < 0.01) continue;
    const h = Math.max(dpr, v * mH * 0.9);
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
  const nw = Math.round(c.offsetWidth * dpr);
  const nh = Math.round(c.offsetHeight * dpr);
  if (!nw || !nh) return;
  mW = nw;
  mH = nh;
  c.width = mW;
  c.height = mH;
  mCtx = c.getContext("2d");
}

function startMini() {
  resizeMini();
  if (!miniAnim) drawMini();
  B.classList.add("has-spec");
}

function stopMini() {
  if (miniAnim) {
    cancelAnimationFrame(miniAnim);
    miniAnim = null;
  }
  B.classList.remove("has-spec");
}

// ---- Line pulse ----
const linePulse = document.getElementById("linePulse");
let pulseE = 0;

function drivePulse() {
  pulseAnim = requestAnimationFrame(drivePulse);
  if (!linePulse) return;

  if (document.body.dataset.mode !== "line") {
    if (+linePulse.style.opacity > 0) linePulse.style.opacity = "0";
    return;
  }

  let bassE = 0;
  if (liveBars) {
    for (let i = 0; i < 15; i++) bassE += liveBars[i] ?? 0;
    bassE /= 15;
  }

  const target = md?.playing ? bassE : 0;
  pulseE += (target - pulseE) * (target > pulseE ? 0.55 : 0.07);

  const o = md?.playing ? Math.min(0.88, 0.08 + pulseE * 0.8) : 0;
  linePulse.style.opacity = o.toFixed(3);
}

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

// ======================================================================
// Inisialisasi
// ======================================================================

export function initMedia() {
  audio.volume = local.volume;
  audio.muted = local.muted;
  syncVolFill($("#mVol"));
  refresh();
  runSearch();

  const syncIndicators = () =>
    requestAnimationFrame(() => {
      moveSegInd("#mTabs", "#mTabInd");
      if (subView === "search") moveSegInd("#mSearchFilter", "#mFilterInd");
    });

  on("tab", (t) => {
    if (t === "media") {
      if (!isLocal() && T && Date.now() - lastPollAt > 2500) poll();
      if (S.open && subView === "player") startSpec();
      syncIndicators();
      tick();
    } else {
      stopSpec();
    }
    updateProgLoop();
  });
  on("open", () => {
    if (S.tab === "media") {
      if (!isLocal() && T && Date.now() - lastPollAt > 2500) poll();
      if (subView === "player") startSpec();
      syncIndicators();
      tick();
    }
    updateProgLoop();
  });
  on("close", () => {
    stopSpec();
    updateProgLoop();
    if (!md?.playing) stopMini();
  });
  on("esc", (ev) => {
    if (S.tab === "media" && subView !== "player") {
      setSubView("player");
      ev.handled = true;
    }
  });

  syncIndicators();
  if (S.open && S.tab === "media" && subView === "player") startSpec();
  ensureSpecCapture();
  drivePulse();
  updateProgLoop();

  if (!T) return;

  poll();
  // Fallback SMTC hanya dipantau saat pemutar lokal idle.
  setInterval(() => {
    if (!isLocal()) poll();
  }, 1800);
}
