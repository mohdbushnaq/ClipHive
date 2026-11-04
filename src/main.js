// ClipHive — Frontend logic (Tauri 2 IPC)
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { open as pickFolder } from '@tauri-apps/plugin-dialog';

// --- Tab navigation ---
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    tab.classList.add('active');
    const section = document.getElementById(`tab-${tab.dataset.tab}`);
    if (section) section.classList.add('active');
    if (tab.dataset.tab === 'library') renderLibrary();
  });
});

// --- Elements ---
const urlInput = document.getElementById('url-input');
const btnAnalyze = document.getElementById('btn-analyze');
const btnScan = document.getElementById('btn-scan');
const errorMsg = document.getElementById('error-msg');
const qualityPanel = document.getElementById('quality-panel');
const scanResults = document.getElementById('scan-results');
const multiResults = document.getElementById('multi-results');
const playlistPanel = document.getElementById('playlist-panel');

let currentVideo = null;      // single analyzed video {url,title,thumbnail_url,...}
let pendingMulti = [];        // [{url,title,thumbnail}] for >1 URL
let currentPlaylist = null;   // analyzed playlist {url,title,count,entries}
const activeDl = new Map();   // id -> {title, thumbnail, file_path, dir}
let settings = null;          // working copy of AppSettings
let playlistQueue = [];       // pending playlist download jobs
let playlistActive = 0;       // jobs currently running
const playlistRunning = new Set(); // download ids started by the playlist queue

function showError(msg){ errorMsg.textContent = msg; errorMsg.hidden = false; }
function clearError(){ errorMsg.hidden = true; errorMsg.textContent=''; }
function errMsg(e){ return typeof e==='string' ? e : ((e && e.message) || String(e)); }

// Thumbnail for a download card / library row:
//  - use the thumbnail URL stored with the download,
//  - otherwise derive a YouTube thumbnail from the video id in the URL,
//  - otherwise show a small placeholder.
function youtubeThumb(url){
  const m = String(url || '').match(/(?:v=|youtu\.be\/|\/shorts\/|\/embed\/|\/live\/)([A-Za-z0-9_-]{11})/);
  return m ? `https://i.ytimg.com/vi/${m[1]}/hqdefault.jpg` : null;
}
function thumbFor(item){
  return (item && item.thumbnail_url) || youtubeThumb(item && item.url) || null;
}
const PLACEHOLDER_THUMB =
  'data:image/svg+xml;utf8,' + encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90" viewBox="0 0 160 90">' +
    '<rect width="160" height="90" rx="8" fill="#1d1c1a"/>' +
    '<path d="M64 30v30l28-15z" fill="#4b4843"/></svg>'
  );

function looksLikePlaylist(url){
  return /[?&]list=|\/playlist(?:[\/?]|$)/i.test(String(url||''));
}
function formatDuration(sec){
  if (sec == null || !Number.isFinite(sec)) return '';
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  return h ? `${h}:${String(m).padStart(2,'0')}:${String(s).padStart(2,'0')}`
           : `${m}:${String(s).padStart(2,'0')}`;
}

// --- Split pasted text into URLs (comma or newline separated) ---
function splitUrls(text){
  return text.split(/[\n,]+/).map(s=>s.trim()).filter(Boolean);
}

// ================= GET VIDEO (single OR multiple) =================
btnAnalyze.addEventListener('click', async () => {
  const urls = splitUrls(urlInput.value);
  if (!urls.length) return;
  btnAnalyze.disabled = true; btnAnalyze.textContent='Reading…'; clearError();
  qualityPanel.hidden=true; scanResults.hidden=true; multiResults.hidden=true; playlistPanel.hidden=true;

  try {
    if (urls.length === 1) {
      if (looksLikePlaylist(urls[0])) {
        currentVideo = null;
        const pl = await invoke('analyze_playlist', { url: urls[0] });
        renderPlaylist(pl);
      } else {
        const info = await invoke('analyze_url', { url: urls[0] });
        currentVideo = info;
        document.getElementById('video-title').textContent = info.title || 'Video';
        const thumb = document.getElementById('single-thumb');
        if (info.thumbnail_url){ thumb.src = info.thumbnail_url; thumb.style.display='block'; }
        else { thumb.removeAttribute('src'); thumb.style.display='none'; }
        const sel = document.getElementById('sel-quality');
        sel.innerHTML='';
        for (const q of (info.qualities||[])){
          const opt=document.createElement('option');
          opt.value = q.height===0 ? 'auto' : `${q.height}p`;
          opt.textContent = q.label + (q.fps?` @ ${q.fps}fps`:'');
          sel.appendChild(opt);
        }
        qualityPanel.hidden=false;
      }
    } else {
      pendingMulti=[];
      for (const u of urls.slice(0,25)){
        try { const info=await invoke('analyze_url',{url:u}); pendingMulti.push({url:u,title:info.title||u,thumbnail:info.thumbnail_url||null}); }
        catch(e){ /* skip invalid links */ }
      }
      renderMultiList();
    }
  } catch (e) { showError(errMsg(e) || 'Could not read that URL.'); }
  finally { btnAnalyze.disabled=false; btnAnalyze.textContent='Get Video'; }
});

