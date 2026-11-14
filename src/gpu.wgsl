// Luma Motion v0.6 WGSL compute pipeline:
// 1. motion_coarse: 1/2-scale block matching for wide motion reach
// 2. motion: full-analysis-scale refinement around coarse seed
// 3. smooth_vectors: 3x3 confidence-weighted spatial vector smoothing
// 4. integrate: bidirectional motion blur with IGN jitter and color-aware dehalo
// 5. interpolate: GPU temporal frame synthesis for high-FPS output & deduplication

struct Params {
    w: u32,
    h: u32,
    gw: u32,
    gh: u32,
    fw: u32,
    fh: u32,
    radius: u32,
    samples: u32,
    shutter: f32,
    protect: u32,
    cut: u32,
    flags: u32,
    interp_t: f32,
    cfw: u32,
    cfh: u32,
    padding: u32,
}

@group(0) @binding(0) var<storage, read> previous: array<u32>;
@group(0) @binding(1) var<storage, read> current: array<u32>;
@group(0) @binding(2) var<storage, read_write> vectors: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
@group(0) @binding(4) var<storage, read> linear: array<f32>;
@group(0) @binding(5) var<storage, read> encoded: array<u32>;
@group(0) @binding(6) var<uniform> p: Params;
@group(0) @binding(7) var<storage, read> weights: array<f32>;
@group(0) @binding(8) var<storage, read_write> vectors_smooth: array<vec4<f32>>;

fn luminance(pixel: u32) -> f32 {
    return f32(((pixel & 255u) * 19u + ((pixel >> 8u) & 255u) * 183u + ((pixel >> 16u) & 255u) * 54u) / 256u);
}

fn gray_at(pos: vec2<i32>, old: bool) -> f32 {
    let q = vec2<u32>(clamp(pos, vec2<i32>(0), vec2<i32>(i32(p.gw) - 1, i32(p.gh) - 1)));
    let index = (q.y * p.h / p.gh) * p.w + q.x * p.w / p.gw;
    if (old) {
        return luminance(previous[index]);
    }
    return luminance(current[index]);
}

fn gray_half(pos: vec2<i32>, old: bool) -> f32 {
    return gray_at(pos * 2, old);
}

fn sad(pos: vec2<i32>, offset: vec2<i32>) -> f32 {
    var total = 0.0;
    for (var y = -4; y <= 4; y += 2) {
        for (var x = -4; x <= 4; x += 2) {
            let q = pos + vec2<i32>(x, y);
            total += abs(gray_at(q, false) - gray_at(q + offset, true));
        }
    }
    return total / 25.0;
}

fn sad_half(pos: vec2<i32>, offset: vec2<i32>) -> f32 {
    var total = 0.0;
    for (var y = -4; y <= 4; y += 2) {
        for (var x = -4; x <= 4; x += 2) {
            let q = pos + vec2<i32>(x, y);
            total += abs(gray_half(q, false) - gray_half(q + offset, true));
        }
    }
    return total / 25.0;
}

