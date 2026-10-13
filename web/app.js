const $ = id => document.getElementById(id);
let media = null, job = null, view = 'original', lastResult = null, locked = false;
const original = $('original'), result = $('result');
const formatTime = value => { const n = Number.isFinite(value) ? value : 0; return `${String(Math.floor(n/60)).padStart(2,'0')}:${(n%60).toFixed(2).padStart(5,'0')}`; };
function toast(message) { $('toast').textContent = message; $('toast').hidden = false; clearTimeout(toast.timer); toast.timer = setTimeout(() => $('toast').hidden = true, 6000); }
async function api(url, options) { const response = await fetch(url, options); const data = await response.json(); if (!response.ok) throw new Error(typeof data.detail === 'string' ? data.detail : 'Confira os ajustes e tente novamente.'); return data; }
function lock(value) { locked = value; ['preview','export','compress'].forEach(id => $(id).disabled = value || !media); ['import-top','import-empty','file'].forEach(id => $(id).disabled = value); }
['import-top','import-empty'].forEach(id => $(id).onclick = () => $('file').click());
$('file').onchange = () => { if ($('file').files[0]) loadFile($('file').files[0]); };
const zone = $('dropzone');
zone.ondragover = event => { event.preventDefault(); zone.classList.add('drag'); };
zone.ondragleave = () => zone.classList.remove('drag');
zone.ondrop = event => { event.preventDefault(); zone.classList.remove('drag'); if (!locked && event.dataTransfer.files[0]) loadFile(event.dataTransfer.files[0]); };
async function loadFile(file) {
  lock(true);window.lumaFeedback?.('import','running'); toast('Importando vídeo para o estúdio local…');
  try {
    const body = new FormData(); body.append('file', file);
    const nextMedia = await api('/api/media', {method:'POST',body});
    original.pause(); result.pause(); media = nextMedia; lastResult = null;
    original.src = media.url; result.removeAttribute('src'); result.load();
    $('empty').hidden = true; $('player').hidden = false;
    $('filename').textContent = media.name; $('file-detail').textContent = `${media.width} × ${media.height} · ${media.fps.toFixed(2)} fps · ${formatTime(media.duration)} · ${media.codec.toUpperCase()}`;
    $('source-tag').textContent = `${Math.round(media.fps)} FPS`; $('start').max = Math.max(0, media.duration - .1);
    $('start').value = 0; $('job-box').hidden = true;
    document.querySelectorAll('[data-view]').forEach(button => button.disabled = button.dataset.view !== 'original');
    ['play','seek','mute'].forEach(id => $(id).disabled = false); setView('original');
    window.lumaFeedback?.('import','done');toast('Vídeo pronto. Escolha um preset e teste uma prévia.');
  } catch(error) {window.lumaFeedback?.('import','error'); toast(error.message); } finally { lock(false); $('file').value = ''; }
}
const presets = {clean:{shutter:120,samples:9,sharpness:0},smooth:{shutter:180,samples:15,sharpness:10},cinema:{shutter:270,samples:15,sharpness:0}};
document.querySelectorAll('[data-preset]').forEach(button => button.onclick = () => {
  const preset = presets[button.dataset.preset]; Object.entries(preset).forEach(([key,value]) => $(key).value = value);
  $('protection').checked = true; document.querySelectorAll('[data-preset]').forEach(b => b.classList.toggle('active', b === button)); updateLabels(); markStale();
});
function updateLabels() {$('compression-hint').textContent=$('compression_mode').value==='aggressive'?'HEVC com compressão forte: prioriza tamanho, com perda visível em detalhes e movimentos. Mantém resolução e FPS.':'HEVC com alta fidelidade: prioriza preservar detalhes com perda visual mínima. Não é sem perdas; o tamanho depende do vídeo.';$('engine-badge').textContent='FLOW / '+$('processing').value.toUpperCase();window.updateSummary?.();$('preview').innerHTML=icon('play')+' Testar '+$('preview_duration').value+' segundos';$('preview-tag').textContent='PRÉVIA · ATÉ '+$('preview_duration').value+' SEGUNDOS'; $('shutter-value').textContent = `${$('shutter').value}°`; $('sharpness-value').textContent = `${$('sharpness').value}%`; }
function markStale() { if (lastResult) $('job-detail').textContent = 'Ajustes alterados. Gere uma nova prévia para ver as mudanças.'; }
['compression_enabled','compression_mode','shutter','samples','sharpness','protection','fps','codec','crf','start','analysis_width','search_radius','weighting','encoder_preset','gamma','gaussian_sigma','brightness','contrast','saturation','preserve_audio','preview_duration','processing','video_encoder'].forEach(id => $(id).addEventListener('input', () => {updateLabels(); markStale(); if(['shutter','samples','sharpness'].includes(id)) document.querySelectorAll('[data-preset]').forEach(b => b.classList.remove('active')); }));
$('codec').onchange = () => {if($('compression_enabled').checked&&$('codec').value==='prores')$('codec').value='h264';$('codec').querySelector('[value="prores"]').disabled=$('compression_enabled').checked;$('video_encoder').querySelector('[value="cpu"]').textContent=$('compression_enabled').checked?'CPU · HEVC':'CPU · H.264'; $('crf').disabled = $('codec').value === 'prores'||$('compression_enabled').checked;$('video_encoder').disabled=$('codec').value==='prores'&&!$('compression_enabled').checked;$('codec').querySelector('[value="h264"]').textContent=$('compression_enabled').checked?'MP4 · HEVC comprimido':'MP4 · H.264';};
$('compression_enabled').addEventListener('input',()=>{$('codec').onchange(); });
function settings(preview) {return {compression_enabled:$('compression_enabled').checked,compression_mode:$('compression_mode').value,media_id:media.id,shutter:+$('shutter').value,samples:+$('samples').value,protection:$('protection').checked,sharpness:+$('sharpness').value/100,fps:$('fps').value,codec:$('codec').value,crf:+$('crf').value,analysis_width:+$('analysis_width').value,search_radius:+$('search_radius').value,weighting:$('weighting').value,encoder_preset:$('encoder_preset').value,gamma:+$('gamma').value,gaussian_sigma:+$('gaussian_sigma').value,brightness:+$('brightness').value,contrast:+$('contrast').value,saturation:+$('saturation').value,preserve_audio:$('preserve_audio').checked,preview_duration:+$('preview_duration').value,processing:$('processing').value,video_encoder:$('video_encoder').value,start:preview ? +$('start').value : 0,preview};}
$('preview').onclick = () => submit(true); $('export').onclick = () => submit(false); $('compress').onclick=()=>submit(false,true);
async function submit(preview,compressOnly=false) {
  if (!media || locked) return;
  lock(true); original.pause(); result.pause(); $('play').innerHTML=icon('play');
  const request = {...settings(preview),compress_only:compressOnly}; $('job-box').hidden = false;window.lumaFeedback?.('render','running'); $('download').hidden = true; $('cancel').hidden = false;
  $('job-title').textContent = preview ? 'Gerando prévia…' : compressOnly ? 'Comprimindo arquivo…' : 'Exportando vídeo…'; $('progress').value = 0;
  $('job-detail').textContent = compressOnly ? 'Codificando diretamente, sem calcular blur.' : request.fps === '120' ? 'Interpolação por CPU. A primeira etapa pode levar alguns minutos.' : 'Estimando o movimento e integrando as amostras.';
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
api('/api/health').then(data=>{$('health').textContent=data.ready?'Rust · motor disponível':'FFmpeg não encontrado';$('gpu-status').textContent=(data.gpu?.ready?'GPU disponível: '+data.gpu.name:'GPU indisponível. Use CPU.')+' · '+(data.nvenc?.ready?'NVENC disponível.':'NVENC indisponível.');$('processing').querySelector('[value="gpu"]').disabled=!data.gpu?.ready;$('video_encoder').querySelector('[value="nvenc"]').disabled=!data.nvenc?.ready;if(!data.ready)toast('Instale FFmpeg e ffprobe e reinicie o aplicativo.');}).catch(()=>{$('health').textContent='Motor desconectado';});
