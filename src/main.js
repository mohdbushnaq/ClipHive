// ClipHive — Frontend logic (Tauri 2 IPC)
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// --- Tab navigation ---
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    tab.classList.add('active');
    const section = document.getElementById(`tab-${tab.dataset.tab}`);
    if (section) section.classList.add('active');
  });
});

// --- Download flow ---
const urlInput = document.getElementById('url-input');
const btnAnalyze = document.getElementById('btn-analyze');
const btnScan = document.getElementById('btn-scan');
const errorMsg = document.getElementById('error-msg');
const qualityPanel = document.getElementById('quality-panel');
const scanResults = document.getElementById('scan-results');

let currentVideo = null;

function showError(msg) {
  errorMsg.textContent = msg;
  errorMsg.hidden = false;
}

function clearError() {
  errorMsg.hidden = true;
  errorMsg.textContent = '';
}

btnAnalyze.addEventListener('click', async () => {
  const url = urlInput.value.trim();
  if (!url) return;
  
  btnAnalyze.disabled = true;
  btnAnalyze.textContent = 'Reading…';
  clearError();
  qualityPanel.hidden = true;
  scanResults.hidden = true;

  try {
    const info = await invoke('analyze_url', { url });
    currentVideo = info;
    
    document.getElementById('video-title').textContent = info.title || 'Video';
    const sel = document.getElementById('sel-quality');
    sel.innerHTML = '';
    for (const q of (info.qualities || [])) {
      const opt = document.createElement('option');
      opt.value = q.height === 0 ? 'auto' : `${q.height}p`;
      opt.textContent = q.label + (q.fps ? ` @ ${q.fps}fps` : '');
      sel.appendChild(opt);
    }
    
    qualityPanel.hidden = false;
  } catch (e) {
    showError(typeof e === 'string' ? e : (e.message || 'Could not read that URL.'));
  } finally {
    btnAnalyze.disabled = false;
    btnAnalyze.textContent = 'Get Video';
  }
});