function renderMultiList(){
  const list=document.getElementById('multi-list'); list.innerHTML='';
  document.getElementById('multi-count').textContent = pendingMulti.length;
  for (const v of pendingMulti){
    const li=document.createElement('li');
    const label=document.createElement('label');
    const cb=document.createElement('input'); cb.type='checkbox'; cb.checked=true; cb.dataset.url=v.url;
    if (v.thumbnail){ const img=document.createElement('img'); img.className='thumb-sm'; img.src=v.thumbnail; img.onerror=()=>img.remove(); label.appendChild(img); }
    const span=document.createElement('span'); span.className='ml-title'; span.textContent=v.title||v.url;
    label.appendChild(cb); label.appendChild(span); li.appendChild(label); list.appendChild(li);
  }
  multiResults.hidden = pendingMulti.length===0;
}

// ================= PLAYLIST RESULTS =================
function renderPlaylist(info){
  currentPlaylist = info;
  const entries = info.entries || [];
  document.getElementById('playlist-title').textContent = info.title || 'Playlist';
  const total = info.count || entries.length;
  let meta = `${entries.length} video${entries.length===1?'':'s'}`;
  if (total > entries.length) meta += ` (${total} in playlist)`;
  document.getElementById('playlist-meta').textContent = meta;

  const list = document.getElementById('playlist-list');
  list.innerHTML = '';
  for (const v of entries){
    const li = document.createElement('li');
    const label = document.createElement('label');
    const cb = document.createElement('input');
    cb.type = 'checkbox'; cb.checked = true; cb.dataset.url = v.url;
    const img = document.createElement('img');
    img.className = 'thumb-sm'; img.alt = ''; img.loading = 'lazy';
    img.src = youtubeThumb(v.url) || v.thumbnail_url || PLACEHOLDER_THUMB;
    img.onerror = () => { img.src = PLACEHOLDER_THUMB; };
    const title = document.createElement('span');
    title.className = 'pl-title'; title.textContent = v.title || v.url;
    const dur = document.createElement('span');
    dur.className = 'pl-duration'; dur.textContent = formatDuration(v.duration_sec);
    label.append(cb, img, title, dur);
    li.appendChild(label);
    list.appendChild(li);
  }

  // Default quality/format follow the user's settings.
  const q = document.getElementById('pl-quality');
  const defQ = settings?.default_quality || 'auto';
  q.value = [...q.options].some(o => o.value === defQ) ? defQ : 'auto';
  const defFmt = settings?.default_format || 'mp4';
  const radio = document.querySelector(`input[name="plfmt"][value="${defFmt}"]`);
  if (radio) radio.checked = true;

  playlistPanel.hidden = false;
  updatePlaylistSelected();
}

function updatePlaylistSelected(){
  const boxes = [...document.querySelectorAll('#playlist-list input[type="checkbox"]')];
  const n = boxes.filter(b => b.checked).length;
  document.getElementById('pl-selected-count').textContent = `${n} selected`;
  document.getElementById('btn-pl-download').disabled = n === 0;
}

function playlistLimit(){
  return Math.max(1, Math.min(10, settings?.max_concurrent || 3));
}

function pumpPlaylistQueue(){
  const limit = playlistLimit();
  while (playlistActive < limit && playlistQueue.length){
    const job = playlistQueue.shift();
    playlistActive++;
    queueOne(job.url, job.quality, job.format, job.title, job.thumbnail, job.playlistName)
      .then(id => { if (id) playlistRunning.add(id); })
      .catch(e => {
        playlistActive = Math.max(0, playlistActive - 1);
        console.error('Playlist download failed to start:', errMsg(e));
        pumpPlaylistQueue();
      });
  }
}

