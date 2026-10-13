import {readFile,writeFile} from 'node:fs/promises';
import {execFileSync,spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root=process.env.LUMA_URL||'http://127.0.0.1:8765';
const api=async(path,opts)=>{let r=await fetch(root+path,opts);let d=await r.json();assert.ok(r.ok,JSON.stringify(d));return d;};
const upload=async(path)=>{let f=new FormData();f.append('file',new Blob([await readFile(path)]),'compression-test.mp4');return api('/api/media',{method:'POST',body:f});};
const source='workspace/gpu-motion-test.mp4',media=await upload(source),reports=[];
const run=async(settings)=>{const j=await api('/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({media_id:media.id,...settings})});let s;do{await new Promise(r=>setTimeout(r,100));s=await api('/api/jobs/'+j.id);assert.notEqual(s.status,'error',s.error);}while(['queued','running'].includes(s.status));assert.equal(s.status,'done');return {id:j.id,...s};};
for(const mode of ['quality','aggressive']){
 const state=await run({compress_only:true,compression_mode:mode,video_encoder:'nvenc',processing:'gpu',fps:'120',brightness:.8,codec:'prores'});
 assert.equal(state.meta.codec,'hevc');assert.equal(state.meta.width,1920);assert.equal(state.meta.fps,60);assert.equal(state.meta.audio,true);assert.match(state.meta.processor,/direto/);assert.match(state.meta.encoder,/NVENC/);
 const path='workspace/compressed-'+mode+'.mp4';await writeFile(path,Buffer.from(await (await fetch(root+'/api/jobs/'+state.id+'/video')).arrayBuffer()));
 const ff=spawnSync('dist/bin/ffmpeg.exe',['-hide_banner','-i',source,'-i',path,'-lavfi','[0:v][1:v]ssim','-an','-f','null','-'],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
 assert.equal(ff.status,0,ff.stderr);const ssim=+ff.stderr.match(/All:([\d.]+)/)[1];assert.ok(ssim>(mode==='quality'?.97:.85), 'Unexpected quality regression');
 reports.push({mode,...state.meta,path,ssim});console.log({mode,...state.meta});
}
assert.ok(reports[1].bytes<reports[0].bytes,'Aggressive must be smaller than fidelity on the same footage');
const combined=await run({compression_enabled:true,compression_mode:'aggressive',video_encoder:'nvenc',processing:'gpu',shutter:180});assert.equal(combined.meta.codec,'hevc');assert.match(combined.meta.processor,/GPU/);
const preview=await run({compression_enabled:true,compression_mode:'aggressive',video_encoder:'nvenc',processing:'gpu',shutter:0,preview:true});assert.equal(preview.meta.codec,'h264');assert.equal(preview.meta.width,960);
const invalid=await fetch(root+'/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({media_id:media.id,compression_mode:'fake'})});assert.equal(invalid.status,400);
const cancelJob=await api('/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({media_id:media.id,compress_only:true,video_encoder:'cpu'})});await api('/api/jobs/'+cancelJob.id+'/cancel',{method:'POST'});
let stopped;do{await new Promise(r=>setTimeout(r,100));stopped=await api('/api/jobs/'+cancelJob.id);}while(['queued','running'].includes(stopped.status));assert.equal(stopped.status,'cancelled');
execFileSync('dist/bin/ffmpeg.exe',['-v','error','-y','-i',source,'-vf','scale=320:180','-t','0.5','-c:v','libx264','workspace/compression-small.mp4']);
const small=await upload('workspace/compression-small.mp4');const cpu=await run({media_id:small.id,compress_only:true,compression_mode:'aggressive',video_encoder:'cpu'});assert.equal(cpu.meta.codec,'hevc');assert.match(cpu.meta.encoder,/CPU/);
await writeFile('workspace/compression-validation.json',JSON.stringify({reports,combined:combined.meta,preview:preview.meta,cpu:cpu.meta,cancellation:stopped.status},null,2));console.log('Compression, direct encoding, GPU blur + HEVC, previews, CPU fallback, validation and cancellation passed.');
