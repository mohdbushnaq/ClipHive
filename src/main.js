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

let currentVideo = null;      // single analyzed video {url,title,thumbnail_url,...}
let pendingMulti = [];        // [{url,title,thumbnail}] for >1 URL
const activeDl = new Map();   // id -> {title, thumbnail, file_path, dir}
let settings = null;          // working copy of AppSettings

function showError(msg){ errorMsg.textContent = msg; errorMsg.hidden = false; }
function clearError(){ errorMsg.hidden = true; errorMsg.textContent=''; }
function errMsg(e){ return typeof e==='string' ? e : ((e && e.message) || String(e)); }

// --- Split pasted text into URLs (comma or newline separated) ---
function splitUrls(text){
  return text.split(/[\n,]+/).map(s=>s.trim()).filter(Boolean);
}

// ================= GET VIDEO (single OR multiple) =================
btnAnalyze.addEventListener('click', async () => {
  const urls = splitUrls(urlInput.value);
  if (!urls.length) return;
  btnAnalyze.disabled = true; btnAnalyze.textContent='Reading…'; clearError();
  qualityPanel.hidden=true; scanResults.hidden=true; multiResults.hidden=true;

  try {
    if (urls.length === 1) {
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
async function queueOne(url, quality, format, title, thumbnail){
  const id = await invoke('start_download', { url, quality, format, title: title||null, thumbnail_url: thumbnail||null });
  if (id) activeDl.set(id, { title: title||'Video', thumbnail: thumbnail||null, file_path:null, dir:null });
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

function resetForm(){ urlInput.value=''; currentVideo=null; pendingMulti=[]; qualityPanel.hidden=true; scanResults.hidden=true; multiResults.hidden=true; }

// ================= DOWNLOAD PROGRESS (thumbnail + name + bar + check) =================
listen('download-event', (event) => {
  const p = event.payload || {};
  const { id, status, progress_pct, speed_bps, eta_sec, error, file_path, dir } = p;
  if (!id) return;

  let meta = activeDl.get(id);
  if (!meta){ meta = { title:'Video', thumbnail:null, file_path:null, dir:null }; activeDl.set(id, meta); }
  if (file_path) meta.file_path = file_path;
  if (dir) meta.dir = dir;

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
    const barOuter=document.createElement('div'); barOuter.className='progress-bar';
    const fill=document.createElement('div'); fill.className='progress-fill'; fill.style.width='0%';
    barOuter.appendChild(fill);
    const statusEl=document.createElement('span'); statusEl.className='dl-status'; statusEl.textContent='Starting…';
    body.appendChild(titleEl); body.appendChild(barOuter); body.appendChild(statusEl);
    row.appendChild(img); row.appendChild(body); card.appendChild(row);
    list.prepend(card);
  }

  const fill=card.querySelector('.progress-fill');
  const statusEl=card.querySelector('.dl-status');
  const existingActions=card.querySelector('.actions'); if (existingActions) existingActions.remove();

  if (status==='downloading'){
    if (progress_pct!==undefined && progress_pct!==null) fill.style.width=`${Math.min(100,progress_pct)}%`;
    let txt='Downloading…';
    const bits=[];
    if (speed_bps) bits.push(formatSpeed(speed_bps));
    if (eta_sec) bits.push(formatEta(eta_sec));
    if (bits.length) txt += '  ·  ' + bits.join('  ·  ');
    statusEl.textContent=txt; statusEl.className='dl-status';
  } else if (status==='complete'){
    fill.style.width='100%'; fill.classList.add('complete');
    statusEl.innerHTML='<span class="check">✓</span> Done'; statusEl.className='dl-status done';
    addActions(card, meta);
  } else if (status==='failed'){
    fill.style.background='var(--danger)';
    statusEl.textContent = 'Failed: ' + (error||''); statusEl.className='dl-status failed';
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
    if (it.thumbnail_url){ img.src=it.thumbnail_url; } else { img.style.display='none'; }
    img.onerror=()=>{ img.style.display='none'; };
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

document.getElementById('btn-browse-path').addEventListener('click', browseAndSetPath);
document.getElementById('btn-change-dl-path').addEventListener('click', async () => {
  await browseAndSetPath();
  if (settings) { try { await invoke('save_settings',{settings}); flashSaved(); } catch(e){} }
});

// ================= SETTINGS (load / save + feedback) =================
async function loadSettings(){
  try { settings = await invoke('get_settings'); } catch(e){ settings=null; }
  if (!settings) return;
  document.getElementById('set-quality').value = settings.default_quality || 'auto';
  document.getElementById('set-format').value = settings.default_format || 'mp4';
  document.getElementById('set-concurrent').value = settings.max_concurrent || 3;
  document.getElementById('set-speed').value = settings.speed_limit_mbps ?? 0;
  document.getElementById('set-proxy').value = settings.proxy_url || '';
  document.getElementById('set-subfolder').checked = !!settings.create_subfolder;
  refreshPathDisplays();
}

function flashSaved(){
  const el=document.getElementById('settings-saved');
  el.hidden=false; clearTimeout(flashSaved._t); flashSaved._t=setTimeout(()=>el.hidden=true, 2500);
}

document.getElementById('btn-save-settings').addEventListener('click', async () => {
  if (!settings) settings = {};
  settings.default_quality=document.getElementById('set-quality').value;
  settings.default_format=document.getElementById('set-format').value;
  settings.max_concurrent=parseInt(document.getElementById('set-concurrent').value)||3;
  settings.speed_limit_mbps=parseInt(document.getElementById('set-speed').value)||0;
  settings.proxy_url=document.getElementById('set-proxy').value||null;
  settings.create_subfolder=document.getElementById('set-subfolder').checked;
  settings.theme='dark';
  settings.filename_pattern='{title}_{quality}';
  try { await invoke('save_settings',{settings}); refreshPathDisplays(); flashSaved(); }
  catch (e) { alert('Failed to save: ' + errMsg(e)); }
});

document.getElementById('set-subfolder').addEventListener('change', ()=>{ if(settings){ settings.create_subfolder=document.getElementById('set-subfolder').checked; refreshPathDisplays(); }});

// ================= CHECK FOR UPDATES =================
const appVersionEl=document.getElementById('app-version');
invoke('get_version').then(v=>{ if(appVersionEl) appVersionEl.textContent=v; }).catch(()=>{});

function setUpdateStatus(msg,isError){ const el=document.getElementById('update-status'); if(!el)return; el.textContent=msg; el.hidden=false; el.classList.toggle('error',!!isError); }
document.getElementById('btn-check-update').addEventListener('click', async () => {
  const btn=document.getElementById('btn-check-update');
  try {
    btn.disabled=true; setUpdateStatus('Checking for updates…');
    const update=await check();
    if (!update){ setUpdateStatus("You're up to date ✓"); return; }
    setUpdateStatus(`Found v${update.version} — downloading & installing…`);
    await update.downloadAndInstall(()=>{});
    setUpdateStatus('Installed! Relaunching ClipHive…');
    setTimeout(async()=>{ try { await relaunch(); } catch(_){} }, 800);
  } catch (e) { setUpdateStatus('Update check failed: ' + errMsg(e), true); }
  finally { btn.disabled=false; }
});

// Enter key submits URL
urlInput.addEventListener('keydown', e => { if (e.key==='Enter' && !e.shiftKey){ e.preventDefault(); btnAnalyze.click(); }});

// ================= INIT =================
loadSettings();
renderLibrary();