function enqueuePlaylist(entries, playlistName, quality, format){
  if (!entries.length) return;
  const jobs = entries.map(v => ({
    url: v.url,
    title: v.title,
    thumbnail: youtubeThumb(v.url) || v.thumbnail_url || null,
    quality,
    format,
    playlistName,
  }));
  playlistQueue.push(...jobs);
  document.getElementById('playlist-note').textContent =
    `Queued ${jobs.length} video${jobs.length===1?'':'s'} — downloading ${playlistLimit()} at a time.`;
  pumpPlaylistQueue();
}

document.getElementById('playlist-list').addEventListener('change', updatePlaylistSelected);
document.getElementById('btn-pl-select-all').addEventListener('click', () => {
  document.querySelectorAll('#playlist-list input[type="checkbox"]').forEach(cb => cb.checked = true);
  updatePlaylistSelected();
});
document.getElementById('btn-pl-none').addEventListener('click', () => {
  document.querySelectorAll('#playlist-list input[type="checkbox"]').forEach(cb => cb.checked = false);
  updatePlaylistSelected();
});
document.getElementById('btn-pl-download').addEventListener('click', () => {
  if (!currentPlaylist) return;
  const quality = document.getElementById('pl-quality').value || 'auto';
  const formatEl = document.querySelector('input[name="plfmt"]:checked');
  const format = formatEl ? formatEl.value : 'mp4';
  const chosen = [...document.querySelectorAll('#playlist-list input[type="checkbox"]:checked')]
    .map(cb => currentPlaylist.entries.find(v => v.url === cb.dataset.url))
    .filter(Boolean);
  if (!chosen.length) return;
  enqueuePlaylist(chosen, currentPlaylist.title, quality, format);
});
document.getElementById('btn-pl-download-all').addEventListener('click', () => {
  if (!currentPlaylist) return;
  const quality = document.getElementById('pl-quality').value || 'auto';
  const formatEl = document.querySelector('input[name="plfmt"]:checked');
  const format = formatEl ? formatEl.value : 'mp4';
  enqueuePlaylist(currentPlaylist.entries, currentPlaylist.title, quality, format);
});