@compute @workgroup_size(64)
fn motion_coarse(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= p.cfw * p.cfh) {
        return;
    }
    if (p.cut != 0u || p.shutter == 0.0) {
        vectors_smooth[i] = vec4<f32>(0.0);
        return;
    }
    let cw = max(p.gw / 2u, 1u);
    let ch = max(p.gh / 2u, 1u);
    let pos = vec2<i32>(
        i32(min((i % p.cfw) * 8u, cw - 1u)),
        i32(min((i / p.cfw) * 8u, ch - 1u))
    );
    let zero = sad_half(pos, vec2<i32>(0));
    if (zero < 1.0) {
        vectors_smooth[i] = vec4<f32>(0.0);
        return;
    }
    var best = zero;
    var delta = vec2<i32>(0);
    let cr = max(i32(p.radius) / 2, 4);
    for (var y = -cr; y <= cr; y += 2) {
        for (var x = -cr; x <= cr; x += 2) {
            let score = sad_half(pos, vec2<i32>(x, y)) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    vectors_smooth[i] = vec4<f32>(vec2<f32>(delta * 2), 0.0, 0.0);
}

@compute @workgroup_size(64)
fn motion(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= p.fw * p.fh) {
        return;
    }
    if (p.cut != 0u || p.shutter == 0.0) {
        vectors[i] = vec4<f32>(0.0);
        return;
    }
    let bx = i % p.fw;
    let by = i / p.fw;
    let pos = vec2<i32>(i32(min(bx * 8u, p.gw - 1u)), i32(min(by * 8u, p.gh - 1u)));
    let zero = sad(pos, vec2<i32>(0));
    if (zero < 1.0) {
        vectors[i] = vec4<f32>(0.0, 0.0, 1.0, 0.0);
        return;
    }
    let cbx = min(bx / 2u, p.cfw - 1u);
    let cby = min(by / 2u, p.cfh - 1u);
    let seed = vec2<i32>(vectors_smooth[cby * p.cfw + cbx].xy);
    var best = zero;
    var delta = vec2<i32>(0);
    for (var y = seed.y - 3; y <= seed.y + 3; y++) {
        for (var x = seed.x - 3; x <= seed.x + 3; x++) {
            let score = sad(pos, vec2<i32>(x, y)) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    let lr = min(i32(p.radius), 8);
    for (var y = -lr; y <= lr; y += 2) {
        for (var x = -lr; x <= lr; x += 2) {
            let score = sad(pos, vec2<i32>(x, y)) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    let center = delta;
    for (var y = center.y - 1; y <= center.y + 1; y++) {
        for (var x = center.x - 1; x <= center.x + 1; x++) {
            let score = sad(pos, vec2<i32>(x, y)) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    var confidence = 1.0;
    if (p.protect != 0u) {
        confidence = clamp(1.0 - best / 28.0, 0.0, 1.0) * clamp((zero - best) / 6.0, 0.0, 1.0);
    }
    vectors[i] = vec4<f32>(vec2<f32>(delta), confidence, 0.0);
}

@compute @workgroup_size(64)
fn smooth_vectors(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= p.fw * p.fh) {
        return;
    }
    let center = vectors[i];
    if ((p.flags & 1u) == 0u || p.cut != 0u || p.shutter == 0.0) {
        vectors_smooth[i] = center;
        return;
    }
    let bx = i32(i % p.fw);
    let by = i32(i / p.fw);
    let cw = center.z + 0.05;
    var sum_xy = center.xy * cw * 2.0;
    var sum_c = center.z * 2.0;
    var w_total = cw * 2.0;
    var count = 2.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let nx = u32(clamp(bx + dx, 0, i32(p.fw) - 1));
            let ny = u32(clamp(by + dy, 0, i32(p.fh) - 1));
            let n = vectors[ny * p.fw + nx];
            let w = n.z + 0.05;
            sum_xy += n.xy * w;
            sum_c += n.z;
            w_total += w;
            count += 1.0;
        }
    }
    vectors_smooth[i] = vec4<f32>(sum_xy / w_total, clamp(sum_c / count, 0.0, 1.0), 0.0);
}

fn vector_at(pos: vec2<f32>) -> vec3<f32> {
    let q = clamp(pos / 8.0, vec2<f32>(0.0), vec2<f32>(f32(p.fw) - 1.0, f32(p.fh) - 1.0));
    let a = vec2<u32>(q);
    let b = min(a + vec2<u32>(1u), vec2<u32>(p.fw - 1u, p.fh - 1u));
    let t = q - vec2<f32>(a);
    return vectors_smooth[a.y * p.fw + a.x].xyz * (1.0 - t.x) * (1.0 - t.y)
        + vectors_smooth[a.y * p.fw + b.x].xyz * t.x * (1.0 - t.y)
        + vectors_smooth[b.y * p.fw + a.x].xyz * (1.0 - t.x) * t.y
        + vectors_smooth[b.y * p.fw + b.x].xyz * t.x * t.y;
}

fn color_curr(index: u32) -> vec3<f32> {
    let pixel = current[index];
    return vec3<f32>(linear[pixel & 255u], linear[(pixel >> 8u) & 255u], linear[(pixel >> 16u) & 255u]);
}

fn color_prev(index: u32) -> vec3<f32> {
    let pixel = previous[index];
    return vec3<f32>(linear[pixel & 255u], linear[(pixel >> 8u) & 255u], linear[(pixel >> 16u) & 255u]);
}

fn sample_curr(pos: vec2<f32>) -> vec3<f32> {
    let q = clamp(pos, vec2<f32>(0.0), vec2<f32>(f32(p.w) - 1.0, f32(p.h) - 1.0));
    let a = vec2<u32>(q);
    let b = min(a + vec2<u32>(1u), vec2<u32>(p.w - 1u, p.h - 1u));
    let t = q - vec2<f32>(a);
    return color_curr(a.y * p.w + a.x) * (1.0 - t.x) * (1.0 - t.y)
        + color_curr(a.y * p.w + b.x) * t.x * (1.0 - t.y)
        + color_curr(b.y * p.w + a.x) * (1.0 - t.x) * t.y
        + color_curr(b.y * p.w + b.x) * t.x * t.y;
}

fn sample_prev(pos: vec2<f32>) -> vec3<f32> {
    let q = clamp(pos, vec2<f32>(0.0), vec2<f32>(f32(p.w) - 1.0, f32(p.h) - 1.0));
    let a = vec2<u32>(q);
    let b = min(a + vec2<u32>(1u), vec2<u32>(p.w - 1u, p.h - 1u));
    let t = q - vec2<f32>(a);
    return color_prev(a.y * p.w + a.x) * (1.0 - t.x) * (1.0 - t.y)
        + color_prev(a.y * p.w + b.x) * t.x * (1.0 - t.y)
        + color_prev(b.y * p.w + a.x) * (1.0 - t.x) * t.y
        + color_prev(b.y * p.w + b.x) * t.x * t.y;
}

fn ign(pos: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pos, vec2<f32>(0.06711056, 0.00583715))));
}

