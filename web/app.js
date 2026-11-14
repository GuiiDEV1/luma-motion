const $ = id => document.getElementById(id);
let media = null, job = null, view = 'original', lastResult = null, locked = false;
const original = $('original'), result = $('result');
const formatTime = value => { const n = Number.isFinite(value) ? value : 0; return `${String(Math.floor(n/60)).padStart(2,'0')}:${(n%60).toFixed(2).padStart(5,'0')}`; };
function toast(message) { $('toast').textContent = message; $('toast').hidden = false; clearTimeout(toast.timer); toast.timer = setTimeout(() => $('toast').hidden = true, 6000); }
async function api(url, options) { const response = await fetch(url, options); const data = await response.json(); if (!response.ok) throw new Error(typeof data.detail === 'string' ? data.detail : 'Confira os ajustes e tente novamente.'); return data; }
function lock(value) { locked = value; ['preview','export','compress','add-queue','trim-only-btn'].forEach(id => { if ($(id)) $(id).disabled = value || !media; }); ['import-top','import-empty','file'].forEach(id => $(id).disabled = value); if ($('run-queue')) $('run-queue').disabled = value || !queue.length; }
let zoomLevel = 1;
if ($('zoom-btn')) {
  $('zoom-btn').onclick = () => {
    zoomLevel = zoomLevel === 1 ? 2 : zoomLevel === 2 ? 4 : 1;
    $('zoom-btn').textContent = zoomLevel + '×';
    $('zoom-btn').classList.toggle('active', zoomLevel > 1);
    const scale = `scale(${zoomLevel})`;
    original.style.transform = scale;
    result.style.transform = scale;
  };
  $('dropzone').addEventListener('pointermove', e => {
    if (zoomLevel === 1) return;
    const r = $('dropzone').getBoundingClientRect();
    const ox = ((e.clientX - r.left) / r.width * 100).toFixed(1) + '%';
    const oy = ((e.clientY - r.top) / r.height * 100).toFixed(1) + '%';
    original.style.transformOrigin = `${ox} ${oy}`;
    result.style.transformOrigin = `${ox} ${oy}`;
  });
}
async function pickDesktopFile() {
  if (locked) return;
  lock(true); window.lumaFeedback?.('import','running');
  try {
    const response = await fetch('/api/media/pick', {method:'POST'});
    if (response.status === 204) { window.lumaFeedback?.('import','idle'); return; }
    const nextMedia = await response.json();
    if (!response.ok) throw new Error(typeof nextMedia.detail === 'string' ? nextMedia.detail : 'Não foi possível abrir o vídeo.');
    applyMedia(nextMedia);
  } catch (error) { window.lumaFeedback?.('import','error'); toast(error.message); } finally { lock(false); }
}
['import-top','import-empty'].forEach(id => $(id).onclick = () => window.LUMA_DESKTOP ? pickDesktopFile() : $('file').click());
$('file').onchange = () => { if ($('file').files[0]) loadFile($('file').files[0]); };
const zone = $('dropzone');
zone.ondragover = event => { event.preventDefault(); zone.classList.add('drag'); };
zone.ondragleave = () => zone.classList.remove('drag');
zone.ondrop = event => { event.preventDefault(); zone.classList.remove('drag'); if (!locked && event.dataTransfer.files[0]) loadFile(event.dataTransfer.files[0]); };
function applyMedia(nextMedia) {
  original.pause(); result.pause(); media = nextMedia; lastResult = null;
  original.src = media.url; result.removeAttribute('src'); result.load();
  $('empty').hidden = true; $('player').hidden = false;
  $('filename').textContent = media.name; $('file-detail').textContent = `${media.width} × ${media.height} · ${media.fps.toFixed(2)} fps · ${formatTime(media.duration)} · ${media.codec.toUpperCase()}`;
  $('source-tag').textContent = `${Math.round(media.fps)} FPS`; $('start').max = Math.max(0, media.duration - .1);
  $('start').value = 0; $('trim_start').value = 0; $('trim_end').value = 0; $('trim_start').max = media.duration.toFixed(2); $('trim_end').max = media.duration.toFixed(2); $('job-box').hidden = true;
  if ($('timeline-editor')) { $('timeline-editor').hidden = false; tlZoom = 1; if ($('tl-track')) $('tl-track').style.width = '100%'; if ($('tl-filmstrip')) $('tl-filmstrip').style.backgroundImage = `url('/api/media/${media.id}/filmstrip')`; resetTimeline(); }
  updateLabels();
  document.querySelectorAll('[data-view]').forEach(button => button.disabled = button.dataset.view !== 'original');
  ['play','seek','mute','zoom-btn'].forEach(id => { if ($(id)) $(id).disabled = false; }); setView('original');
  window.lumaFeedback?.('import','done'); toast('Vídeo pronto. Escolha um preset e teste uma prévia.');
}
async function loadFile(file) {
  lock(true);window.lumaFeedback?.('import','running'); toast('Importando vídeo para o estúdio local…');
  try {
    const body = new FormData(); body.append('file', file);
    const nextMedia = await api('/api/media', {method:'POST',body});
    applyMedia(nextMedia);
  } catch(error) {window.lumaFeedback?.('import','error'); toast(error.message); } finally { lock(false); $('file').value = ''; }
}
const presets = {clean:{shutter:120,samples:9,sharpness:0},smooth:{shutter:180,samples:15,sharpness:10},cinema:{shutter:270,samples:15,sharpness:0}};
document.querySelectorAll('[data-preset]').forEach(button => button.onclick = () => {
  const preset = presets[button.dataset.preset]; Object.entries(preset).forEach(([key,value]) => $(key).value = value);
  $('protection').checked = true; document.querySelectorAll('[data-preset]').forEach(b => b.classList.toggle('active', b === button)); updateLabels(); markStale();
});
function updateWeightCurve() {
  const path = $('weight-path');
  if (!path) return;
  const mode = $('weighting').value, sigma = Math.max(0.15, +$('gaussian_sigma').value || 0.65), n = 36;
  const pts = [];
  for (let i = 0; i < n; i++) {
    const t = (i / (n - 1)) * 2 - 1;
    let w = 1;
    if (mode === 'uniform') w = 1;
    else if (mode === 'pyramid') w = Math.max(0.02, 1 - Math.abs(t));
    else if (mode === 'ascending') w = (t + 1) / 2 + 0.02;
    else if (mode === 'descending') w = (1 - t) / 2 + 0.02;
    else if (mode === 'vegas') w = Math.max(0.02, Math.pow(0.5 * (1 + Math.cos(t * Math.PI)), 1.2));
    else if (mode === 'gaussian_bound') w = Math.max(0.02, Math.exp(-0.5 * (t / sigma) ** 2) - Math.exp(-0.5 / (sigma ** 2)));
    else w = Math.exp(-0.5 * (t / sigma) ** 2);
    pts.push(w);
  }
  const maxW = Math.max(...pts, 0.01);
  const d = pts.map((w, i) => `${i === 0 ? 'M' : 'L'}${(6 + (i / (n - 1)) * 168).toFixed(1)},${(40 - (w / maxW) * 32).toFixed(1)}`).join(' ');
  path.setAttribute('d', d);
}
function updateLabels() {
  $('compression-hint').textContent=$('compression_mode').value==='aggressive'?'HEVC com compressão forte: prioriza tamanho, com perda visível em detalhes e movimentos. Mantém resolução e FPS.':'HEVC com alta fidelidade: prioriza preservar detalhes com perda visual mínima. Não é sem perdas; o tamanho depende do vídeo.';
  $('engine-badge').textContent='FLOW / '+$('processing').value.toUpperCase();
  window.updateSummary?.();
  $('preview').innerHTML=icon('play')+' Testar '+$('preview_duration').value+' segundos';
  $('preview-tag').textContent='PRÉVIA · ATÉ '+$('preview_duration').value+' SEGUNDOS';
  $('shutter-value').textContent = `${$('shutter').value}°`;
  $('sharpness-value').textContent = `${$('sharpness').value}%`;
  if ($('samples-value')) $('samples-value').textContent = `${$('samples').value} amostras`;
  if ($('analysis_width-value')) $('analysis_width-value').textContent = `${$('analysis_width').value} px`;
  if ($('search_radius-value')) $('search_radius-value').textContent = `${$('search_radius').value} px`;
  if ($('fps-custom')) $('fps-custom').hidden = $('fps').value !== 'custom';
  if ($('trim-hint')) {
    const tIn = +$('trim_start').value || 0, tOut = +$('trim_end').value || 0;
    $('trim-hint').textContent = (tIn > 0 || tOut > 0) ? `Corte ativo: ${formatTime(tIn)} → ${tOut > 0 ? formatTime(tOut) : 'fim do vídeo'}` : '0 = vídeo inteiro. Marque In/Out no player para cortar antes de exportar.';
  }
  updateWeightCurve();
}
function markStale() { if (lastResult) $('job-detail').textContent = 'Ajustes alterados. Gere uma nova prévia para ver as mudanças.'; }
['compression_enabled','compression_mode','shutter','samples','sharpness','protection','fps','fps-custom','interp_mode','restore_quality','aspect_ratio','target_mb','fade_in','fade_out','audio_gain','audio_normalize','codec','crf','start','trim_start','trim_end','analysis_width','search_radius','weighting','encoder_preset','gamma','gaussian_sigma','brightness','contrast','saturation','preserve_audio','preview_duration','processing','video_encoder','deduplicate','vector_smoothing','dehalo','hud_protect','subframe_blend'].forEach(id => { if ($(id)) $(id).addEventListener('input', () => {updateLabels(); markStale(); if(['shutter','samples','sharpness'].includes(id)) document.querySelectorAll('[data-preset]').forEach(b => b.classList.remove('active')); }); });
if ($('set-trim-in')) $('set-trim-in').onclick = () => { if (!media) return; const seg = tlSegments[selectedSegIdx]; if (seg) { seg.start = Math.min(master().currentTime || 0, seg.end - 0.1); renderTimeline(); } };
if ($('set-trim-out')) $('set-trim-out').onclick = () => { if (!media) return; const seg = tlSegments[selectedSegIdx]; if (seg) { seg.end = Math.max(master().currentTime || 0, seg.start + 0.1); renderTimeline(); } };
if ($('trim-reset')) $('trim-reset').onclick = () => { resetTimeline(); };
['trim_start','trim_end'].forEach(id => { if ($(id)) $(id).addEventListener('change', () => { const seg = tlSegments[selectedSegIdx]; if (!seg || !media) return; const vIn = Math.max(0, +$('trim_start').value || 0); const vOut = +$('trim_end').value > 0 ? Math.min(media.duration, +$('trim_end').value) : media.duration; if (vOut - vIn >= 0.1) { seg.start = vIn; seg.end = vOut; renderTimeline(); } }); });
$('codec').onchange = () => {if($('compression_enabled').checked&&$('codec').value==='prores')$('codec').value='h264';$('codec').querySelector('[value="prores"]').disabled=$('compression_enabled').checked;$('video_encoder').querySelector('[value="cpu"]').textContent=$('compression_enabled').checked?'CPU · HEVC':'CPU · H.264'; $('crf').disabled = $('codec').value === 'prores'||$('compression_enabled').checked;$('video_encoder').disabled=$('codec').value==='prores'&&!$('compression_enabled').checked;$('codec').querySelector('[value="h264"]').textContent=$('compression_enabled').checked?'MP4 · HEVC comprimido':'MP4 · H.264';};
$('compression_enabled').addEventListener('input',()=>{$('codec').onchange(); });
function activeSegmentsPayload() {
  if (!media || !tlSegments.length) return [];
  const active = tlSegments.filter(s => s.active).map(s => ({ start: Number(s.start.toFixed(3)), end: Number(s.end.toFixed(3)), speed: Number((s.speed || 1).toFixed(2)) }));
  if (active.length === 1 && active[0].start <= 0.02 && active[0].end >= media.duration - 0.02 && Math.abs(active[0].speed - 1) < 0.01) return [];
  return active;
}
function settings(preview) {const fpsVal = $('fps').value === 'custom' ? String(+$('fps-custom').value || 60) : $('fps').value; return {compression_enabled:$('compression_enabled').checked,compression_mode:$('compression_mode').value,media_id:media.id,shutter:+$('shutter').value,samples:+$('samples').value,protection:$('protection').checked,sharpness:+$('sharpness').value/100,fps:fpsVal,interp_mode:$('interp_mode').value,restore_quality:$('restore_quality').value,aspect_ratio:$('aspect_ratio')?$('aspect_ratio').value:'source',target_mb:$('target_mb')?(+$('target_mb').value||0):0,fade_in:$('fade_in')?(+$('fade_in').value||0):0,fade_out:$('fade_out')?(+$('fade_out').value||0):0,audio_gain:$('audio_gain')?(+$('audio_gain').value||1):1,audio_normalize:$('audio_normalize')?$('audio_normalize').checked:false,codec:$('codec').value,crf:+$('crf').value,analysis_width:+$('analysis_width').value,search_radius:+$('search_radius').value,weighting:$('weighting').value,encoder_preset:$('encoder_preset').value,gamma:+$('gamma').value,gaussian_sigma:+$('gaussian_sigma').value,brightness:+$('brightness').value,contrast:+$('contrast').value,saturation:+$('saturation').value,preserve_audio:$('preserve_audio').checked,preview_duration:+$('preview_duration').value,processing:$('processing').value,video_encoder:$('video_encoder').value,deduplicate:$('deduplicate').checked,vector_smoothing:$('vector_smoothing').checked,dehalo:$('dehalo').checked,hud_protect:$('hud_protect').checked,subframe_blend:$('subframe_blend').checked,start:preview ? +$('start').value : 0,trim_start:preview ? 0 : (+$('trim_start').value || 0),trim_end:+$('trim_end').value || 0,trim_only:false,segments:preview ? [] : activeSegmentsPayload(),preview};}
const queue = [];
function renderQueue() {
  const panel = $('queue-panel'), list = $('queue-list');
  panel.hidden = queue.length === 0;
  $('queue-count').textContent = queue.length;
  $('run-queue').disabled = locked || !queue.some(item => item.status === 'pending');
  list.innerHTML = '';
  queue.forEach((item, idx) => {
    const row = document.createElement('div');
    row.className = 'queue-item';
    row.dataset.status = item.status;
    const title = document.createElement('span');
    title.textContent = `${item.name} · ${item.request.shutter}° · ${item.request.fps === 'source' ? 'orig' : item.request.fps + 'fps'}`;
    row.append(title);
    if (item.status === 'done' && item.jobId) {
      const dl = document.createElement('a');
      dl.href = `/api/jobs/${item.jobId}/video?download=true`;
      dl.textContent = 'Salvar';
      dl.className = 'queue-save';
      row.append(dl);
    } else if (item.status === 'pending') {
      const rm = document.createElement('button');
      rm.textContent = '×';
      rm.className = 'queue-remove';
      rm.onclick = () => { queue.splice(idx, 1); renderQueue(); };
      row.append(rm);
    } else {
      const st = document.createElement('small');
      st.textContent = item.status === 'running' ? `${Math.round(item.progress || 0)}%` : item.status === 'error' ? 'Erro' : 'Cancelado';
      row.append(st);
    }
    list.append(row);
  });
}
$('add-queue').onclick = () => {
  if (!media || locked) return;
  queue.push({ name: media.name, request: { ...settings(false), compress_only: false }, status: 'pending', progress: 0, jobId: null });
  renderQueue();
  toast(`“${media.name}” adicionado à fila (${queue.length}).`);
};
$('run-queue').onclick = async () => {
  if (locked) return;
  for (const item of queue) {
    if (item.status !== 'pending') continue;
    item.status = 'running'; renderQueue();
    lock(true); $('job-box').hidden = false; window.lumaFeedback?.('render','running'); $('download').hidden = true; $('cancel').hidden = false;
    $('job-title').textContent = `Fila · ${item.name}`; $('progress').value = 0;
    try {
      job = await api('/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(item.request)});
      while (job) {
        await new Promise(resolve => setTimeout(resolve, 150));
        const state = await api(`/api/jobs/${job.id}`); $('progress').value = state.progress; item.progress = state.progress;
        $('job-title').textContent = `Fila (${item.name}) · ${state.progress.toFixed(0)}%`;
        if (state.status === 'error') throw new Error(state.error);
        if (state.status === 'cancelled') { item.status = 'cancelled'; window.lumaFeedback?.('render','cancelled'); break; }
        if (state.status === 'done') { item.status = 'done'; item.jobId = job.id; window.lumaFeedback?.('render','done'); break; }
      }
    } catch (err) { item.status = 'error'; toast(err.message); window.lumaFeedback?.('render','error'); }
    finally { job = null; $('cancel').hidden = true; lock(false); renderQueue(); }
    if (item.status === 'cancelled') break;
  }
  toast('Processamento da fila finalizado.');
};
$('preview').onclick = () => submit(true); $('export').onclick = () => submit(false); $('compress').onclick=()=>submit(false,true); if ($('trim-only-btn')) $('trim-only-btn').onclick = () => submit(false, false, true);
async function submit(preview,compressOnly=false,trimOnly=false) {
  if (!media || locked) return;
  lock(true); original.pause(); result.pause(); $('play').innerHTML=icon('play');
  const request = {...settings(preview),compress_only:compressOnly,trim_only:trimOnly}; $('job-box').hidden = false;window.lumaFeedback?.('render','running'); $('download').hidden = true; $('cancel').hidden = false;
  $('job-title').textContent = preview ? 'Gerando prévia…' : trimOnly ? 'Cortando vídeo…' : compressOnly ? 'Comprimindo arquivo…' : 'Exportando vídeo…'; $('progress').value = 0;
  $('job-detail').textContent = trimOnly ? 'Recortando o trecho selecionado.' : compressOnly ? 'Codificando diretamente, sem calcular blur.' : request.restore_quality === 'ai_2x' ? 'Aplicando Real-ESRGAN 2× e renderizando.' : 'Estimando o movimento e integrando as amostras.';
  try {
    job = await api('/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(request)});
    while (job) {
      await new Promise(resolve => setTimeout(resolve, 150));
      const state = await api(`/api/jobs/${job.id}`); $('progress').value = state.progress;
      if (state.status === 'error') throw new Error(state.error);
      if (state.status === 'cancelled') {window.lumaFeedback?.('render','cancelled'); $('job-title').textContent = 'Cancelado'; $('job-detail').textContent='Você pode ajustar e tentar novamente.'; break; }
      if (state.status === 'done') {window.lumaFeedback?.('render','done');
        lastResult = {id:job.id,preview,start:request.start,duration:state.meta.duration};
        if(state.meta.codec!=='hevc') result.src = `/api/jobs/${job.id}/video`; $('download').href=`/api/jobs/${job.id}/video?download=true`; $('download').hidden=false;
        $('job-title').textContent=preview ? 'Prévia pronta' : 'Exportação concluída'; $('job-detail').textContent=preview ? 'Compare o efeito. A prévia usa resolução reduzida; a exportação usa a original.' : `${(state.meta.bytes/1048576).toFixed(2)} MB · ${state.meta.source_bytes ? (state.meta.bytes<=state.meta.source_bytes?(100*(1-state.meta.bytes/state.meta.source_bytes)).toFixed(1)+'% menor · ':(100*(state.meta.bytes/state.meta.source_bytes-1)).toFixed(1)+'% maior · ') : ''}${state.meta.render_seconds.toFixed(2)} s · ${state.meta.width} × ${state.meta.height} · ${state.meta.fps.toFixed(2)} fps · ${state.meta.processor || 'CPU'} · ${state.meta.encoder || 'H.264'} · Áudio ${state.meta.audio ? 'incluído' : 'não incluído'}`;
        document.querySelectorAll('[data-view]').forEach(button=>button.disabled=state.meta.codec==='hevc'&&button.dataset.view!=='original'); setView(state.meta.codec==='hevc'?'original':'split');
        toast(preview ? 'Prévia pronta para comparar.' : state.meta.codec==='hevc'?'Arquivo HEVC pronto. Salve para abrir em um player compatível.':'Vídeo pronto. Clique em Salvar vídeo.'); break;
      }
      $('job-title').textContent=`${preview ? 'Gerando prévia' : 'Exportando'} · ${state.progress.toFixed(0)}%`;
    }
  } catch(error) {window.lumaFeedback?.('render','error'); $('job-title').textContent='Não foi possível concluir'; $('job-detail').textContent=error.message; toast(error.message); }
  finally {$('dropzone').dataset.rendering='false';job=null; $('cancel').hidden=true; lock(false);}
}
$('cancel').onclick = async () => { if(job) {try { await api(`/api/jobs/${job.id}/cancel`,{method:'POST'}); $('job-detail').textContent='Cancelando ao finalizar o quadro atual…'; } catch(error) {toast(error.message);} } };
function master() {return view === 'original' ? original : result;}
let splitPos = 50;
function applySplit() {
  if (view !== 'split') return;
  $('result-layer').style.clipPath = `inset(0 0 0 ${splitPos}%)`;
  $('split-line').style.left = `${splitPos}%`;
}
if ($('split-line')) {
  $('split-line').onpointerdown = e => {
    e.stopPropagation();
    const box = $('player').getBoundingClientRect();
    const move = ev => {
      splitPos = Math.max(2, Math.min(98, ((ev.clientX - box.left) / box.width) * 100));
      applySplit();
    };
    const up = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };
}
function setView(next) {
  original.pause(); result.pause(); $('play').innerHTML=icon('play'); view = next;
  document.querySelectorAll('[data-view]').forEach(b=>b.classList.toggle('active',b.dataset.view===next));
  $('result-layer').setAttribute('aria-hidden',String(next==='original'));$('result-layer').style.display='block';$('result-layer').style.opacity=next==='original'?'0':'1';
  $('result-layer').style.clipPath=next === 'original'?'inset(0 100% 0 0)':next === 'split' ? `inset(0 0 0 ${splitPos}%)` : 'inset(0 0 0 0)'; $('split-line').hidden=next !== 'split';
  if (next === 'split') $('split-line').style.left = `${splitPos}%`;
  $('video-label').textContent=next === 'split' ? 'ORIGINAL / LUMA' : next === 'result' ? 'LUMA MOTION' : 'ORIGINAL';
  $('preview-tag').hidden=next === 'original' || !lastResult?.preview;
  original.muted=next !== 'original'; result.muted=next === 'original';
  if (next !== 'original' && lastResult) {result.currentTime=0; original.currentTime=lastResult.start;}
  updateTransport();window.updateMotionIndicators?.();
}
document.querySelectorAll('[data-view]').forEach(button=>button.onclick=()=>setView(button.dataset.view));
let tlSegments = [], selectedSegIdx = 0, tlZoom = 1;
function resetTimeline() {
  if (!media) return;
  tlSegments = [{ start: 0, end: media.duration, active: true, speed: 1 }];
  selectedSegIdx = 0;
  renderTimeline();
}
function updatePlayhead() {
  if (!media || !$('tl-playhead')) return;
  const pct = Math.max(0, Math.min(100, (original.currentTime / Math.max(0.01, media.duration)) * 100));
  $('tl-playhead').style.left = pct + '%';
}
function renderTimeline() {
  if (!media || !$('tl-clips')) return;
  const clipsEl = $('tl-clips');
  clipsEl.innerHTML = '';
  const dur = Math.max(0.01, media.duration);
  tlSegments.forEach((seg, idx) => {
    const el = document.createElement('div');
    el.className = 'tl-clip' + (idx === selectedSegIdx ? ' selected' : '') + (seg.active ? '' : ' cut');
    el.style.left = (seg.start / dur * 100) + '%';
    el.style.width = Math.max(1, ((seg.end - seg.start) / dur * 100)) + '%';
    const lbl = document.createElement('span');
    const spd = seg.speed && Math.abs(seg.speed - 1) > 0.01 ? ` · ${seg.speed}×` : '';
    lbl.textContent = seg.active ? `${((seg.end - seg.start) / (seg.speed || 1)).toFixed(1)}s${spd}` : 'Cortado';
    const hl = document.createElement('div'); hl.className = 'tl-handle l'; hl.title = 'Arrastar início';
    const hr = document.createElement('div'); hr.className = 'tl-handle r'; hr.title = 'Arrastar fim';
    hl.onpointerdown = e => startTrimDrag(e, idx, 'l');
    hr.onpointerdown = e => startTrimDrag(e, idx, 'r');
    el.onclick = e => {
      if (e.target.classList.contains('tl-handle')) return;
      selectedSegIdx = idx;
      renderTimeline();
    };
    el.ondblclick = () => { selectedSegIdx = idx; toggleSelectedSegment(); };
    el.append(hl, lbl, hr);
    clipsEl.append(el);
  });
  const active = tlSegments.filter(s => s.active);
  const totalActive = active.reduce((acc, s) => acc + ((s.end - s.start) / (s.speed || 1)), 0);
  if ($('tl-summary')) $('tl-summary').textContent = `${formatTime(totalActive)} · ${active.length} ${active.length === 1 ? 'trecho ativo' : 'trechos ativos'}`;
  const sel = tlSegments[selectedSegIdx];
  if ($('tl-toggle') && sel) {
    $('tl-toggle').textContent = sel.active ? 'Excluir trecho' : 'Restaurar trecho';
    $('tl-toggle').classList.toggle('danger', sel.active);
  }
  if ($('tl-speed') && sel) {
    $('tl-speed').value = String(sel.speed || 1);
    $('tl-speed').disabled = !sel.active;
  }
  if (sel && $('trim_start') && $('trim_end')) {
    $('trim_start').value = sel.start.toFixed(2);
    $('trim_end').value = sel.end.toFixed(2);
    updateLabels();
  }
  updatePlayhead();
}
function startTrimDrag(e, idx, side) {
  e.stopPropagation();
  const track = $('tl-track');
  if (!track || !media) return;
  selectedSegIdx = idx;
  const seg = tlSegments[idx];
  const prevEnd = idx > 0 ? tlSegments[idx - 1].end : 0;
  const nextStart = idx < tlSegments.length - 1 ? tlSegments[idx + 1].start : media.duration;
  const rect = track.getBoundingClientRect();
  const move = ev => {
    const t = Math.max(0, Math.min(media.duration, ((ev.clientX - rect.left) / rect.width) * media.duration));
    if (side === 'l') {
      seg.start = Math.max(prevEnd, Math.min(t, seg.end - 0.1));
      original.currentTime = seg.start;
    } else {
      seg.end = Math.min(nextStart, Math.max(t, seg.start + 0.1));
      original.currentTime = seg.end;
    }
    renderTimeline();
  };
  const up = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
}
function splitAtPlayhead() {
  if (!media) return;
  const t = original.currentTime;
  const idx = tlSegments.findIndex(s => t > s.start + 0.1 && t < s.end - 0.1);
  if (idx === -1) { toast('Posicione a agulha dentro de um trecho para dividir.'); return; }
  const orig = tlSegments[idx];
  tlSegments.splice(idx, 1, { start: orig.start, end: t, active: orig.active, speed: orig.speed || 1 }, { start: t, end: orig.end, active: orig.active, speed: orig.speed || 1 });
  selectedSegIdx = idx + 1;
  renderTimeline();
}
function stepFrame(dir) {
  if (!media) return;
  original.pause(); result.pause(); $('play').innerHTML = icon('play');
  const step = 1 / Math.max(1, media.fps || 30);
  const v = master();
  v.currentTime = Math.max(0, Math.min(v.duration || media.duration, v.currentTime + dir * step));
  if (view === 'split' && lastResult) original.currentTime = result.currentTime + lastResult.start;
  updateTransport();
}
function setTlZoom(next) {
  tlZoom = Math.max(1, Math.min(8, next));
  if ($('tl-track')) $('tl-track').style.width = `${tlZoom * 100}%`;
}
function toggleSelectedSegment() {
  const seg = tlSegments[selectedSegIdx];
  if (!seg) return;
  if (seg.active && tlSegments.filter(s => s.active).length <= 1) {
    toast('Mantenha pelo menos um trecho ativo.');
    return;
  }
  seg.active = !seg.active;
  renderTimeline();
}
if ($('tl-split')) $('tl-split').onclick = splitAtPlayhead;
if ($('tl-toggle')) $('tl-toggle').onclick = toggleSelectedSegment;
if ($('tl-reset')) $('tl-reset').onclick = resetTimeline;
if ($('tl-frame-prev')) $('tl-frame-prev').onclick = () => stepFrame(-1);
if ($('tl-frame-next')) $('tl-frame-next').onclick = () => stepFrame(1);
if ($('tl-zoom-in')) $('tl-zoom-in').onclick = () => setTlZoom(tlZoom * 1.5);
if ($('tl-zoom-out')) $('tl-zoom-out').onclick = () => setTlZoom(tlZoom / 1.5);
if ($('tl-speed')) $('tl-speed').onchange = () => { const seg = tlSegments[selectedSegIdx]; if (seg) { seg.speed = +$('tl-speed').value || 1; renderTimeline(); } };
if ($('tl-rate')) $('tl-rate').onchange = () => { const r = +$('tl-rate').value || 1; original.playbackRate = r; result.playbackRate = r; };
if ($('tl-scroll')) $('tl-scroll').addEventListener('wheel', e => { if (e.ctrlKey) { e.preventDefault(); setTlZoom(e.deltaY < 0 ? tlZoom * 1.25 : tlZoom / 1.25); } }, { passive: false });
if ($('tl-track')) {
  $('tl-track').onpointerdown = e => {
    if (e.target.classList.contains('tl-handle') || !media) return;
    const rect = $('tl-track').getBoundingClientRect();
    const scrub = ev => {
      const t = Math.max(0, Math.min(media.duration, ((ev.clientX - rect.left) / rect.width) * media.duration));
      original.currentTime = t;
      if (view !== 'original' && lastResult) result.currentTime = Math.max(0, t - lastResult.start);
      updatePlayhead();
    };
    scrub(e);
    const up = () => { window.removeEventListener('pointermove', scrub); window.removeEventListener('pointerup', up); };
    window.addEventListener('pointermove', scrub);
    window.addEventListener('pointerup', up);
  };
}
document.addEventListener('keydown', e => {
  if (!media || ['INPUT','SELECT','TEXTAREA'].includes(document.activeElement?.tagName)) return;
  if (e.key === 'c' || e.key === 'C' || e.key === 's' || e.key === 'S') { e.preventDefault(); splitAtPlayhead(); }
  if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); toggleSelectedSegment(); }
  if (e.key === 'ArrowLeft') { e.preventDefault(); stepFrame(-1); }
  if (e.key === 'ArrowRight') { e.preventDefault(); stepFrame(1); }
});
function updateTransport() {const video=master(); $('time').textContent=formatTime(video.currentTime); $('duration').textContent=formatTime(video.duration); $('seek').value=video.duration ? video.currentTime/video.duration*100 : 0; updatePlayhead();}
[original,result].forEach(video=>{video.addEventListener('loadedmetadata',updateTransport); video.addEventListener('timeupdate',()=>{if(video===master()) updateTransport(); if(video===original && view==='original' && !original.paused && tlSegments.some(s=>!s.active)) { const t = original.currentTime; const inActive = tlSegments.some(s => s.active && t >= s.start - 0.03 && t < s.end - 0.03); if (!inActive) { const next = tlSegments.find(s => s.active && s.start > t); if (next) original.currentTime = next.start; else { original.pause(); $('play').innerHTML=icon('play'); } } } if(video===result && view==='split' && lastResult && Math.abs(original.currentTime-result.currentTime-lastResult.start)>.08) original.currentTime=result.currentTime+lastResult.start;});video.addEventListener('ended',()=>{original.pause(); result.pause(); $('play').innerHTML=icon('play');});video.addEventListener('error',()=>{if(video.getAttribute('src')) toast('O navegador não reproduz este formato. Use um MP4 H.264 para comparar; ProRes pode ser salvo e aberto no seu editor.');});});
$('play').onclick=async()=>{const video=master(); if(video.paused){try{if(video.ended) video.currentTime=0; await video.play(); if(view==='split'){original.currentTime=result.currentTime+lastResult.start; await original.play();} $('play').innerHTML=icon('pause');}catch{toast('Não foi possível reproduzir este formato no navegador.');}}else{original.pause();result.pause();$('play').innerHTML=icon('play');}};
$('seek').oninput=()=>{const video=master();if(Number.isFinite(video.duration)){video.currentTime=+$('seek').value/100*video.duration;if(view==='split')original.currentTime=video.currentTime+lastResult.start;}};
$('mute').onclick=()=>{const video=master();video.muted=!video.muted;$('mute').style.opacity=video.muted?.4:1;};
$('help').onclick=()=>$('help-dialog').showModal(); $('close-help').onclick=()=>$('help-dialog').close();
api('/api/health').then(data=>{
  $('health').textContent=data.ready?'Rust · motor disponível':'FFmpeg não encontrado';
  const hw=[]; if(data.nvenc?.ready) hw.push('NVENC'); if(data.amf?.ready) hw.push('AMD AMF'); if(data.qsv?.ready) hw.push('Intel QSV');
  $('gpu-status').textContent=(data.gpu?.ready?'GPU: '+data.gpu.name:'GPU indisponível (usando CPU)')+' · Encoders: '+(hw.length?hw.join(', '):'CPU apenas')+(data.realesrgan?.ready?' · Real-ESRGAN IA OK':'');
  $('processing').querySelector('[value="gpu"]').disabled=!data.gpu?.ready;
  $('video_encoder').querySelector('[value="nvenc"]').disabled=!data.nvenc?.ready;
  $('video_encoder').querySelector('[value="amf"]').disabled=!data.amf?.ready;
  $('video_encoder').querySelector('[value="qsv"]').disabled=!data.qsv?.ready;
  const aiOpt = $('restore_quality')?.querySelector('[value="ai_2x"]');
  if (aiOpt && !data.realesrgan?.ready) aiOpt.textContent = 'IA · Upscale 2× (Requer realesrgan-ncnn-vulkan em bin/)';
  const rifeOpt = $('interp_mode')?.querySelector('[value="ai_rife"]');
  if (rifeOpt && !data.rife?.ready) rifeOpt.textContent = 'IA · RIFE (Requer rife-ncnn-vulkan em bin/)';
  if(!data.ready)toast('Instale FFmpeg e ffprobe e reinicie o aplicativo.');
}).catch(()=>{$('health').textContent='Motor desconectado';});
