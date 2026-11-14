//! Original motion-adaptive exposure integration. Parallel block matching and sampling.
use rayon::prelude::*;

#[derive(Clone, Copy, Default)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub confidence: f32,
}

fn gray_strided(frame: &[u8], w: usize, h: usize, gw: usize, gh: usize, stride: usize) -> Vec<u8> {
    (0..gw * gh)
        .map(|i| {
            let x = (i % gw) * w / gw;
            let y = (i / gw) * h / gh;
            let p = (y * w + x) * stride;
            ((frame[p] as u32 * 19 + frame[p + 1] as u32 * 183 + frame[p + 2] as u32 * 54) / 256)
                as u8
        })
        .collect()
}

fn gray(frame: &[u8], w: usize, h: usize, gw: usize, gh: usize) -> Vec<u8> {
    gray_strided(frame, w, h, gw, gh, 3)
}

// Coordinates are explicit in this hot-path image primitive.
#[allow(clippy::too_many_arguments)]
fn sad(a: &[u8], b: &[u8], w: usize, h: usize, x: i32, y: i32, dx: i32, dy: i32) -> f32 {
    let mut sum = 0u32;
    let mut count = 0;
    for by in (-4..=4).step_by(2) {
        for bx in (-4..=4).step_by(2) {
            let ax = (x + bx).clamp(0, w as i32 - 1) as usize;
            let ay = (y + by).clamp(0, h as i32 - 1) as usize;
            let tx = (x + bx + dx).clamp(0, w as i32 - 1) as usize;
            let ty = (y + by + dy).clamp(0, h as i32 - 1) as usize;
            sum += a[ay * w + ax].abs_diff(b[ty * w + tx]) as u32;
            count += 1;
        }
    }
    sum as f32 / count as f32
}