// ================= SCAN PAGE =================
btnScan.addEventListener('click', async () => {
  const urls = splitUrls(urlInput.value);
  if (!urls.length) return;
  btnScan.disabled=true; btnScan.textContent='Scanning…'; clearError();
  qualityPanel.hidden=true; multiResults.hidden=true;

  try {
    const result = await invoke('scan_page', { url: urls[0] });
    const videos = result.videos || [];
    document.getElementById('scan-count').textContent = `${videos.length}`;
    const list = document.getElementById('scan-list');
    list.innerHTML='';
    for (const v of videos){
      const li=document.createElement('li');
      const label=document.createElement('label');
      const cb=document.createElement('input'); cb.type='checkbox'; cb.checked=true; cb.dataset.url=v.url;
      const span=document.createElement('span'); span.textContent = v.title || v.url.replace(/^https?:\/\//,'').slice(0,60);
      const badge=document.createElement('span'); badge.className='source-badge'; badge.textContent=v.source;
      label.appendChild(cb); label.appendChild(span); label.appendChild(badge); li.appendChild(label); list.appendChild(li);
    }
    scanResults.hidden=false;
  } catch (e) { showError(errMsg(e) || 'Could not scan that page.'); }
  finally { btnScan.disabled=false; btnScan.textContent='Scan Page'; }
});

document.getElementById('btn-select-all').addEventListener('click', () => {
  document.querySelectorAll('#scan-list input[type="checkbox"]').forEach(cb => cb.checked=true);
});
document.getElementById('btn-deselect-all').addEventListener('click', () => {
  document.querySelectorAll('#scan-list input[type="checkbox"]').forEach(cb => cb.checked=false);
});

// ================= QUEUE DOWNLOADS =================
async function queueOne(url, quality, format, title, thumbnail, playlistName){
  const thumb = thumbnail || youtubeThumb(url) || null;
  const payload = { url, quality, format, title: title||null, thumbnail_url: thumb };
  if (playlistName) payload.playlist_title = playlistName;
  const id = await invoke('start_download', payload);
  if (id) activeDl.set(id, { title: title||'Video', thumbnail: thumb, file_path:null, dir:null });
  return id;
}

document.getElementById('btn-queue').addEventListener('click', async () => {
  if (!currentVideo) return;
  const quality = document.getElementById('sel-quality').value || 'auto';
  const formatEl = document.querySelector('input[name="fmt"]:checked');
  const format = formatEl ? formatEl.value : 'mp4';
  try { await queueOne(currentVideo.url, quality, format, currentVideo.title, currentVideo.thumbnail_url); resetForm(); }
  catch (e) { showError(errMsg(e) || 'Failed to start download.'); }
});

document.getElementById('btn-queue-multi').addEventListener('click', async () => {
  const checked=[...document.querySelectorAll('#multi-list input[type="checkbox"]:checked')];
  if (!checked.length) return;
  for (const cb of checked){
    const v = pendingMulti.find(x=>x.url===cb.dataset.url);
    try { await queueOne(cb.dataset.url, 'auto', 'mp4', v?v.title:null, v?v.thumbnail:null); } catch(e){}
  }
  resetForm();
});

document.getElementById('btn-queue-scanned').addEventListener('click', async () => {
  const checked=[...document.querySelectorAll('#scan-list input[type="checkbox"]:checked')];
  if (!checked.length) return;
  for (const cb of checked){ try { await queueOne(cb.dataset.url, 'auto', 'mp4', null, null); } catch(e){} }
  resetForm();
});

function resetForm(){ urlInput.value=''; currentVideo=null; pendingMulti=[]; currentPlaylist=null; qualityPanel.hidden=true; scanResults.hidden=true; multiResults.hidden=true; playlistPanel.hidden=true; }

// ================= DOWNLOAD PROGRESS (thumbnail + name + bar + check) =================
listen('download-event', (event) => {
  const p = event.payload || {};
  const { id, status, progress_pct, speed_bps, eta_sec, error, file_path, dir } = p;
  if (!id) return;

  let meta = activeDl.get(id);
  if (!meta){ meta = { title:'Video', thumbnail:null, file_path:null, dir:null }; activeDl.set(id, meta); }
  if (file_path) meta.file_path = file_path;
  if (dir) meta.dir = dir;
  if (p.title) meta.title = p.title;
  if (p.thumbnail_url) meta.thumbnail = p.thumbnail_url;

  const section=document.getElementById('downloads-section'); section.hidden=false;
  const list=document.getElementById('downloads-list');
  let card=list.querySelector(`[data-dl-id="${id}"]`);
  if (!card){
    card=document.createElement('div'); card.className='download-item'; card.dataset.dlId=id;
    const row=document.createElement('div'); row.className='dl-row';
    const img=document.createElement('img'); img.className='thumb'; img.alt='';
    if (meta.thumbnail){ img.src=meta.thumbnail; } else { img.style.display='none'; }
    img.onerror=()=>{ img.style.display='none'; };
    const body=document.createElement('div'); body.className='dl-body';
    const titleEl=document.createElement('p'); titleEl.className='title'; titleEl.textContent = meta.title || 'Video';
    const progressRow=document.createElement('div'); progressRow.className='dl-progress-row';
    const barOuter=document.createElement('div'); barOuter.className='progress-bar';
    const fill=document.createElement('div'); fill.className='progress-fill'; fill.style.width='0%';
    barOuter.appendChild(fill);
    const pctEl=document.createElement('span'); pctEl.className='progress-pct'; pctEl.textContent='0%';
    progressRow.appendChild(barOuter); progressRow.appendChild(pctEl);
    const statusEl=document.createElement('span'); statusEl.className='dl-status'; statusEl.textContent='Starting…';
    body.appendChild(titleEl); body.appendChild(progressRow); body.appendChild(statusEl);
    row.appendChild(img); row.appendChild(body); card.appendChild(row);
    list.prepend(card);
  }

  const titleEl=card.querySelector('.title');
  if (titleEl && meta.title) titleEl.textContent = meta.title;

  const imgEl=card.querySelector('img.thumb');
  if (imgEl && meta.thumbnail){ imgEl.src = meta.thumbnail; imgEl.style.display='block'; }

  const fill=card.querySelector('.progress-fill');
  const pctEl=card.querySelector('.progress-pct');
  const statusEl=card.querySelector('.dl-status');
  const existingActions=card.querySelector('.actions'); if (existingActions) existingActions.remove();

  if (status==='downloading'){
    if (progress_pct!==undefined && progress_pct!==null){
      const pct=Math.max(0, Math.min(100, progress_pct));
      fill.style.width=`${pct}%`;
      fill.classList.remove('complete');
      pctEl.textContent=`${pct.toFixed(1)}%`;
    }
    let txt='Downloading…';
    const bits=[];
    if (speed_bps) bits.push(formatSpeed(speed_bps));
    if (eta_sec) bits.push(formatEta(eta_sec));
    if (bits.length) txt += '  ·  ' + bits.join('  ·  ');
    statusEl.textContent = p.note || txt;
    statusEl.className='dl-status';
  } else if (status==='complete'){
    fill.style.width='100%'; fill.classList.add('complete'); pctEl.textContent='100%';
    statusEl.innerHTML='<span class="check">✓</span> Done'; statusEl.className='dl-status done';
    addActions(card, meta);
  } else if (status==='failed'){
    fill.style.background='var(--danger)';
    statusEl.textContent = 'Failed: ' + (error||''); statusEl.className='dl-status failed';
  }

  // Keep the playlist queue moving: one finished/failed job starts the next.
  if ((status==='complete' || status==='failed') && playlistRunning.has(id)) {
    playlistRunning.delete(id);
    playlistActive = Math.max(0, playlistActive - 1);
    pumpPlaylistQueue();
  }
});

function addActions(card, meta){
  const actions=document.createElement('div'); actions.className='actions';
  const target = meta.file_path || meta.dir;
  if (target){
    const openFolder=document.createElement('button'); openFolder.className='btn btn-secondary'; openFolder.textContent='Open Folder';
    openFolder.onclick=()=>invoke('reveal_in_folder',{path:target}).catch(()=>{});
    actions.appendChild(openFolder);
  }
  if (meta.file_path){
    const play=document.createElement('button'); play.className='btn btn-primary'; play.textContent='Play';
    play.onclick=()=>invoke('play_file',{path:meta.file_path}).catch(e=>alert(errMsg(e)));
    actions.appendChild(play);
  }
  card.appendChild(actions);
}

function formatSpeed(bps){ if (bps>=1e6) return `${(bps/1e6).toFixed(1)} MB/s`; if (bps>=1e3) return `${(bps/1e3).toFixed(0)} KB/s`; return `${bps} B/s`; }
function formatEta(sec){ if (sec<60) return `${sec}s left`; return `${Math.floor(sec/60)}m ${sec%60}s left`; }

// ================= LIBRARY (thumbnail + Open Folder + Play + delete) =================
async function renderLibrary(){
  const listEl=document.getElementById('library-list');
  const emptyEl=document.getElementById('lib-empty');
  let items=[];
  try { items = await invoke('get_downloads'); } catch(e){ return; }
  const q=(document.getElementById('lib-search').value||'').toLowerCase();
  const completed=(items||[]).filter(i=>i.status==='complete' && (!q || (i.title||'').toLowerCase().includes(q)));

  listEl.innerHTML='';
  emptyEl.style.display = completed.length ? 'none' : 'block';

  for (const it of completed){
    const li=document.createElement('li'); li.className='lib-item'; li.dataset.id=it.id;
    const row=document.createElement('div'); row.className='dl-row';
    const img=document.createElement('img'); img.className='thumb'; img.alt='';
    const thumb=thumbFor(it);
    img.src = thumb || PLACEHOLDER_THUMB;
    img.style.display='block';
    img.onerror=()=>{ img.src=PLACEHOLDER_THUMB; img.onerror=null; };
    const body=document.createElement('div'); body.className='dl-body';
    const titleEl=document.createElement('p'); titleEl.className='title'; titleEl.textContent=it.title||'Video';
    if (it.file_path){ const sub=document.createElement('span'); sub.className='muted small'; sub.textContent=it.file_path; body.appendChild(sub); }
    body.prepend(titleEl);
    row.appendChild(img); row.appendChild(body); li.appendChild(row);

    const actions=document.createElement('div'); actions.className='lib-actions';
    if (it.file_path || it.dir){
      const of=document.createElement('button'); of.className='btn btn-secondary sm'; of.textContent='Open Folder';
      of.onclick=()=>invoke('reveal_in_folder',{path: it.file_path||it.dir}).catch(()=>{});
      actions.appendChild(of);
    }
    if (it.file_path){
      const pl=document.createElement('button'); pl.className='btn btn-primary sm'; pl.textContent='Play';
      pl.onclick=()=>invoke('play_file',{path:it.file_path}).catch(e=>alert(errMsg(e)));
      actions.appendChild(pl);
    }

    // Three-dots menu with Delete
    const dots=document.createElement('button'); dots.className='dots-btn'; dots.textContent='⋯'; dots.setAttribute('aria-label','More');
    const menu=document.createElement('div'); menu.className='ctx-menu'; menu.hidden=true;
    const del=document.createElement('button'); del.className='ctx-item danger'; del.textContent='Delete media…';
    del.onclick=async()=>{
      if (confirm('Delete this file and remove it from your library?')){
        try { await invoke('delete_download',{id:it.id, deleteFile:true}); renderLibrary(); } catch(e){ alert(errMsg(e)); }
      }
    };
    menu.appendChild(del); li.appendChild(menu);
    dots.onclick=(e)=>{ e.stopPropagation(); document.querySelectorAll('.ctx-menu').forEach(m=>{ if(m!==menu) m.hidden=true; }); menu.hidden=!menu.hidden; };
    actions.appendChild(dots);

    li.appendChild(actions);
    listEl.appendChild(li);
  }
}
document.getElementById('lib-search').addEventListener('input', renderLibrary);
document.addEventListener('click', ()=>document.querySelectorAll('.ctx-menu').forEach(m=>m.hidden=true));

// ================= SAVE LOCATION (Task 1) =================
function effectivePath(){
  if (!settings || !settings.download_path) return '…';
  const base=settings.download_path.replace(/\\/g,'/');
  return settings.create_subfolder ? `${base}/ClipHive` : base;
}
function refreshPathDisplays(){
  const p=effectivePath();
  document.getElementById('dl-path-display').textContent=p;
  document.getElementById('set-path-display').textContent=p;
}

async function browseAndSetPath(){
  try {
    const chosen = await pickFolder({ directory:true, multiple:false });
    if (!chosen) return;
    if (!settings) settings = (await invoke('get_settings')) || {};
    settings.download_path = chosen;
    document.getElementById('set-subfolder').checked = !!settings.create_subfolder;
    refreshPathDisplays();
  } catch(e){ /* cancelled */ }
}

document.getElementById('btn-browse-path').addEventListener('click', async () => {
  await browseAndSetPath();
  await saveSettingsNow();
});
document.getElementById('btn-change-dl-path').addEventListener('click', async () => {
  await browseAndSetPath();
  await saveSettingsNow();
});

// ================= SETTINGS (auto-save, no Save button) =================
const CONNECTIONS_MIN = 1, CONNECTIONS_MAX = 64;

async function loadSettings(){
  try { settings = await invoke('get_settings'); } catch(e){ settings=null; }
  if (!settings) return;
  document.getElementById('set-quality').value = settings.default_quality || 'auto';
  document.getElementById('set-format').value = settings.default_format || 'mp4';
  document.getElementById('set-concurrent').value = settings.max_concurrent || 3;
  document.getElementById('set-connections').value = settings.connections || 8;
  document.getElementById('set-speed').value = settings.speed_limit_mbps ?? 0;
  document.getElementById('set-proxy').value = settings.proxy_url || '';
  document.getElementById('set-subfolder').checked = !!settings.create_subfolder;
  clampConnections();
  refreshPathDisplays();
}

function clampConnections(){
  const el=document.getElementById('set-connections');
  let n=parseInt(el.value,10);
  if (!Number.isFinite(n)) n=8;
  n=Math.max(CONNECTIONS_MIN, Math.min(CONNECTIONS_MAX, n));
  el.value=n;
  return n;
}

function collectSettings(){
  if (!settings) settings={};
  settings.default_quality=document.getElementById('set-quality').value;
  settings.default_format=document.getElementById('set-format').value;
  settings.max_concurrent=Math.max(1, Math.min(10, parseInt(document.getElementById('set-concurrent').value)||3));
  settings.connections=clampConnections();
  settings.speed_limit_mbps=Math.max(0, parseInt(document.getElementById('set-speed').value)||0);
  const proxy=document.getElementById('set-proxy').value.trim();
  settings.proxy_url = proxy || null;
  settings.create_subfolder=document.getElementById('set-subfolder').checked;
  settings.theme='dark';
  return settings;
}

function showToast(msg, isError){
  const el=document.getElementById('settings-saved');
  if (!el) return;
  el.textContent = isError ? msg : 'Saved ✓';
  el.classList.toggle('error', !!isError);
  el.hidden=false;
  clearTimeout(showToast._t);
  showToast._t=setTimeout(()=>{ el.hidden=true; el.classList.remove('error'); }, isError ? 4000 : 2200);
}
function flashSaved(){ showToast('Saved ✓'); }

let saveTimer=null;
function scheduleSave(){
  clearTimeout(saveTimer);
  saveTimer=setTimeout(saveSettingsNow, 350);
}

async function saveSettingsNow(){
  clearTimeout(saveTimer);
  const payload=collectSettings();
  refreshPathDisplays();
  try {
    await invoke('save_settings', { settings: payload });
    flashSaved();
  } catch (e) {
    showToast('Could not save: ' + errMsg(e), true);
  }
}

['set-quality','set-format','set-concurrent','set-connections','set-speed','set-proxy'].forEach(id=>{
  const el=document.getElementById(id);
  el.addEventListener('change', scheduleSave);
  el.addEventListener('input', scheduleSave);
});
document.getElementById('set-connections').addEventListener('blur', ()=>{ clampConnections(); scheduleSave(); });
document.getElementById('set-subfolder').addEventListener('change', scheduleSave);

// ================= CHECK FOR UPDATES =================
const appVersionEl=document.getElementById('app-version');
invoke('get_version').then(v=>{ if(appVersionEl) appVersionEl.textContent=v; }).catch(()=>{});

function setUpdateStatus(msg,isError){ const el=document.getElementById('update-status'); if(!el)return; el.textContent=msg; el.hidden=false; el.classList.toggle('error',!!isError); }

function friendlyUpdateError(e){
  const m=errMsg(e);
  if (/404|not[\s-]*found|manifest|release json/i.test(m))
    return "the update server has no manifest for this release (update-manifest.json is missing).";
  if (/network|fetch|connect|dns|timeout|timed out|econn|socket|offline|status code/i.test(m))
    return "couldn't reach the update server. Check your internet connection and try again.";
  if (/signature|verify|pubkey|minisign/i.test(m))
    return "the update failed its signature check, so it was not installed.";
  return m;
}

let updateCheckRunning=false;
async function runUpdateCheck(auto=false){
  if (updateCheckRunning) return;
  updateCheckRunning=true;
  const btn=document.getElementById('btn-check-update');
  if (btn) btn.disabled=true;
  try {
    setUpdateStatus('Checking for updates…');
    const update=await check({ timeout: 20000 });
    if (!update){
      setUpdateStatus("You're up to date ✓");
      return;
    }
    let total=0, downloaded=0;
    setUpdateStatus(`Found v${update.version} — downloading…`);
    await update.downloadAndInstall((ev)=>{
      if (ev.event==='Started') { total=ev.data.contentLength||0; }
      else if (ev.event==='Progress') {
        downloaded += ev.data.chunkLength||0;
        if (total) setUpdateStatus(`Downloading v${update.version}… ${Math.min(100, Math.round(downloaded/total*100))}%`);
      } else if (ev.event==='Finished') {
        setUpdateStatus('Download complete — installing…');
      }
    }, { timeout: 600000 });
    // On Windows the installer takes over and closes the app; on other platforms relaunch.
    setUpdateStatus('Installed. ClipHive is restarting…');
    setTimeout(async()=>{ try { await relaunch(); } catch(_){} }, 800);
  } catch (e) {
    setUpdateStatus('Update failed: ' + friendlyUpdateError(e), true);
  } finally {
    updateCheckRunning=false;
    if (btn) btn.disabled=false;
  }
}

document.getElementById('btn-check-update').addEventListener('click', ()=>runUpdateCheck(false));

// Enter key submits URL
urlInput.addEventListener('keydown', e => { if (e.key==='Enter' && !e.shiftKey){ e.preventDefault(); btnAnalyze.click(); }});

// ================= INIT =================
(async () => {
  await loadSettings();
  renderLibrary();
  // Check for updates automatically on every launch (slightly delayed so the
  // window appears first). Manual checks keep the button.
  setTimeout(()=>{ runUpdateCheck(true); }, 2000);
})();
