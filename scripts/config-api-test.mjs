import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const root=process.env.LUMA_URL || 'http://127.0.0.1:8765';
const before=await(await fetch(root+'/api/config')).json();
const config=JSON.parse(await readFile('examples/gameplay-60.luma.json','utf8'));
const put=body=>fetch(root+'/api/config',{method:'PUT',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
config.settings.compression_enabled=true;config.settings.compression_mode='aggressive';
try{
  const saved=await put(config);assert.equal(saved.status,200);
  const restored=await(await fetch(root+'/api/config')).json();
  assert.equal(restored.name,config.name);assert.equal(restored.settings.analysis_width,720);assert.equal(restored.settings.weighting,'uniform');
  assert.equal(restored.settings.compression_enabled,true);assert.equal(restored.settings.compression_mode,'aggressive');
  const exported=await(await fetch(root+'/api/config/export')).json();assert.equal(exported.settings.compression_mode,'aggressive');
  const invalid=await put({...config,settings:{...config.settings,search_radius:999}});assert.equal(invalid.status,400);
  const preserved=await(await fetch(root+'/api/config')).json();assert.equal(preserved.settings.search_radius,24);
  console.log('Config persistence and rejection without state loss passed.');
}finally{assert.equal((await put(before)).status,200);}