#[allow(clippy::too_many_arguments)]
fn field(
    current: &[u8],
    previous: &[u8],
    w: usize,
    h: usize,
    fw: usize,
    fh: usize,
    protect: bool,
    radius: i32,
) -> Vec<Vector> {
    // 2-level coarse-to-fine pyramid: search at half-resolution first to double effective reach.
    let cw = (w / 2).max(1);
    let ch = (h / 2).max(1);
    let cfw = cw.div_ceil(8);
    let cfh = ch.div_ceil(8);
    let mut ca = vec![0u8; cw * ch];
    let mut cb = vec![0u8; cw * ch];
    for y in 0..ch {
        let sy = (y * 2).min(h - 1);
        for x in 0..cw {
            let sx = (x * 2).min(w - 1);
            ca[y * cw + x] = current[sy * w + sx];
            cb[y * cw + x] = previous[sy * w + sx];
        }
    }
    let coarse_radius = (radius / 2).max(4);
    let coarse: Vec<(i32, i32)> = (0..cfw * cfh)
        .into_par_iter()
        .map(|i| {
            let x = (i % cfw * 8).min(cw - 1) as i32;
            let y = (i / cfw * 8).min(ch - 1) as i32;
            let zero = sad(&ca, &cb, cw, ch, x, y, 0, 0);
            if zero < 1.0 {
                return (0, 0);
            }
            let mut best = zero;
            let mut dx = 0;
            let mut dy = 0;
            for sy in (-coarse_radius..=coarse_radius).step_by(2) {
                for sx in (-coarse_radius..=coarse_radius).step_by(2) {
                    let score = sad(&ca, &cb, cw, ch, x, y, sx, sy)
                        + (sx.abs() + sy.abs()) as f32 * 0.07;
                    if score < best {
                        best = score;
                        dx = sx;
                        dy = sy;
                    }
                }
            }
            (dx * 2, dy * 2)
        })
        .collect();

    (0..fw * fh)
        .into_par_iter()
        .map(|i| {
            let bx = i % fw;
            let by = i / fw;
            let x = (bx * 8).min(w - 1) as i32;
            let y = (by * 8).min(h - 1) as i32;
            let zero = sad(current, previous, w, h, x, y, 0, 0);
            if zero < 1.0 {
                return Vector {
                    confidence: 1.0,
                    ..Vector::default()
                };
            }
            let cx_idx = (bx / 2).min(cfw - 1);
            let cy_idx = (by / 2).min(cfh - 1);
            let (seed_x, seed_y) = coarse[cy_idx * cfw + cx_idx];
            let mut best = zero;
            let mut dx = 0;
            let mut dy = 0;
            // Refine around the coarse pyramid seed, and also test local neighborhood.
            for sy in seed_y - 3..=seed_y + 3 {
                for sx in seed_x - 3..=seed_x + 3 {
                    let score = sad(current, previous, w, h, x, y, sx, sy)
                        + (sx.abs() + sy.abs()) as f32 * 0.07;
                    if score < best {
                        best = score;
                        dx = sx;
                        dy = sy;
                    }
                }
            }
            let local_radius = radius.min(8);
            for sy in (-local_radius..=local_radius).step_by(2) {
                for sx in (-local_radius..=local_radius).step_by(2) {
                    let score = sad(current, previous, w, h, x, y, sx, sy)
                        + (sx.abs() + sy.abs()) as f32 * 0.07;
                    if score < best {
                        best = score;
                        dx = sx;
                        dy = sy;
                    }
                }
            }
            let cx = dx;
            let cy = dy;
            for sy in cy - 1..=cy + 1 {
                for sx in cx - 1..=cx + 1 {
                    let score = sad(current, previous, w, h, x, y, sx, sy)
                        + (sx.abs() + sy.abs()) as f32 * 0.07;
                    if score < best {
                        best = score;
                        dx = sx;
                        dy = sy;
                    }
                }
            }
            let confidence = if protect {
                (1.0 - best / 28.0).clamp(0.0, 1.0) * ((zero - best) / 6.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            // Parabolic sub-pixel refinement around (dx, dy)
            let s_center = sad(current, previous, w, h, x, y, dx, dy);
            let s_left = sad(current, previous, w, h, x, y, dx - 1, dy);
            let s_right = sad(current, previous, w, h, x, y, dx + 1, dy);
            let s_up = sad(current, previous, w, h, x, y, dx, dy - 1);
            let s_down = sad(current, previous, w, h, x, y, dx, dy + 1);
            let denom_x = 2.0 * (s_left + s_right - 2.0 * s_center);
            let denom_y = 2.0 * (s_up + s_down - 2.0 * s_center);
            let sub_x = if denom_x > 0.01 {
                ((s_left - s_right) / denom_x).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let sub_y = if denom_y > 0.01 {
                ((s_up - s_down) / denom_y).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            Vector {
                x: dx as f32 + sub_x,
                y: dy as f32 + sub_y,
                confidence,
            }
        })
        .collect()
}

fn block_luma(gray_curr: &[u8], gw: usize, gh: usize, bx: usize, by: usize) -> f32 {
    let x = (bx * 8).min(gw - 1);
    let y = (by * 8).min(gh - 1);
    gray_curr[y * gw + x] as f32
}

fn smooth_vectors(
    vectors: &[Vector],
    gray_curr: &[u8],
    gw: usize,
    gh: usize,
    fw: usize,
    fh: usize,
) -> Vec<Vector> {
    (0..fw * fh)
        .into_par_iter()
        .map(|i| {
            let x = (i % fw) as isize;
            let y = (i / fw) as isize;
            let center = vectors[i];
            let center_l = block_luma(gray_curr, gw, gh, x as usize, y as usize);
            let mut sum_x = center.x * (center.confidence + 0.05) * 2.0;
            let mut sum_y = center.y * (center.confidence + 0.05) * 2.0;
            let mut sum_c = center.confidence * 2.0;
            let mut w_total = (center.confidence + 0.05) * 2.0;
            let mut count = 2.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = (x + dx).clamp(0, fw as isize - 1) as usize;
                    let ny = (y + dy).clamp(0, fh as isize - 1) as usize;
                    let n = vectors[ny * fw + nx];
                    let nl = block_luma(gray_curr, gw, gh, nx, ny);
                    let dl = center_l - nl;
                    let edge_w = (-dl * dl / 400.0).exp().max(0.08);
                    let w = (n.confidence + 0.05) * edge_w;
                    sum_x += n.x * w;
                    sum_y += n.y * w;
                    sum_c += n.confidence;
                    w_total += w;
                    count += 1.0;
                }
            }
            Vector {
                x: sum_x / w_total,
                y: sum_y / w_total,
                confidence: (sum_c / count).clamp(0.0, 1.0),
            }
        })
        .collect()
}

fn vector_at(vectors: &[Vector], fw: usize, fh: usize, x: f32, y: f32) -> Vector {
    let x = (x / 8.0).clamp(0.0, fw as f32 - 1.0);
    let y = (y / 8.0).clamp(0.0, fh as f32 - 1.0);
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(fw - 1);
    let y1 = (y0 + 1).min(fh - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let mut out = Vector::default();
    for (index, weight) in [
        (y0 * fw + x0, (1.0 - tx) * (1.0 - ty)),
        (y0 * fw + x1, tx * (1.0 - ty)),
        (y1 * fw + x0, (1.0 - tx) * ty),
        (y1 * fw + x1, tx * ty),
    ] {
        out.x += vectors[index].x * weight;
        out.y += vectors[index].y * weight;
        out.confidence += vectors[index].confidence * weight;
    }
    out
}

fn sample(frame: &[u8], w: usize, h: usize, x: f32, y: f32, lut: &[f32; 256]) -> [f32; 3] {
    let x = x.clamp(0.0, w as f32 - 1.0);
    let y = y.clamp(0.0, h as f32 - 1.0);
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let mut out = [0.0; 3];
    for (p, weight) in [
        ((y0 * w + x0) * 3, (1.0 - tx) * (1.0 - ty)),
        ((y0 * w + x1) * 3, tx * (1.0 - ty)),
        ((y1 * w + x0) * 3, (1.0 - tx) * ty),
        ((y1 * w + x1) * 3, tx * ty),
    ] {
        for c in 0..3 {
            out[c] += lut[frame[p + c] as usize] * weight;
        }
    }
    out
}

pub fn sample_weights(mode: &str, sigma: f32, samples: usize) -> Vec<f32> {
    let mut weights: Vec<f32> = (0..samples)
        .map(|i| {
            let t = i as f32 / (samples - 1) as f32 * 2.0 - 1.0;
            match mode {
                "uniform" => 1.0,
                "pyramid" => (1.0 - t.abs()).max(0.01),
                "ascending" => (t + 1.0) / 2.0 + 0.01,
                "descending" => (1.0 - t) / 2.0 + 0.01,
                "vegas" => (0.5 * (1.0 + (t * std::f32::consts::PI).cos()))
                    .powf(1.2)
                    .max(0.01),
                "gaussian_bound" => {
                    let s = sigma.max(0.15);
                    ((-0.5 * (t / s).powi(2)).exp() - (-0.5 / (s * s)).exp()).max(0.01)
                }
                _ => (-0.5 * (t / sigma).powi(2)).exp(),
            }
        })
        .collect();
    let sum: f32 = weights.iter().sum();
    for value in &mut weights {
        *value /= sum;
    }
    weights
}
pub fn frame_difference_strided(
    previous: &[u8],
    current: &[u8],
    w: usize,
    h: usize,
    width: usize,
    stride: usize,
) -> f32 {
    let gw = w.min(width);
    let gh = (h * gw / w).max(1);
    let a = gray_strided(current, w, h, gw, gh, stride);
    let b = gray_strided(previous, w, h, gw, gh, stride);
    a.iter()
        .zip(&b)
        .map(|(a, b)| a.abs_diff(*b) as f32)
        .sum::<f32>()
        / a.len() as f32
}
pub fn is_scene_cut(previous: &[u8], current: &[u8], w: usize, h: usize, width: usize) -> bool {
    frame_difference_strided(previous, current, w, h, width, 3) > 65.0
}
pub fn is_scene_cut_bgra(previous: &[u8], current: &[u8], w: usize, h: usize, width: usize) -> bool {
    frame_difference_strided(previous, current, w, h, width, 4) > 65.0
}
pub fn is_duplicate_frame_strided(
    previous: &[u8],
    current: &[u8],
    w: usize,
    h: usize,
    width: usize,
    stride: usize,
) -> bool {
    frame_difference_strided(previous, current, w, h, width, stride) < 0.6
}
#[cfg(test)]
pub fn is_duplicate_frame(previous: &[u8], current: &[u8], w: usize, h: usize, width: usize) -> bool {
    is_duplicate_frame_strided(previous, current, w, h, width, 3)
}
fn ign(x: usize, y: usize) -> f32 {
    let v = (0.06711056 * x as f32 + 0.00583715 * y as f32).fract();
    (52.982_918 * v).fract()
}
pub struct MotionEngine {
    linear: [f32; 256],
    encoded: Vec<u8>,
    analysis_width: usize,
    search_radius: i32,
    weighting: String,
    sigma: f32,
    vector_smoothing: bool,
    dehalo: bool,
    hud_protect: bool,
    subframe_blend: bool,
}
impl MotionEngine {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            analysis_width: 480,
            search_radius: 16,
            weighting: "gaussian".into(),
            sigma: 0.65,
            vector_smoothing: true,
            dehalo: true,
            hud_protect: true,
            subframe_blend: true,
            linear: std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)),
            encoded: (0..=4096)
                .map(|i| ((i as f32 / 4096.0).powf(1.0 / 2.2) * 255.0).round() as u8)
                .collect(),
        }
    }
    #[cfg(test)]
    pub fn configured(width: usize, radius: i32, weighting: &str, gamma: f32, sigma: f32) -> Self {
        Self::configured_full(width, radius, weighting, gamma, sigma, true, true, true, true)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn configured_full(
        width: usize,
        radius: i32,
        weighting: &str,
        gamma: f32,
        sigma: f32,
        vector_smoothing: bool,
        dehalo: bool,
        hud_protect: bool,
        subframe_blend: bool,
    ) -> Self {
        Self {
            analysis_width: width,
            search_radius: radius,
            weighting: weighting.into(),
            sigma,
            vector_smoothing,
            dehalo,
            hud_protect,
            subframe_blend,
            linear: std::array::from_fn(|i| (i as f32 / 255.0).powf(gamma)),
            encoded: (0..=4096)
                .map(|i| ((i as f32 / 4096.0).powf(1.0 / gamma) * 255.0).round() as u8)
                .collect(),
        }
    }
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        previous: Option<&[u8]>,
        current: &[u8],
        w: usize,
        h: usize,
        shutter: u32,
        samples: usize,
        protect: bool,
    ) -> Vec<u8> {
        self.apply_with_next(previous, current, None, w, h, shutter, samples, protect)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn apply_with_next(
        &self,
        previous: Option<&[u8]>,
        current: &[u8],
        next: Option<&[u8]>,
        w: usize,
        h: usize,
        shutter: u32,
        samples: usize,
        protect: bool,
    ) -> Vec<u8> {
        let Some(previous) = previous else {
            return current.to_vec();
        };
        if shutter == 0 {
            return current.to_vec();
        }
        let gw = w.min(self.analysis_width);
        let gh = (h * gw / w).max(1);
        let a = gray(current, w, h, gw, gh);
        let b = gray(previous, w, h, gw, gh);
        let difference: f32 = a
            .iter()
            .zip(&b)
            .map(|(a, b)| a.abs_diff(*b) as f32)
            .sum::<f32>()
            / a.len() as f32;
        if protect && difference > 65.0 {
            return current.to_vec();
        }
        let fw = gw.div_ceil(8);
        let fh = gh.div_ceil(8);
        let raw_vectors = field(&a, &b, gw, gh, fw, fh, protect, self.search_radius);
        let vectors = if self.vector_smoothing {
            smooth_vectors(&raw_vectors, &a, gw, gh, fw, fh)
        } else {
            raw_vectors
        };
        let next_valid = next.filter(|n| !protect || !is_scene_cut(current, n, w, h, self.analysis_width));
        let next_vectors = next_valid.map(|n| {
            let c = gray(n, w, h, gw, gh);
            let raw = field(&a, &c, gw, gh, fw, fh, protect, self.search_radius);
            if self.vector_smoothing {
                smooth_vectors(&raw, &a, gw, gh, fw, fh)
            } else {
                raw
            }
        });
        let weights = sample_weights(&self.weighting, self.sigma, samples);
        let scale = w as f32 / gw as f32;
        let mut output = vec![0u8; current.len()];
        output
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, pixel) in row.chunks_mut(3).enumerate() {
                    let vector = vector_at(&vectors, fw, fh, x as f32 / scale, y as f32 / scale);
                    let p = (y * w + x) * 3;
                    if vector.confidence < 0.02 || vector.x.abs() + vector.y.abs() < 0.01 {
                        pixel.copy_from_slice(&current[p..p + 3]);
                        continue;
                    }
                    let vx = (vector.x * scale).clamp(-160.0, 160.0);
                    let vy = (vector.y * scale).clamp(-160.0, 160.0);
                    let mut conf = vector.confidence;
                    if self.hud_protect && (vx.abs() + vy.abs()) > 1.5 {
                        let d_prev = u32::from(current[p].abs_diff(previous[p]))
                            + u32::from(current[p + 1].abs_diff(previous[p + 1]))
                            + u32::from(current[p + 2].abs_diff(previous[p + 2]));
                        if d_prev <= 6 {
                            conf *= 0.08;
                        }
                    }
                    let (fx, fy) = if let Some(ref nv) = next_vectors {
                        let v_next = vector_at(nv, fw, fh, x as f32 / scale, y as f32 / scale);
                        // Forward-backward consistency: if forward and backward agree, boost; if occluded, dampen
                        let consistency_err = ((vector.x + v_next.x).powi(2) + (vector.y + v_next.y).powi(2)).sqrt();
                        if protect && consistency_err > 6.0 {
                            conf *= (1.0 - (consistency_err - 6.0) / 14.0).clamp(0.15, 1.0);
                        }
                        (
                            (-v_next.x * scale).clamp(-160.0, 160.0),
                            (-v_next.y * scale).clamp(-160.0, 160.0),
                        )
                    } else {
                        (vx, vy)
                    };
                    let center_lin = [
                        self.linear[current[p] as usize],
                        self.linear[current[p + 1] as usize],
                        self.linear[current[p + 2] as usize],
                    ];
                    let center_luma =
                        0.0722 * center_lin[0] + 0.7152 * center_lin[1] + 0.2126 * center_lin[2];
                    let jitter = (ign(x, y) - 0.5) / samples as f32;
                    let mut integrated = [0.0; 3];
                    let mut w_sum = 0.0;
                    for (i, weight) in weights.iter().enumerate() {
                        let base_t = i as f32 / (samples - 1) as f32 - 0.5;
                        let t = (base_t + jitter).clamp(-0.5, 0.5) * shutter as f32 / 360.0;
                        let color = if self.subframe_blend {
                            let curr_s = sample(
                                current,
                                w,
                                h,
                                x as f32 + vx * t,
                                y as f32 + vy * t,
                                &self.linear,
                            );
                            if t < 0.0 {
                                let prev_s = sample(
                                    previous,
                                    w,
                                    h,
                                    x as f32 + vx * (t + 1.0),
                                    y as f32 + vy * (t + 1.0),
                                    &self.linear,
                                );
                                let m = (-t * 2.0).clamp(0.0, 1.0);
                                [
                                    curr_s[0] * (1.0 - m) + prev_s[0] * m,
                                    curr_s[1] * (1.0 - m) + prev_s[1] * m,
                                    curr_s[2] * (1.0 - m) + prev_s[2] * m,
                                ]
                            } else if let Some(n) = next_valid {
                                let next_s = sample(
                                    n,
                                    w,
                                    h,
                                    x as f32 - fx * (1.0 - t),
                                    y as f32 - fy * (1.0 - t),
                                    &self.linear,
                                );
                                let m = (t * 2.0).clamp(0.0, 1.0);
                                [
                                    curr_s[0] * (1.0 - m) + next_s[0] * m,
                                    curr_s[1] * (1.0 - m) + next_s[1] * m,
                                    curr_s[2] * (1.0 - m) + next_s[2] * m,
                                ]
                            } else {
                                curr_s
                            }
                        } else if t < 0.0 {
                            sample(
                                previous,
                                w,
                                h,
                                x as f32 + vx * (t + 1.0),
                                y as f32 + vy * (t + 1.0),
                                &self.linear,
                            )
                        } else if let Some(n) = next_valid {
                            sample(
                                n,
                                w,
                                h,
                                x as f32 - fx * (1.0 - t),
                                y as f32 - fy * (1.0 - t),
                                &self.linear,
                            )
                        } else {
                            sample(
                                current,
                                w,
                                h,
                                x as f32 + vx * t,
                                y as f32 + vy * t,
                                &self.linear,
                            )
                        };
                        let w = if self.dehalo {
                            let sample_luma =
                                0.0722 * color[0] + 0.7152 * color[1] + 0.2126 * color[2];
                            let diff = (sample_luma - center_luma).abs();
                            let bilateral = (-diff * diff * 6.0).exp().max(0.15);
                            weight * bilateral
                        } else {
                            *weight
                        };
                        w_sum += w;
                        for c in 0..3 {
                            integrated[c] += color[c] * w;
                        }
                    }
                    let inv = if w_sum > 0.0 { 1.0 / w_sum } else { 1.0 };
                    for c in 0..3 {
                        let blurred = integrated[c] * inv;
                        let value = blurred * conf + center_lin[c] * (1.0 - conf);
                        pixel[c] = self.encoded[(value.clamp(0.0, 1.0) * 4096.0).round() as usize];
                    }
                }
            });
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_and_zero_shutter_are_exact() {
        let frame: Vec<u8> = (0..96 * 64 * 3).map(|i| (i % 256) as u8).collect();
        let engine = MotionEngine::new();
        assert_eq!(
            engine.apply(Some(&frame), &frame, 96, 64, 180, 9, true),
            frame
        );
        assert_eq!(engine.apply(None, &frame, 96, 64, 180, 9, true), frame);
        assert_eq!(
            engine.apply(Some(&vec![0; frame.len()]), &frame, 96, 64, 0, 9, true),
            frame
        );
    }
    #[test]
    fn cut_is_not_blended() {
        let a = vec![0; 96 * 64 * 3];
        let b = vec![255; 96 * 64 * 3];
        assert_eq!(
            MotionEngine::new().apply(Some(&a), &b, 96, 64, 360, 9, true),
            b
        );
    }
    #[test]
    fn weighting_and_gamma_change_moving_edges() {
        let mut a = vec![0; 96 * 64 * 3];
        let mut b = a.clone();
        for y in 12..50 {
            for x in 20..50 {
                for c in 0..3 {
                    a[(y * 96 + x) * 3 + c] = 220;
                    b[(y * 96 + x + 6) * 3 + c] = 220;
                }
            }
        }
        let render = |mode, gamma, sigma| {
            MotionEngine::configured(480, 16, mode, gamma, sigma).apply(
                Some(&a),
                &b,
                96,
                64,
                360,
                15,
                false,
            )
        };
        assert_ne!(render("uniform", 1.0, 0.65), render("uniform", 2.2, 0.65));
        assert_ne!(
            render("ascending", 2.2, 0.65),
            render("descending", 2.2, 0.65)
        );
        assert_ne!(render("gaussian", 2.2, 0.1), render("gaussian", 2.2, 2.0));
        assert_ne!(render("pyramid", 2.2, 0.65), b);
    }
    #[test]
    fn translation_creates_intermediate_edges() {
        let mut a = vec![0; 96 * 64 * 3];
        let mut b = a.clone();
        for y in 12..50 {
            for x in 20..50 {
                for c in 0..3 {
                    a[(y * 96 + x) * 3 + c] = 220;
                    b[(y * 96 + x + 6) * 3 + c] = 220;
                }
            }
        }
        let out = MotionEngine::new().apply(Some(&a), &b, 96, 64, 360, 15, true);
        assert_ne!(out, b);
        assert!(out.iter().any(|v| *v > 0 && *v < 220));
    }
    #[test]
    fn duplicate_detection_and_high_samples() {
        let mut a = vec![0; 96 * 64 * 3];
        let mut b = a.clone();
        for y in 12..50 {
            for x in 20..50 {
                for c in 0..3 {
                    a[(y * 96 + x) * 3 + c] = 220;
                    b[(y * 96 + x + 8) * 3 + c] = 220;
                }
            }
        }
        assert!(is_duplicate_frame(&a, &a, 96, 64, 480));
        assert!(!is_duplicate_frame(&a, &b, 96, 64, 480));
        let out24 = MotionEngine::new().apply(Some(&a), &b, 96, 64, 360, 24, true);
        let out32 = MotionEngine::new().apply(Some(&a), &b, 96, 64, 360, 32, true);
        assert_ne!(out24, b);
        assert_ne!(out32, b);
    }
    #[test]
    fn three_frame_symmetric_and_hud_protection() {
        let mut a = vec![20; 96 * 64 * 3];
        let mut b = a.clone();
        let mut c = a.clone();
        for y in 12..50 {
            for x in 20..50 {
                for ch in 0..3 {
                    a[(y * 96 + x) * 3 + ch] = 220;
                    b[(y * 96 + x + 6) * 3 + ch] = 220;
                    c[(y * 96 + x + 12) * 3 + ch] = 220;
                }
            }
        }
        // Static bright HUD crosshair in the center (identical across a, b, c)
        for ch in 0..3 {
            a[(32 * 96 + 48) * 3 + ch] = 255;
            b[(32 * 96 + 48) * 3 + ch] = 255;
            c[(32 * 96 + 48) * 3 + ch] = 255;
        }
        let engine = MotionEngine::new();
        let out = engine.apply_with_next(Some(&a), &b, Some(&c), 96, 64, 360, 15, true);
        assert_ne!(out, b);
        // Static HUD pixel should stay near 255 despite surrounding motion
        assert!(out[(32 * 96 + 48) * 3] > 235);
    }
}
