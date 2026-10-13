const assert=require('node:assert/strict');
const {validate,defaults}=require('../web/configs.js');
const input=structuredClone(defaults);
input.mode='advanced';input.theme='light';input.name='Gameplay 60';input.settings.search_radius=24;input.settings.encoder_preset='slow';
assert.deepEqual(validate(JSON.parse(JSON.stringify(input))),input);
for(const bad of [null,{}, {...input,version:2},{...input,theme:'invalid'},{...input,settings:{...input.settings,shutter:NaN}},{...input,settings:{...input.settings,search_radius:500}},{...input,settings:{...input.settings,protection:'yes'}}])assert.throws(()=>validate(bad));
const sanitized=validate({...input,settings:{...input.settings,media_id:'private',start:50}});
assert.equal(sanitized.settings.media_id,undefined);assert.equal(sanitized.settings.start,undefined);
assert.equal(defaults.settings.search_radius,16);
console.log('Configuration roundtrip, sanitization and invalid-file tests passed.');

const legacy=JSON.parse(require('node:fs').readFileSync('examples/gameplay-60.luma.json','utf8'));
assert.equal(validate(legacy).settings.gamma,2.2);
for(const key of ['gamma','gaussian_sigma','brightness','contrast','saturation','preview_duration']) assert.throws(()=>validate({...input,settings:{...input.settings,[key]:NaN}}));
for(const weighting of ['gaussian','uniform','pyramid','ascending','descending']) assert.equal(validate({...input,settings:{...input.settings,weighting}}).settings.weighting,weighting);

assert.equal(validate(legacy).settings.processing,'cpu');assert.equal(validate(legacy).settings.video_encoder,'cpu');
for(const processing of ['cpu','gpu']) for(const video_encoder of ['cpu','nvenc']) assert.equal(validate({...input,settings:{...input.settings,processing,video_encoder}}).settings.video_encoder,video_encoder);
for(const key of ['processing','video_encoder']) assert.throws(()=>validate({...input,settings:{...input.settings,[key]:'bad'}}));

for(const compression_mode of ['quality','aggressive'])assert.equal(validate({...input,settings:{...input.settings,compression_enabled:true,compression_mode}}).settings.compression_mode,compression_mode);
assert.equal(validate(legacy).settings.compression_enabled,false);
assert.throws(()=>validate({...input,settings:{...input.settings,compression_mode:'fake'}}));
assert.throws(()=>validate({...input,settings:{...input.settings,compression_enabled:'yes'}}));