btnScan.addEventListener('click', async () => {
  const url = urlInput.value.trim();
  if (!url) return;
  
  btnScan.disabled = true;
  btnScan.textContent = 'Scanning…';
  clearError();
  qualityPanel.hidden = true;

  try {
    const result = await invoke('scan_page', { url });
    const videos = result.videos || [];
    
    document.getElementById('scan-count').textContent = `${videos.length}`;
    const list = document.getElementById('scan-list');
    list.innerHTML = '';
    
    for (const v of videos) {
      const li = document.createElement('li');
      const label = document.createElement('label');
      
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.checked = true;
      cb.dataset.url = v.url;
      
      const span = document.createElement('span');
      span.textContent = v.title || v.url.replace(/^https?:\/\//, '').slice(0, 60);
      
      const badge = document.createElement('span');
      badge.className = 'source-badge';
      badge.textContent = v.source;
      
      label.appendChild(cb);
      label.appendChild(span);
      label.appendChild(badge);
      li.appendChild(label);
      list.appendChild(li);
    }
    
    scanResults.hidden = false;
  } catch (e) {
    showError(typeof e === 'string' ? e : (e.message || 'Could not scan that page.'));
  } finally {
    btnScan.disabled = false;
    btnScan.textContent = 'Scan Page';
  }
});

document.getElementById('btn-select-all').addEventListener('click', () => {
  document.querySelectorAll('#scan-list input[type="checkbox"]').forEach(cb => cb.checked = true);
});
document.getElementById('btn-deselect-all').addEventListener('click', () => {
  document.querySelectorAll('#scan-list input[type="checkbox"]').forEach(cb => cb.checked = false);
});

// --- Queue download (single video) ---
document.getElementById('btn-queue').addEventListener('click', async () => {
  if (!currentVideo) return;
  const quality = document.getElementById('sel-quality').value || 'auto';
  const formatEl = document.querySelector('input[name="fmt"]:checked');
  const format = formatEl ? formatEl.value : 'mp4';
  
  try {
    await invoke('start_download', { 
      url: currentVideo.url, 
      quality, 
      format 
    });
    resetForm();
  } catch (e) {
    showError(typeof e === 'string' ? e : (e.message || 'Failed to start download.'));
  }
});

// --- Queue downloads (scanned page) ---
document.getElementById('btn-queue-scanned').addEventListener('click', async () => {
  const checked = [...document.querySelectorAll('#scan-list input[type="checkbox"]:checked')];
  if (checked.length === 0) return;
  
  for (const cb of checked) {
    try {
      await invoke('start_download', { url: cb.dataset.url, quality: 'auto', format: 'mp4' });
    } catch (e) { /* continue with next */ }
  }
  resetForm();
});

function resetForm() {
  urlInput.value = '';
  currentVideo = null;
  qualityPanel.hidden = true;
  scanResults.hidden = true;
}

// --- Download progress events from Rust backend ---
listen('download-event', (event) => {
  const payload = event.payload || {};
  const { id, status, progress_pct, speed_bps, eta_sec, error } = payload;
  
  if (!id) return;
  
  let item = document.querySelector(`[data-dl-id="${id}"]`);
  
  if (!item) {
    // Create new download item in the list
    const section = document.getElementById('downloads-section');
    section.hidden = false;
    const list = document.getElementById('downloads-list');
    
    item = document.createElement('div');
    item.className = 'download-item';
    item.dataset.dlId = id;
    
    const titleEl = document.createElement('p');
    titleEl.className = 'title';
    titleEl.textContent = status === 'complete' ? 'Done ✓' : (status === 'failed' ? `Failed: ${error || ''}` : 'Downloading…');
    
    const metaEl = document.createElement('div');
    metaEl.className = 'meta';
    const speedSpan = document.createElement('span');
    speedSpan.className = 'speed';
    if (speed_bps) speedSpan.textContent = formatSpeed(speed_bps);
    const etaSpan = document.createElement('span');
    etaSpan.className = 'eta';
    if (eta_sec) etaSpan.textContent = formatEta(eta_sec);
    metaEl.appendChild(speedSpan);
    metaEl.appendChild(etaSpan);
    
    const barOuter = document.createElement('div');
    barOuter.className = 'progress-bar';
    const barFill = document.createElement('div');
    barFill.className = 'progress-fill' + (status === 'complete' ? ' complete' : '');
    barFill.style.width = `${progress_pct || 0}%`;
    if (status === 'failed') barFill.style.background = 'var(--danger)';
    barOuter.appendChild(barFill);
    
    const actions = document.createElement('div');
    actions.className = 'actions';
    if (status === 'complete') {
      const btn = document.createElement('button');
      btn.className = 'btn btn-primary';
      btn.style.cssText = 'height:32px;font-size:12px;padding:0 12px;';
      btn.textContent = 'Open Folder';
      btn.onclick = () => invoke('reveal_in_folder', { path: '' }).catch(() => alert('Check your Downloads/ClipHive folder'));
      actions.appendChild(btn);
    } else if (status !== 'failed') {
      const btn = document.createElement('button');
      btn.className = 'btn btn-secondary';
      btn.style.cssText = 'height:32px;font-size:12px;padding:0 12px;';
      btn.textContent = 'Cancel';
      btn.onclick = () => invoke('cancel_download', { id }).catch(() => {});
      actions.appendChild(btn);
    }
    
    item.appendChild(titleEl);
    item.appendChild(metaEl);
    item.appendChild(barOuter);
    if (actions.children.length) item.appendChild(actions);
    list.prepend(item);
  } else {
    // Update existing item
    const fill = item.querySelector('.progress-fill');
    if (fill && progress_pct !== undefined) {
      fill.style.width = `${progress_pct}%`;
      if (status === 'complete') fill.classList.add('complete');
      if (status === 'failed') fill.style.background = 'var(--danger)';
    }
    
    const titleEl = item.querySelector('.title');
    if (titleEl && status === 'complete') titleEl.textContent = 'Done ✓';
    if (titleEl && status === 'failed') titleEl.textContent = `Failed: ${error || ''}`;
    
    const speedEl = item.querySelector('.speed');
    if (speedEl && speed_bps) speedEl.textContent = formatSpeed(speed_bps);
    const etaEl = item.querySelector('.eta');
    if (etaEl && eta_sec) etaEl.textContent = formatEta(eta_sec);
  }
});

function formatSpeed(bps) {
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(1)} MB/s`;
  if (bps >= 1e3) return `${(bps / 1e3).toFixed(0)} KB/s`;
  return `${bps} B/s`;
}

function formatEta(sec) {
  if (sec < 60) return `${sec}s left`;
  return `${Math.floor(sec/60)}m ${sec%60}s left`;
}

// --- Batch mode ---
document.getElementById('btn-batch-validate').addEventListener('click', async () => {
  const lines = document.getElementById('batch-input').value.split('\n').map(l => l.trim()).filter(Boolean);
  if (lines.length === 0) return;
  
  const resultsDiv = document.getElementById('batch-results');
  resultsDiv.innerHTML = '<p class="subtitle">Checking your links…</p>';
  
  const results = [];
  for (const url of lines.slice(0, 20)) {
    try {
      const info = await invoke('analyze_url', { url });
      results.push({ url, valid: true, title: info.title || url });
    } catch (e) {
      results.push({ url, valid: false, error: (typeof e === 'string' ? e : e.message)?.slice(0, 80) || 'Failed' });
    }
  }

  const validCount = results.filter(r => r.valid).length;
  
  let html = `<p style="margin-bottom:12px"><strong>${validCount}</strong> valid · ${results.length - validCount} failed</p><ul role="list">`;
  for (const r of results) {
    html += `<li class="batch-item" style="padding:6px 0;border-bottom:1px solid var(--border)">`;
    html += `${r.valid ? '✓' : '✗'} ${r.title || r.url.slice(0, 50)}`;
    if (!r.valid) html += ` <small style="color:var(--danger)">${r.error}</small>`;
    html += `</li>`;
  }
  html += '</ul>';
  resultsDiv.innerHTML = html;
  
  const btnQueue = document.getElementById('btn-batch-queue');
  btnQueue.hidden = validCount === 0;
  btnQueue.textContent = `Queue ${validCount} Video${validCount !== 1 ? 's' : ''}`;
  btnQueue.onclick = async () => {
    for (const r of results.filter(r => r.valid)) {
      try { await invoke('start_download', { url: r.url, quality: 'auto', format: 'mp4' }); } catch(e) {}
    }
    resetForm();
  };
});

// --- Settings ---
document.getElementById('btn-save-settings').addEventListener('click', async () => {
  const settings = {
    default_quality: document.getElementById('set-quality').value,
    default_format: document.getElementById('set-format').value,
    max_concurrent: parseInt(document.getElementById('set-concurrent').value) || 3,
    speed_limit_mbps: parseInt(document.getElementById('set-speed').value) || 0,
    proxy_url: document.getElementById('set-proxy').value || null,
    theme: 'dark',
    filename_pattern: '{title}_{quality}',
  };
  
  try {
    await invoke('save_settings', { settings });
    const saved = document.getElementById('settings-saved');
    saved.hidden = false;
    setTimeout(() => saved.hidden = true, 3000);
  } catch (e) {
    alert('Failed to save: ' + (typeof e === 'string' ? e : e.message));
  }
});

// Load settings on start
invoke('get_settings').then(s => {
  if (!s) return;
  if (s.default_quality) document.getElementById('set-quality').value = s.default_quality;
  if (s.default_format) document.getElementById('set-format').value = s.default_format;
  if (s.max_concurrent) document.getElementById('set-concurrent').value = s.max_concurrent;
  if (s.speed_limit_mbps !== undefined && s.speed_limit_mbps !== null) {
    document.getElementById('set-speed').value = s.speed_limit_mbps;
  }
  if (s.proxy_url) document.getElementById('set-proxy').value = s.proxy_url;
}).catch(() => {});

// Enter key submits URL
urlInput.addEventListener('keydown', e => { if (e.key === 'Enter') btnAnalyze.click(); });
