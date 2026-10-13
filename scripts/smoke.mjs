// Run the app first, then: node scripts/smoke.mjs
import {readFile,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const root=process.env.LUMA_URL || 'http://127.0.0.1:8765';
async function api(path,options){const r=await fetch(root+path,options);const data=await r.json();assert.ok(r.ok,JSON.stringify(data));return data;}
const health=await api('/api/health');assert.equal(health.ready,true);
const form=new FormData();form.append('file',new Blob([await readFile('test-input.mp4')]),'test-input.mp4');
const media=await api('/api/media',{method:'POST',body:form});assert.equal(media.width,1920);assert.equal(media.fps,60);
const base={media_id:media.id,shutter:180,samples:9,protection:true,sharpness:0.1,fps:'source',codec:'h264',crf:18,start:0,preview:false};
const reports=[];
const cases=[['1080p60 MP4',{}],['prévia',{preview:true}],['120 fps',{preview:true,fps:'120'}],['ProRes',{codec:'prores',shutter:0}],['finalização sem áudio',{preview:true,fps:'30',weighting:'pyramid',gamma:1,gaussian_sigma:.3,brightness:.05,contrast:1.1,saturation:.8,preserve_audio:false,preview_duration:1}]];
if(health.gpu?.ready)cases.push(['GPU · 1080p60',{processing:'gpu'}],['GPU · 120fps',{processing:'gpu',preview:true,fps:'120'}],['GPU · ProRes',{processing:'gpu',codec:'prores',video_encoder:'nvenc'}]);
if(health.gpu?.ready&&health.nvenc?.ready)cases.push(['GPU + NVENC',{processing:'gpu',video_encoder:'nvenc'}]);
for(const [label,changes] of cases){
  const start=performance.now();
  const job=await api('/api/jobs',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({...base,...changes})});
  let status;
  do {await new Promise(r=>setTimeout(r,300));status=await api(`/api/jobs/${job.id}`);assert.notEqual(status.status,'error',status.error);} while(['queued','running'].includes(status.status));
  assert.equal(status.status,'done');if(changes.processing==='gpu')assert.match(status.meta.processor,/GPU/);if(changes.video_encoder==='nvenc'&&changes.codec!=='prores')assert.match(status.meta.encoder,/NVENC/);if(changes.codec==='prores')assert.match(status.meta.encoder,/CPU/);assert.equal(status.meta.audio,changes.preserve_audio!==false);
  assert.equal(status.meta.width,changes.preview?960:1920);
  assert.equal(Math.round(status.meta.fps),changes.fps?+changes.fps:60);
  assert.ok(Math.abs(status.meta.duration-media.duration)<.08,`duration ${status.meta.duration}`);
  const range=await fetch(root+`/api/jobs/${job.id}/video`,{headers:{Range:'bytes=0-99'}});assert.equal(range.status,206);assert.equal((await range.arrayBuffer()).byteLength,100);
  const download=await fetch(root+`/api/jobs/${job.id}/video?download=true`,{method:'HEAD'});assert.match(download.headers.get('content-disposition'),/attachment/);
  const seconds=(performance.now()-start)/1000;reports.push({label,seconds:+seconds.toFixed(2),meta:status.meta,id:job.id});
  console.log(label,seconds.toFixed(2)+'s',status.meta);
}
const bad=await fetch(root+'/api/jobs',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({...base,shutter:900})});assert.equal(bad.status,400);
const foreign=await fetch(root+'/api/health',{headers:{Origin:'https://example.com'}});assert.equal(foreign.status,403);
const cancelled=await api('/api/jobs',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({...base,fps:'120'})});
await api(`/api/jobs/${cancelled.id}/cancel`,{method:'POST'});
let status;
do {await new Promise(r=>setTimeout(r,300));status=await api(`/api/jobs/${cancelled.id}`);}while(['queued','running'].includes(status.status));
assert.equal(status.status,'cancelled');
await writeFile('workspace/validation.json',JSON.stringify({reports,cancellation:true,rangeRequests:true,validation:true},null,2));
console.log('All smoke checks passed.');