fn luma_lin(c: vec3<f32>) -> f32 {
    return 0.0722 * c.x + 0.7152 * c.y + 0.2126 * c.z;
}

@compute @workgroup_size(16, 16)
fn integrate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.w || id.y >= p.h) {
        return;
    }
    let index = id.y * p.w + id.x;
    if (p.cut != 0u || p.shutter == 0.0) {
        output[index] = current[index];
        return;
    }
    let scale = f32(p.w) / f32(p.gw);
    let pos = vec2<f32>(id.xy);
    let v = vector_at(pos / scale);
    if (v.z < 0.02 || abs(v.x) + abs(v.y) < 0.01) {
        output[index] = current[index];
        return;
    }
    let delta = clamp(v.xy * scale, vec2<f32>(-160.0), vec2<f32>(160.0));
    let center_c = color_curr(index);
    let center_l = luma_lin(center_c);
    var jitter = 0.0;
    if ((p.flags & 4u) != 0u) {
        jitter = (ign(pos) - 0.5) / f32(p.samples);
    }
    var total = vec3<f32>(0.0);
    var w_sum = 0.0;
    for (var i = 0u; i < p.samples; i++) {
        let base_t = f32(i) / f32(p.samples - 1u) - 0.5;
        let t = clamp(base_t + jitter, -0.5, 0.5) * p.shutter / 360.0;
        var c: vec3<f32>;
        if (t < 0.0) {
            c = sample_prev(pos + delta * (t + 1.0));
        } else {
            c = sample_curr(pos + delta * t);
        }
        var w = weights[i];
        if ((p.flags & 2u) != 0u) {
            let diff = abs(luma_lin(c) - center_l);
            w *= max(exp(-diff * diff * 6.0), 0.15);
        }
        w_sum += w;
        total += c * w;
    }
    let inv = select(1.0, 1.0 / w_sum, w_sum > 0.0);
    let value = clamp((total * inv) * v.z + center_c * (1.0 - v.z), vec3<f32>(0.0), vec3<f32>(1.0));
    let bins = vec3<u32>(round(value * 4096.0));
    output[index] = encoded[bins.x] | (encoded[bins.y] << 8u) | (encoded[bins.z] << 16u);
}

@compute @workgroup_size(16, 16)
fn interpolate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.w || id.y >= p.h) {
        return;
    }
    let index = id.y * p.w + id.x;
    let t = clamp(p.interp_t, 0.0, 1.0);
    if (p.cut != 0u) {
        output[index] = select(previous[index], current[index], t >= 0.5);
        return;
    }
    let scale = f32(p.w) / f32(p.gw);
    let pos = vec2<f32>(id.xy);
    let v = vector_at(pos / scale);
    let delta = clamp(v.xy * scale, vec2<f32>(-160.0), vec2<f32>(160.0));
    // Bidirectional motion-compensated warp between previous (t=0) and current (t=1)
    let c_prev = sample_prev(pos + delta * t);
    let c_curr = sample_curr(pos - delta * (1.0 - t));
    let warped = mix(c_prev, c_curr, t);
    let fallback = mix(color_prev(index), color_curr(index), t);
    let value = clamp(mix(fallback, warped, v.z), vec3<f32>(0.0), vec3<f32>(1.0));
    let bins = vec3<u32>(round(value * 4096.0));
    output[index] = encoded[bins.x] | (encoded[bins.y] << 8u) | (encoded[bins.z] << 16u);
}
