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
  $('start').value = 0; $('trim_start').value = 0; $('trim_end').value = 0; $('trim_start').max = media.duration.toFixed(2); $('trim_end').max = media.duration.toFixed(2); $('job-box').hidden = true; updateLabels();
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
['compression_enabled','compression_mode','shutter','samples','sharpness','protection','fps','fps-custom','interp_mode','restore_quality','codec','crf','start','trim_start','trim_end','analysis_width','search_radius','weighting','encoder_preset','gamma','gaussian_sigma','brightness','contrast','saturation','preserve_audio','preview_duration','processing','video_encoder','deduplicate','vector_smoothing','dehalo','hud_protect','subframe_blend'].forEach(id => { if ($(id)) $(id).addEventListener('input', () => {updateLabels(); markStale(); if(['shutter','samples','sharpness'].includes(id)) document.querySelectorAll('[data-preset]').forEach(b => b.classList.remove('active')); }); });
if ($('set-trim-in')) $('set-trim-in').onclick = () => { $('trim_start').value = (master().currentTime || 0).toFixed(2); updateLabels(); };
if ($('set-trim-out')) $('set-trim-out').onclick = () => { $('trim_end').value = (master().currentTime || 0).toFixed(2); updateLabels(); };
if ($('trim-reset')) $('trim-reset').onclick = () => { $('trim_start').value = 0; $('trim_end').value = 0; updateLabels(); };
$('codec').onchange = () => {if($('compression_enabled').checked&&$('codec').value==='prores')$('codec').value='h264';$('codec').querySelector('[value="prores"]').disabled=$('compression_enabled').checked;$('video_encoder').querySelector('[value="cpu"]').textContent=$('compression_enabled').checked?'CPU · HEVC':'CPU · H.264'; $('crf').disabled = $('codec').value === 'prores'||$('compression_enabled').checked;$('video_encoder').disabled=$('codec').value==='prores'&&!$('compression_enabled').checked;$('codec').querySelector('[value="h264"]').textContent=$('compression_enabled').checked?'MP4 · HEVC comprimido':'MP4 · H.264';};
$('compression_enabled').addEventListener('input',()=>{$('codec').onchange(); });
function settings(preview) {const fpsVal = $('fps').value === 'custom' ? String(+$('fps-custom').value || 60) : $('fps').value; return {compression_enabled:$('compression_enabled').checked,compression_mode:$('compression_mode').value,media_id:media.id,shutter:+$('shutter').value,samples:+$('samples').value,protection:$('protection').checked,sharpness:+$('sharpness').value/100,fps:fpsVal,interp_mode:$('interp_mode').value,restore_quality:$('restore_quality').value,codec:$('codec').value,crf:+$('crf').value,analysis_width:+$('analysis_width').value,search_radius:+$('search_radius').value,weighting:$('weighting').value,encoder_preset:$('encoder_preset').value,gamma:+$('gamma').value,gaussian_sigma:+$('gaussian_sigma').value,brightness:+$('brightness').value,contrast:+$('contrast').value,saturation:+$('saturation').value,preserve_audio:$('preserve_audio').checked,preview_duration:+$('preview_duration').value,processing:$('processing').value,video_encoder:$('video_encoder').value,deduplicate:$('deduplicate').checked,vector_smoothing:$('vector_smoothing').checked,dehalo:$('dehalo').checked,hud_protect:$('hud_protect').checked,subframe_blend:$('subframe_blend').checked,start:preview ? +$('start').value : 0,trim_start:preview ? 0 : (+$('trim_start').value || 0),trim_end:+$('trim_end').value || 0,trim_only:false,preview};}
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
function setView(next) {
  original.pause(); result.pause(); $('play').innerHTML=icon('play'); view = next;
  document.querySelectorAll('[data-view]').forEach(b=>b.classList.toggle('active',b.dataset.view===next));
  $('result-layer').setAttribute('aria-hidden',String(next==='original'));$('result-layer').style.display='block';$('result-layer').style.opacity=next==='original'?'0':'1';
  $('result-layer').style.clipPath=next === 'original'?'inset(0 100% 0 0)':next === 'split' ? 'inset(0 0 0 50%)' : 'inset(0 0 0 0)'; $('split-line').hidden=next !== 'split';
  $('video-label').textContent=next === 'split' ? 'ORIGINAL / LUMA' : next === 'result' ? 'LUMA MOTION' : 'ORIGINAL';
  $('preview-tag').hidden=next === 'original' || !lastResult?.preview;
  original.muted=next !== 'original'; result.muted=next === 'original';
  if (next !== 'original' && lastResult) {result.currentTime=0; original.currentTime=lastResult.start;}
  updateTransport();window.updateMotionIndicators?.();
}
document.querySelectorAll('[data-view]').forEach(button=>button.onclick=()=>setView(button.dataset.view));
function updateTransport() {const video=master(); $('time').textContent=formatTime(video.currentTime); $('duration').textContent=formatTime(video.duration); $('seek').value=video.duration ? video.currentTime/video.duration*100 : 0;}
[original,result].forEach(video=>{video.addEventListener('loadedmetadata',updateTransport); video.addEventListener('timeupdate',()=>{if(video===master()) updateTransport(); if(video===result && view==='split' && lastResult && Math.abs(original.currentTime-result.currentTime-lastResult.start)>.08) original.currentTime=result.currentTime+lastResult.start;});video.addEventListener('ended',()=>{original.pause(); result.pause(); $('play').innerHTML=icon('play');});video.addEventListener('error',()=>{if(video.getAttribute('src')) toast('O navegador não reproduz este formato. Use um MP4 H.264 para comparar; ProRes pode ser salvo e aberto no seu editor.');});});
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
  if(!data.ready)toast('Instale FFmpeg e ffprobe e reinicie o aplicativo.');
}).catch(()=>{$('health').textContent='Motor desconectado';});
