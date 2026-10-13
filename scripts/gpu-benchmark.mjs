import {readFile,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const root=process.env.LUMA_URL||'http://127.0.0.1:8765';
const api=async(path,options)=>{const response=await fetch(root+path,options);const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data;};
const health=await api('/api/health');assert.equal(health.gpu.ready,true);assert.equal(health.nvenc.ready,true);
const form=new FormData();form.append('file',new Blob([await readFile('workspace/gpu-motion-test.mp4')]),'gpu-motion-test.mp4');
const media=await api('/api/media',{method:'POST',body:form});const reports=[];
for(const [processing,video_encoder] of [['cpu','cpu'],['gpu','cpu'],['gpu','nvenc']]){
 const started=performance.now();const job=await api('/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({media_id:media.id,processing,video_encoder,shutter:180,samples:15,analysis_width:720,search_radius:24})});
 let state;do{await new Promise(r=>setTimeout(r,150));state=await api('/api/jobs/'+job.id);assert.notEqual(state.status,'error',state.error);}while(['queued','running'].includes(state.status));
 assert.equal(state.status,'done');assert.equal(state.meta.width,1920);assert.equal(state.meta.height,1080);assert.equal(state.meta.fps,60);assert.equal(state.meta.audio,true);assert.ok(Math.abs(state.meta.duration-2)<.05);
 assert.match(state.meta.processor,processing==='gpu'?/GPU/:/CPU/);assert.match(state.meta.encoder,video_encoder==='nvenc'?/NVENC/:/CPU/);
 const report={processing,video_encoder,seconds:+((performance.now()-started)/1000).toFixed(2),id:job.id,meta:state.meta};reports.push(report);console.log(report);
}
await writeFile('workspace/gpu-validation-v05.json',JSON.stringify({gpu:health.gpu,reports},null,2));
