// Luma Motion v0.7 WGSL compute pipeline:
// 1. motion_coarse: 1/2-scale block matching (bwd + fwd when has_next)
// 2. motion: full-analysis-scale refinement + parabolic sub-pixel + forward-backward occlusion check
// 3. smooth_vectors: 3x3 confidence-weighted spatial vector smoothing
// 4. integrate: 3-frame symmetric shutter (prev/curr/next) + IGN jitter + color-aware dehalo + HUD protection
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
@group(0) @binding(4) var<storage, read> lut: array<u32>; // [0..256) bitcast f32 linear, [256..4353) u32 encoded
@group(0) @binding(5) var<storage, read> next_frame: array<u32>;
@group(0) @binding(6) var<uniform> p: Params;
@group(0) @binding(7) var<storage, read> weights: array<f32>;
@group(0) @binding(8) var<storage, read_write> vectors_smooth: array<vec4<f32>>;

fn linear_lut(idx: u32) -> f32 {
    return bitcast<f32>(lut[idx]);
}

fn encoded_lut(idx: u32) -> u32 {
    return lut[256u + idx];
}

fn luminance(pixel: u32) -> f32 {
    return f32(((pixel & 255u) * 19u + ((pixel >> 8u) & 255u) * 183u + ((pixel >> 16u) & 255u) * 54u) / 256u);
}

// target_frame: 0u = current, 1u = previous, 2u = next_frame
fn gray_at(pos: vec2<i32>, target_frame: u32) -> f32 {
    let q = vec2<u32>(clamp(pos, vec2<i32>(0), vec2<i32>(i32(p.gw) - 1, i32(p.gh) - 1)));
    let index = (q.y * p.h / p.gh) * p.w + q.x * p.w / p.gw;
    if (target_frame == 1u) {
        return luminance(previous[index]);
    }
    if (target_frame == 2u) {
        return luminance(next_frame[index]);
    }
    return luminance(current[index]);
}

fn sad_target(pos: vec2<i32>, offset: vec2<i32>, target_frame: u32) -> f32 {
    var total = 0.0;
    for (var y = -4; y <= 4; y += 2) {
        for (var x = -4; x <= 4; x += 2) {
            let q = pos + vec2<i32>(x, y);
            total += abs(gray_at(q, 0u) - gray_at(q + offset, target_frame));
        }
    }
    return total / 25.0;
}

