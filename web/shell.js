// Desktop shell and page navigation. Video state survives tab changes.
const motionPreference = matchMedia('(prefers-reduced-motion: reduce)');
let pageTransition;
window.animateChange = change => {
  if(motionPreference.matches || !document.startViewTransition) {change();return;}
  pageTransition?.skipTransition();
  pageTransition=document.startViewTransition(change);
};
const renderSlot = $('render-slot');
for (const el of [document.querySelector('.render-actions'), $('job-box'), document.querySelector('.engine-note')]) renderSlot.append(el);
window.updateSummary = () => {
  const entries=[['Rastro', $('shutter').value + '°'], ['Saída', $('fps').value === 'source' ? 'FPS original' : $('fps').value + ' fps'], ['Motor', $('processing').value==='gpu'?'GPU':'CPU'], ['Formato', $('compression_enabled').checked ? ($('compression_mode').value==='aggressive'?'HEVC / intenso':'HEVC / fidelidade') : $('codec').value === 'prores' ? 'ProRes 422 HQ' : 'MP4 / H.264']];
  entries.forEach(([label,value],i)=>{
    let row=$('studio-summary').children[i];
    if(!row){row=document.createElement('div');row.append(document.createElement('span'),document.createElement('strong'));$('studio-summary').append(row);}
    row.children[0].textContent=label;
    if(row.children[1].textContent!==value){row.children[1].textContent=value;if(!motionPreference.matches)row.children[1].animate([{opacity:.4},{opacity:1}],{duration:220});}
  });
};
window.updateSummary();
function navigate(page) {
  window.animateChange(()=>navigateNow(page));
}
function navigateNow(page) {
  for (const name of ['studio', 'settings']) $(name + '-page').hidden = name !== page;
  document.querySelectorAll('.rail-button[data-page]').forEach(button => {
    button.classList.toggle('selected', button.dataset.page === page);
    button.setAttribute('aria-current', button.dataset.page === page ? 'page' : 'false');
  });
  document.querySelector('.intro').hidden = page === 'settings';
  window.scrollTo({top:0, behavior:'instant'});
  window.updateMotionIndicators?.();
}
document.querySelectorAll('[data-page]').forEach(button => button.addEventListener('click', () => navigate(button.dataset.page)));
if (window.LUMA_DESKTOP && window.ipc) {
  document.documentElement.classList.add('desktop');
  const send = command => window.ipc.postMessage(command);
  $('window-minimize').onclick = () => send('minimize');
  $('window-maximize').onclick = () => send('maximize');
  $('window-close').onclick = () => send('close');
  // Let a second press maximize before beginning an OS drag.
  $('window-drag').onmousedown = event => {if(event.button === 0) send(event.detail === 2 ? 'maximize' : 'drag');};
  window.setWindowMaximized = maximized => {
    document.documentElement.classList.toggle('maximized', maximized);
    const button = $('window-maximize'); button.innerHTML = icon(maximized?'restore':'maximize');
    button.title = maximized?'Restaurar':'Maximizar'; button.setAttribute('aria-label', button.title + ' janela');
  };
  for (const direction of ['n','s','e','w','ne','nw','se','sw']) {
    const edge = document.createElement('div');edge.className = 'resize-edge edge-' + direction;
    edge.onpointerdown = event => {if(event.button === 0) send('resize:' + direction);};document.body.append(edge);
  }
}

// Sliding selections, restrained click waves, and feedback for actual operations.
const indicatorGroups=[];
for(const group of document.querySelectorAll('.view-tabs,.mode-tabs')){
  const indicator=document.createElement('span');indicator.className='tab-indicator';indicator.setAttribute('aria-hidden','true');group.prepend(indicator);group.classList.add('has-indicator');indicatorGroups.push([group,indicator]);
}
const railIndicator=document.createElement('span');railIndicator.className='nav-indicator';railIndicator.setAttribute('aria-hidden','true');document.querySelector('.rail').append(railIndicator);
window.updateMotionIndicators=()=>{
  for(const [group,indicator] of indicatorGroups){
    if(!group.getClientRects().length)continue;
    const active=group.querySelector('button.active');if(active){indicator.style.width=active.offsetWidth+'px';indicator.style.height=active.offsetHeight+'px';indicator.style.transform='translate('+active.offsetLeft+'px,'+active.offsetTop+'px)';}
  }
  const selected=document.querySelector('.rail-button.selected');if(selected){railIndicator.style.transform='translateY('+selected.offsetTop+'px)';railIndicator.style.left=selected.offsetLeft+'px';}
};
const resizeObserver=new ResizeObserver(window.updateMotionIndicators);
for(const [group] of indicatorGroups){resizeObserver.observe(group);new MutationObserver(window.updateMotionIndicators).observe(group,{subtree:true,attributes:true,attributeFilter:['class']});}
resizeObserver.observe(document.querySelector('.rail'));window.updateMotionIndicators();
document.addEventListener('pointerdown',event=>{
  const button=event.target.closest('button,.button');if(!button||button.disabled||motionPreference.matches||event.button!==0)return;
  const bounds=button.getBoundingClientRect(),wave=document.createElement('span');wave.className='click-wave';wave.setAttribute('aria-hidden','true');wave.style.left=(event.clientX-bounds.left)+'px';wave.style.top=(event.clientY-bounds.top)+'px';button.append(wave);wave.addEventListener('animationend',()=>wave.remove(),{once:true});wave.addEventListener('animationcancel',()=>wave.remove(),{once:true});
});
window.lumaFeedback=(kind,state)=>{
  const element=kind==='render'?$('job-box'):$('dropzone');element.dataset.state=state;
  if(kind==='render')$('dropzone').dataset.rendering=state==='running'?'true':'false';
  if(!motionPreference.matches && (state==='done'||state==='error'))element.animate([{opacity:.55,transform:'translateY(5px)'},{opacity:1,transform:'translateY(0)'}],{duration:320,easing:'cubic-bezier(.2,.8,.2,1)'});
};
for(const card of document.querySelectorAll('.presets button,.render-panel')){
  card.addEventListener('pointermove',event=>{if(motionPreference.matches)return;const rect=card.getBoundingClientRect();card.style.setProperty('--pointer-x',(event.clientX-rect.left)+'px');card.style.setProperty('--pointer-y',(event.clientY-rect.top)+'px');});
}
