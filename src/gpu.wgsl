// Original compute implementation of Luma's block search and exposure integration.
struct Params { w:u32, h:u32, gw:u32, gh:u32, fw:u32, fh:u32, radius:u32, samples:u32, shutter:f32, protect:u32, cut:u32, padding:u32 }
@group(0) @binding(0) var<storage,read> previous:array<u32>;
@group(0) @binding(1) var<storage,read> current:array<u32>;
@group(0) @binding(2) var<storage,read_write> vectors:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> output:array<u32>;
@group(0) @binding(4) var<storage,read> linear:array<f32>;
@group(0) @binding(5) var<storage,read> encoded:array<u32>;
@group(0) @binding(6) var<uniform> p:Params;
@group(0) @binding(7) var<storage,read> weights:array<f32>;
fn luminance(pixel:u32)->f32 { return f32(((pixel & 255u)*19u+((pixel>>8u)&255u)*183u+((pixel>>16u)&255u)*54u)/256u); }
fn gray_at(pos:vec2<i32>, old:bool)->f32 {
 let q=vec2<u32>(clamp(pos,vec2<i32>(0),vec2<i32>(i32(p.gw)-1,i32(p.gh)-1)));
 let index=(q.y*p.h/p.gh)*p.w+q.x*p.w/p.gw;
 if(old){return luminance(previous[index]);} return luminance(current[index]);
}
fn sad(pos:vec2<i32>, offset:vec2<i32>)->f32 {
 var total=0.0;
 for(var y=-4;y<=4;y+=2){for(var x=-4;x<=4;x+=2){let q=pos+vec2<i32>(x,y);total+=abs(gray_at(q,false)-gray_at(q+offset,true));}}
 return total/25.0;
}
@compute @workgroup_size(64)
fn motion(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x;if(i>=p.fw*p.fh){return;}
 if(p.cut!=0u||p.shutter==0.0){vectors[i]=vec4<f32>(0.0);return;}
 let pos=vec2<i32>(i32(min((i%p.fw)*8u,p.gw-1u)),i32(min((i/p.fw)*8u,p.gh-1u)));
 let zero=sad(pos,vec2<i32>(0));
 if(zero<1.0){vectors[i]=vec4<f32>(0.0,0.0,1.0,0.0);return;}
 var best=zero;var delta=vec2<i32>(0);let radius=i32(p.radius);
 for(var y=-radius;y<=radius;y+=2){for(var x=-radius;x<=radius;x+=2){let score=sad(pos,vec2<i32>(x,y))+f32(abs(x)+abs(y))*0.07;if(score<best){best=score;delta=vec2<i32>(x,y);}}}
 let center=delta;
 for(var y=center.y-1;y<=center.y+1;y++){for(var x=center.x-1;x<=center.x+1;x++){let score=sad(pos,vec2<i32>(x,y))+f32(abs(x)+abs(y))*0.07;if(score<best){best=score;delta=vec2<i32>(x,y);}}}
 var confidence=1.0;if(p.protect!=0u){confidence=clamp(1.0-best/28.0,0.0,1.0)*clamp((zero-best)/6.0,0.0,1.0);}
 vectors[i]=vec4<f32>(vec2<f32>(delta),confidence,0.0);
}
fn vector_at(pos:vec2<f32>)->vec3<f32> {
 let q=clamp(pos/8.0,vec2<f32>(0.0),vec2<f32>(f32(p.fw)-1.0,f32(p.fh)-1.0));let a=vec2<u32>(q);let b=min(a+vec2<u32>(1),vec2<u32>(p.fw-1u,p.fh-1u));let t=q-vec2<f32>(a);
 return vectors[a.y*p.fw+a.x].xyz*(1.0-t.x)*(1.0-t.y)+vectors[a.y*p.fw+b.x].xyz*t.x*(1.0-t.y)+vectors[b.y*p.fw+a.x].xyz*(1.0-t.x)*t.y+vectors[b.y*p.fw+b.x].xyz*t.x*t.y;
}
fn color(index:u32)->vec3<f32> {let pixel=current[index];return vec3<f32>(linear[pixel&255u],linear[(pixel>>8u)&255u],linear[(pixel>>16u)&255u]);}
fn sample_color(pos:vec2<f32>)->vec3<f32> {
 let q=clamp(pos,vec2<f32>(0.0),vec2<f32>(f32(p.w)-1.0,f32(p.h)-1.0));let a=vec2<u32>(q);let b=min(a+vec2<u32>(1),vec2<u32>(p.w-1u,p.h-1u));let t=q-vec2<f32>(a);
 return color(a.y*p.w+a.x)*(1.0-t.x)*(1.0-t.y)+color(a.y*p.w+b.x)*t.x*(1.0-t.y)+color(b.y*p.w+a.x)*(1.0-t.x)*t.y+color(b.y*p.w+b.x)*t.x*t.y;
}
@compute @workgroup_size(16,16)
fn integrate(@builtin(global_invocation_id) id:vec3<u32>) {
 if(id.x>=p.w||id.y>=p.h){return;}let index=id.y*p.w+id.x;
 if(p.cut!=0u||p.shutter==0.0){output[index]=current[index];return;}
 let scale=f32(p.w)/f32(p.gw);let pos=vec2<f32>(id.xy);let v=vector_at(pos/scale);
 if(v.z<0.02||abs(v.x)+abs(v.y)<0.01){output[index]=current[index];return;}
 let delta=clamp(v.xy*scale,vec2<f32>(-80.0),vec2<f32>(80.0));var total=vec3<f32>(0.0);
 for(var i=0u;i<p.samples;i++){let t=(f32(i)/f32(p.samples-1u)-0.5)*p.shutter/360.0;total+=sample_color(pos+delta*t)*weights[i];}
 let value=clamp(total*v.z+color(index)*(1.0-v.z),vec3<f32>(0.0),vec3<f32>(1.0));let bins=vec3<u32>(round(value*4096.0));
 output[index]=encoded[bins.x]|(encoded[bins.y]<<8u)|(encoded[bins.z]<<16u);
}