fn sad_half_target(pos: vec2<i32>, offset: vec2<i32>, target_frame: u32) -> f32 {
    var total = 0.0;
    for (var y = -4; y <= 4; y += 2) {
        for (var x = -4; x <= 4; x += 2) {
            let q = (pos + vec2<i32>(x, y)) * 2;
            total += abs(gray_at(q, 0u) - gray_at(q + offset * 2, target_frame));
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
    let cr = max(i32(p.radius) / 2, 4);

    // Backward coarse search (current -> previous)
    var bwd_delta = vec2<i32>(0);
    let zero_bwd = sad_half_target(pos, vec2<i32>(0), 1u);
    if (zero_bwd >= 1.0) {
        var best = zero_bwd;
        for (var y = -cr; y <= cr; y += 2) {
            for (var x = -cr; x <= cr; x += 2) {
                let score = sad_half_target(pos, vec2<i32>(x, y), 1u) + f32(abs(x) + abs(y)) * 0.07;
                if (score < best) {
                    best = score;
                    bwd_delta = vec2<i32>(x, y);
                }
            }
        }
    }

    // Forward coarse search (current -> next_frame) when has_next is set
    var fwd_delta = vec2<i32>(0);
    if ((p.flags & 8u) != 0u) {
        let zero_fwd = sad_half_target(pos, vec2<i32>(0), 2u);
        if (zero_fwd >= 1.0) {
            var best_f = zero_fwd;
            for (var y = -cr; y <= cr; y += 2) {
                for (var x = -cr; x <= cr; x += 2) {
                    let score = sad_half_target(pos, vec2<i32>(x, y), 2u) + f32(abs(x) + abs(y)) * 0.07;
                    if (score < best_f) {
                        best_f = score;
                        fwd_delta = vec2<i32>(x, y);
                    }
                }
            }
        }
    }
    vectors_smooth[i] = vec4<f32>(vec2<f32>(bwd_delta * 2), vec2<f32>(fwd_delta * 2));
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
    let zero = sad_target(pos, vec2<i32>(0), 1u);
    if (zero < 1.0) {
        vectors[i] = vec4<f32>(0.0, 0.0, 1.0, 0.0);
        return;
    }
    let cbx = min(bx / 2u, p.cfw - 1u);
    let cby = min(by / 2u, p.cfh - 1u);
    let coarse_packed = vectors_smooth[cby * p.cfw + cbx];
    let seed = vec2<i32>(coarse_packed.xy);

    var best = zero;
    var delta = vec2<i32>(0);
    for (var y = seed.y - 3; y <= seed.y + 3; y++) {
        for (var x = seed.x - 3; x <= seed.x + 3; x++) {
            let score = sad_target(pos, vec2<i32>(x, y), 1u) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    let lr = min(i32(p.radius), 8);
    for (var y = -lr; y <= lr; y += 2) {
        for (var x = -lr; x <= lr; x += 2) {
            let score = sad_target(pos, vec2<i32>(x, y), 1u) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }
    let center = delta;
    for (var y = center.y - 1; y <= center.y + 1; y++) {
        for (var x = center.x - 1; x <= center.x + 1; x++) {
            let score = sad_target(pos, vec2<i32>(x, y), 1u) + f32(abs(x) + abs(y)) * 0.07;
            if (score < best) {
                best = score;
                delta = vec2<i32>(x, y);
            }
        }
    }

    // Parabolic sub-pixel refinement
    let s_c = sad_target(pos, delta, 1u);
    let s_l = sad_target(pos, delta + vec2<i32>(-1, 0), 1u);
    let s_r = sad_target(pos, delta + vec2<i32>(1, 0), 1u);
    let s_u = sad_target(pos, delta + vec2<i32>(0, -1), 1u);
    let s_d = sad_target(pos, delta + vec2<i32>(0, 1), 1u);
    let dx_den = 2.0 * (s_l + s_r - 2.0 * s_c);
    let dy_den = 2.0 * (s_u + s_d - 2.0 * s_c);
    let sub_x = select(0.0, clamp((s_l - s_r) / dx_den, -0.5, 0.5), dx_den > 0.01);
    let sub_y = select(0.0, clamp((s_u - s_d) / dy_den, -0.5, 0.5), dy_den > 0.01);
    let bwd_vec = vec2<f32>(delta) + vec2<f32>(sub_x, sub_y);

    var confidence = 1.0;
    if (p.protect != 0u) {
        confidence = clamp(1.0 - best / 28.0, 0.0, 1.0) * clamp((zero - best) / 6.0, 0.0, 1.0);
    }

    // Forward-backward occlusion consistency check when has_next is set
    if ((p.flags & 8u) != 0u && p.protect != 0u) {
        let seed_f = vec2<i32>(coarse_packed.zw);
        let zero_f = sad_target(pos, vec2<i32>(0), 2u);
        var best_f = zero_f;
        var f_delta = vec2<i32>(0);
        for (var y = seed_f.y - 2; y <= seed_f.y + 2; y++) {
            for (var x = seed_f.x - 2; x <= seed_f.x + 2; x++) {
                let score = sad_target(pos, vec2<i32>(x, y), 2u) + f32(abs(x) + abs(y)) * 0.07;
                if (score < best_f) {
                    best_f = score;
                    f_delta = vec2<i32>(x, y);
                }
            }
        }
        let err = length(bwd_vec + vec2<f32>(f_delta));
        if (err > 6.0) {
            confidence *= clamp(1.0 - (err - 6.0) / 14.0, 0.15, 1.0);
        }
    }

    vectors[i] = vec4<f32>(bwd_vec, confidence, 0.0);
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

fn decode_pixel(pixel: u32) -> vec3<f32> {
    return vec3<f32>(
        linear_lut(pixel & 255u),
        linear_lut((pixel >> 8u) & 255u),
        linear_lut((pixel >> 16u) & 255u)
    );
}

fn sample_frame(pos: vec2<f32>, target_frame: u32) -> vec3<f32> {
    let q = clamp(pos, vec2<f32>(0.0), vec2<f32>(f32(p.w) - 1.0, f32(p.h) - 1.0));
    let a = vec2<u32>(q);
    let b = min(a + vec2<u32>(1u), vec2<u32>(p.w - 1u, p.h - 1u));
    let t = q - vec2<f32>(a);
    var c00: vec3<f32>;
    var c10: vec3<f32>;
    var c01: vec3<f32>;
    var c11: vec3<f32>;
    if (target_frame == 1u) {
        c00 = decode_pixel(previous[a.y * p.w + a.x]);
        c10 = decode_pixel(previous[a.y * p.w + b.x]);
        c01 = decode_pixel(previous[b.y * p.w + a.x]);
        c11 = decode_pixel(previous[b.y * p.w + b.x]);
    } else if (target_frame == 2u) {
        c00 = decode_pixel(next_frame[a.y * p.w + a.x]);
        c10 = decode_pixel(next_frame[a.y * p.w + b.x]);
        c01 = decode_pixel(next_frame[b.y * p.w + a.x]);
        c11 = decode_pixel(next_frame[b.y * p.w + b.x]);
    } else {
        c00 = decode_pixel(current[a.y * p.w + a.x]);
        c10 = decode_pixel(current[a.y * p.w + b.x]);
        c01 = decode_pixel(current[b.y * p.w + a.x]);
        c11 = decode_pixel(current[b.y * p.w + b.x]);
    }
    return c00 * (1.0 - t.x) * (1.0 - t.y)
        + c10 * t.x * (1.0 - t.y)
        + c01 * (1.0 - t.x) * t.y
        + c11 * t.x * t.y;
}

fn ign(pos: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pos, vec2<f32>(0.06711056, 0.00583715))));
}

fn luma_lin(c: vec3<f32>) -> f32 {
    return 0.0722 * c.x + 0.7152 * c.y + 0.2126 * c.z;
}

fn rgb_abs_diff(a: u32, b: u32) -> u32 {
    let da = abs(i32(a & 255u) - i32(b & 255u));
    let db = abs(i32((a >> 8u) & 255u) - i32((b >> 8u) & 255u));
    let dc = abs(i32((a >> 16u) & 255u) - i32((b >> 16u) & 255u));
    return u32(da + db + dc);
}

@compute @workgroup_size(16, 16)
fn integrate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.w || id.y >= p.h) {
        return;
    }
    let index = id.y * p.w + id.x;
    if (p.cut != 0u || p.shutter == 0.0) {
        output[index] = current[index] | (255u << 24u);
        return;
    }
    let scale = f32(p.w) / f32(p.gw);
    let pos = vec2<f32>(id.xy);
    let v = vector_at(pos / scale);
    if (v.z < 0.02 || abs(v.x) + abs(v.y) < 0.01) {
        output[index] = current[index] | (255u << 24u);
        return;
    }
    let delta = clamp(v.xy * scale, vec2<f32>(-160.0), vec2<f32>(160.0));
    var conf = v.z;
    if ((p.flags & 16u) != 0u && (abs(delta.x) + abs(delta.y)) > 1.5) {
        if (rgb_abs_diff(current[index], previous[index]) <= 6u) {
            conf *= 0.08;
        }
    }
    let center_c = decode_pixel(current[index]);
    let center_l = luma_lin(center_c);
    var jitter = 0.0;
    if ((p.flags & 4u) != 0u) {
        jitter = (ign(pos) - 0.5) / f32(p.samples);
    }
    let has_next = (p.flags & 8u) != 0u;
    var total = vec3<f32>(0.0);
    var w_sum = 0.0;
    for (var i = 0u; i < p.samples; i++) {
        let base_t = f32(i) / f32(p.samples - 1u) - 0.5;
        let t = clamp(base_t + jitter, -0.5, 0.5) * p.shutter / 360.0;
        var c: vec3<f32>;
        if (t < 0.0) {
            c = sample_frame(pos + delta * (t + 1.0), 1u);
        } else if (has_next) {
            c = sample_frame(pos - delta * (1.0 - t), 2u);
        } else {
            c = sample_frame(pos + delta * t, 0u);
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
    let value = clamp((total * inv) * conf + center_c * (1.0 - conf), vec3<f32>(0.0), vec3<f32>(1.0));
    let bins = vec3<u32>(round(value * 4096.0));
    output[index] = encoded_lut(bins.x) | (encoded_lut(bins.y) << 8u) | (encoded_lut(bins.z) << 16u) | (255u << 24u);
}

@compute @workgroup_size(16, 16)
fn interpolate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.w || id.y >= p.h) {
        return;
    }
    let index = id.y * p.w + id.x;
    let t = clamp(p.interp_t, 0.0, 1.0);
    if (p.cut != 0u) {
        output[index] = select(previous[index], current[index], t >= 0.5) | (255u << 24u);
        return;
    }
    let scale = f32(p.w) / f32(p.gw);
    let pos = vec2<f32>(id.xy);
    let v = vector_at(pos / scale);
    let delta = clamp(v.xy * scale, vec2<f32>(-160.0), vec2<f32>(160.0));
    let c_prev = sample_frame(pos + delta * t, 1u);
    let c_curr = sample_frame(pos - delta * (1.0 - t), 0u);
    let warped = mix(c_prev, c_curr, t);
    let fallback = mix(decode_pixel(previous[index]), decode_pixel(current[index]), t);
    let value = clamp(mix(fallback, warped, v.z), vec3<f32>(0.0), vec3<f32>(1.0));
    let bins = vec3<u32>(round(value * 4096.0));
    output[index] = encoded_lut(bins.x) | (encoded_lut(bins.y) << 8u) | (encoded_lut(bins.z) << 16u) | (255u << 24u);
}
