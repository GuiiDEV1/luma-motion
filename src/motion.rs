//! Original motion-adaptive exposure integration. Parallel block matching and sampling.
use rayon::prelude::*;

#[derive(Clone, Copy, Default)]
struct Vector {
    x: f32,
    y: f32,
    confidence: f32,
}

fn gray(frame: &[u8], w: usize, h: usize, gw: usize, gh: usize) -> Vec<u8> {
    (0..gw * gh)
        .map(|i| {
            let x = (i % gw) * w / gw;
            let y = (i / gw) * h / gh;
            let p = (y * w + x) * 3;
            ((frame[p] as u32 * 19 + frame[p + 1] as u32 * 183 + frame[p + 2] as u32 * 54) / 256)
                as u8
        })
        .collect()
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
    (0..fw * fh)
        .into_par_iter()
        .map(|i| {
            let x = (i % fw * 8).min(w - 1) as i32;
            let y = (i / fw * 8).min(h - 1) as i32;
            let zero = sad(current, previous, w, h, x, y, 0, 0);
            if zero < 1.0 {
                return Vector {
                    confidence: 1.0,
                    ..Vector::default()
                };
            }
            let mut best = zero;
            let mut dx = 0;
            let mut dy = 0;
            // Coarse search, followed by one-pixel refinement. A magnitude penalty
            // favors stationary or short vectors in ambiguous, repetitive regions.
            for sy in (-radius..=radius).step_by(2) {
                for sx in (-radius..=radius).step_by(2) {
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
            Vector {
                x: dx as f32,
                y: dy as f32,
                confidence,
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
pub fn is_scene_cut(previous: &[u8], current: &[u8], w: usize, h: usize, width: usize) -> bool {
    let gw = w.min(width);
    let gh = (h * gw / w).max(1);
    let a = gray(current, w, h, gw, gh);
    let b = gray(previous, w, h, gw, gh);
    a.iter()
        .zip(&b)
        .map(|(a, b)| a.abs_diff(*b) as f32)
        .sum::<f32>()
        / a.len() as f32
        > 65.0
}
pub struct MotionEngine {
    linear: [f32; 256],
    encoded: Vec<u8>,
    analysis_width: usize,
    search_radius: i32,
    weighting: String,
    sigma: f32,
}
impl MotionEngine {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            analysis_width: 480,
            search_radius: 16,
            weighting: "gaussian".into(),
            sigma: 0.65,
            linear: std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)),
            encoded: (0..=4096)
                .map(|i| ((i as f32 / 4096.0).powf(1.0 / 2.2) * 255.0).round() as u8)
                .collect(),
        }
    }
    pub fn configured(width: usize, radius: i32, weighting: &str, gamma: f32, sigma: f32) -> Self {
        Self {
            analysis_width: width,
            search_radius: radius,
            weighting: weighting.into(),
            sigma,
            linear: std::array::from_fn(|i| (i as f32 / 255.0).powf(gamma)),
            encoded: (0..=4096)
                .map(|i| ((i as f32 / 4096.0).powf(1.0 / gamma) * 255.0).round() as u8)
                .collect(),
        }
    }
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
        let vectors = field(&a, &b, gw, gh, fw, fh, protect, self.search_radius);
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
                    let vx = (vector.x * scale).clamp(-80.0, 80.0);
                    let vy = (vector.y * scale).clamp(-80.0, 80.0);
                    let mut integrated = [0.0; 3];
                    for (i, weight) in weights.iter().enumerate() {
                        let t = (i as f32 / (samples - 1) as f32 - 0.5) * shutter as f32 / 360.0;
                        let color = sample(
                            current,
                            w,
                            h,
                            x as f32 + vx * t,
                            y as f32 + vy * t,
                            &self.linear,
                        );
                        for c in 0..3 {
                            integrated[c] += color[c] * weight;
                        }
                    }
                    for c in 0..3 {
                        let value = integrated[c] * vector.confidence
                            + self.linear[current[p + c] as usize] * (1.0 - vector.confidence);
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
}
