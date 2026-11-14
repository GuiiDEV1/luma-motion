use crate::motion::MotionEngine;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub duration: f64,
    pub codec: String,
    pub audio: bool,
    pub hdr: bool,
    pub bytes: u64,
    pub source_bytes: u64,
    pub render_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder: Option<String>,
}

pub fn command(program: &str) -> Command {
    let adjacent = std::env::current_exe().ok().and_then(|p| {
        p.parent()
            .map(|p| p.join("bin").join(format!("{program}.exe")))
    });
    let mut command = Command::new(
        adjacent
            .filter(|p| p.is_file())
            .unwrap_or_else(|| program.into()),
    );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
pub fn probe(path: &Path) -> Result<Meta, String> {
    let result = command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr)
            .chars()
            .take(1200)
            .collect());
    }
    let info: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())?;
    let streams = info["streams"].as_array().ok_or("Arquivo sem streams.")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or("Arquivo sem vídeo.")?;
    let rate = video["avg_frame_rate"].as_str().unwrap_or("0/1");
    let parts: Vec<f64> = rate.split('/').map(|v| v.parse().unwrap_or(0.0)).collect();
    let fps = parts.first().copied().unwrap_or(0.0) / parts.get(1).copied().unwrap_or(1.0);
    let duration = info["format"]["duration"]
        .as_str()
        .unwrap_or("0")
        .parse::<f64>()
        .unwrap_or(0.0);
    let width = video["width"].as_u64().unwrap_or(0) as usize;
    let height = video["height"].as_u64().unwrap_or(0) as usize;
    if !(1.0..=240.0).contains(&fps)
        || duration <= 0.0
        || width < 2
        || height < 2
        || width * height > 3840 * 2160
    {
        return Err("Use vídeo até 4K, de 1 a 240 fps, com duração válida.".into());
    }
    let hdr = video["color_transfer"] == "smpte2084" || video["color_transfer"] == "arib-std-b67";
    Ok(Meta {
        width,
        height,
        fps,
        duration,
        codec: video["codec_name"].as_str().unwrap_or("unknown").into(),
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        hdr,
        bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        source_bytes: 0,
        render_seconds: 0.0,
        processor: None,
        encoder: None,
    })
}

fn default_speed() -> f64 {
    1.0
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    #[serde(default = "default_speed")]
    pub speed: f64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub media_id: String,
    pub shutter: u32,
    pub samples: usize,
    pub protection: bool,
    pub sharpness: f32,
    pub fps: String,
    pub codec: String,
    pub crf: u32,
    pub start: f64,
    pub preview: bool,
    pub analysis_width: usize,
    pub search_radius: i32,
    pub weighting: String,
    pub encoder_preset: String,
    pub gamma: f32,
    pub gaussian_sigma: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub preserve_audio: bool,
    pub preview_duration: f64,
    pub processing: String,
    pub video_encoder: String,
    pub compression_enabled: bool,
    pub compression_mode: String,
    pub compress_only: bool,
    pub deduplicate: bool,
    pub vector_smoothing: bool,
    pub dehalo: bool,
    pub hud_protect: bool,
    pub subframe_blend: bool,
    pub trim_start: f64,
    pub trim_end: f64,
    pub trim_only: bool,
    pub segments: Vec<Segment>,
    pub interp_mode: String,
    pub restore_quality: String,
    pub aspect_ratio: String,
    pub target_mb: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub audio_gain: f64,
    pub audio_normalize: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            media_id: String::new(),
            shutter: 120,
            samples: 9,
            protection: true,
            sharpness: 0.0,
            fps: "source".into(),
            codec: "h264".into(),
            crf: 18,
            start: 0.0,
            preview: false,
            analysis_width: 480,
            search_radius: 16,
            weighting: "gaussian".into(),
            encoder_preset: "fast".into(),
            gamma: 2.2,
            gaussian_sigma: 0.65,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            preserve_audio: true,
            preview_duration: 3.0,
            processing: "cpu".into(),
            video_encoder: "auto".into(),
            compression_enabled: false,
            compression_mode: "quality".into(),
            compress_only: false,
            deduplicate: true,
            vector_smoothing: true,
            dehalo: true,
            hud_protect: true,
            subframe_blend: true,
            trim_start: 0.0,
            trim_end: 0.0,
            trim_only: false,
            segments: Vec::new(),
            interp_mode: "auto".into(),
            restore_quality: "off".into(),
            aspect_ratio: "source".into(),
            target_mb: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            audio_gain: 1.0,
            audio_normalize: false,
        }
    }
}
impl Settings {
    pub fn validate_options(&self) -> Result<(), String> {
        let valid_fps = self.fps == "source"
            || self
                .fps
                .parse::<f64>()
                .is_ok_and(|f| f.is_finite() && (1.0..=240.0).contains(&f));
        if !["quality", "aggressive"].contains(&self.compression_mode.as_str())
            || !["cpu", "gpu"].contains(&self.processing.as_str())
            || !["auto", "cpu", "nvenc", "amf", "qsv"].contains(&self.video_encoder.as_str())
            || !["auto", "gpu", "ffmpeg", "ai_rife", "off"].contains(&self.interp_mode.as_str())
            || !["off", "deblock", "enhance", "ai_2x"].contains(&self.restore_quality.as_str())
            || !["source", "9:16", "1:1", "4:5"].contains(&self.aspect_ratio.as_str())
            || !self.target_mb.is_finite()
            || !(0.0..=500.0).contains(&self.target_mb)
            || !self.fade_in.is_finite()
            || !(0.0..=5.0).contains(&self.fade_in)
            || !self.fade_out.is_finite()
            || !(0.0..=5.0).contains(&self.fade_out)
            || !self.audio_gain.is_finite()
            || !(0.0..=3.0).contains(&self.audio_gain)
            || !(1.0..=3.0).contains(&self.gamma)
            || !(0.1..=2.0).contains(&self.gaussian_sigma)
            || !(-1.0..=1.0).contains(&self.brightness)
            || !(0.5..=2.0).contains(&self.contrast)
            || !(0.0..=2.0).contains(&self.saturation)
            || !(1.0..=10.0).contains(&self.preview_duration)
            || self.shutter > 360
            || !(3..=64).contains(&self.samples)
            || !(0.0..=0.5).contains(&self.sharpness)
            || !valid_fps
            || !["h264", "prores"].contains(&self.codec.as_str())
            || !(0..=40).contains(&self.crf)
            || !(240..=1280).contains(&self.analysis_width)
            || !(4..=48).contains(&self.search_radius)
            || !self.trim_start.is_finite()
            || self.trim_start < 0.0
            || !self.trim_end.is_finite()
            || self.trim_end < 0.0
            || ![
                "gaussian",
                "gaussian_bound",
                "vegas",
                "uniform",
                "pyramid",
                "ascending",
                "descending",
            ]
            .contains(&self.weighting.as_str())
            || !["fast", "medium", "slow"].contains(&self.encoder_preset.as_str())
        {
            return Err("Configuração inválida ou fora dos limites.".into());
        }
        Ok(())
    }
    pub fn validate(&self, meta: &Meta) -> Result<(), String> {
        self.validate_options()?;
        if !self.start.is_finite() || self.start < 0.0 || self.start >= meta.duration {
            return Err("Confira os ajustes e a posição de início.".into());
        }
        if self.trim_start >= meta.duration {
            return Err("O ponto inicial do corte excede a duração do vídeo.".into());
        }
        if self.trim_end > 0.0 && self.trim_end <= self.trim_start {
            return Err("O ponto final do corte deve ser maior que o início.".into());
        }
        for seg in &self.segments {
            if !seg.start.is_finite()
                || !seg.end.is_finite()
                || !seg.speed.is_finite()
                || seg.start < 0.0
                || seg.end > meta.duration + 0.05
                || seg.end - seg.start < 0.05
                || !(0.25..=4.0).contains(&seg.speed)
            {
                return Err("Segmento de corte inválido.".into());
            }
        }
        Ok(())
    }
}

struct ManagedChild(Child);
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn render(
    source: &Path,
    output: &Path,
    settings: &Settings,
    cancel: Arc<AtomicBool>,
    update: impl Fn(f64),
) -> Result<Meta, String> {
    let clock = std::time::Instant::now();
    let meta = probe(source)?;
    settings.validate(&meta)?;
    if settings.trim_only
        || settings.compress_only
        || (settings.shutter == 0
            && settings.restore_quality != "ai_2x"
            && settings.interp_mode != "ai_rife")
    {
        return render_direct(source, output, settings, cancel, update, &meta, clock);
    }
    let segs = resolve_segments(settings, &meta);
    let has_speed = segs.iter().any(|(_, _, spd)| (*spd - 1.0).abs() > 0.01);
    let out_duration: f64 = segs
        .iter()
        .map(|(a, b, spd)| ((b - a) / *spd).max(0.0))
        .sum::<f64>()
        .max(0.1);
    let (start, raw_duration) = if segs.len() == 1 {
        (segs[0].0, (segs[0].1 - segs[0].0).max(0.1))
    } else {
        (0.0, out_duration)
    };
    let scale = if settings.preview {
        (960.0 / meta.width as f64).min(1.0)
    } else {
        1.0
    };
    let ai_guard = if settings.restore_quality == "ai_2x" {
        Some(run_ai_upscale(
            source,
            start,
            raw_duration,
            meta.hdr,
            &cancel,
            &update,
        )?)
    } else {
        None
    };
    let fps = settings.fps.parse::<f64>().unwrap_or(meta.fps);
    let rife_guard = if settings.interp_mode == "ai_rife" && fps > meta.fps + 0.1 {
        Some(run_rife_interp(
            source,
            start,
            raw_duration,
            &meta,
            fps,
            &cancel,
            &update,
        )?)
    } else {
        None
    };
    let ai_mult = if ai_guard.is_some() { 2 } else { 1 };
    let base_w = ((meta.width * ai_mult) as f64 * scale).round() as usize / 2 * 2;
    let base_h = ((meta.height * ai_mult) as f64 * scale).round() as usize / 2 * 2;
    let (w, h) = target_dimensions(base_w, base_h, &settings.aspect_ratio);
    let mut gpu = if settings.processing == "gpu" {
        Some(crate::gpu::GpuMotion::new(w, h, settings)?.streaming())
    } else {
        None
    };
    if settings.interp_mode == "gpu" && gpu.is_none() {
        return Err("Interpolação GPU requer o motor do blur em GPU.".into());
    }
    let encoder_kind = resolve_encoder(settings)?;
    let wants_interp =
        fps > meta.fps + 0.1 && settings.interp_mode != "off" && rife_guard.is_none();
    let gpu_interp = wants_interp && gpu.is_some() && settings.interp_mode != "ffmpeg";
    let mut filters = Vec::new();
    if meta.hdr && ai_guard.is_none() && rife_guard.is_none() {
        filters.push("zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv".into());
    }
    if ai_guard.is_none() && rife_guard.is_none() {
        append_restore_pre_filters(&mut filters, settings);
    }
    if segs.len() == 1 && has_speed {
        filters.push(format!("setpts=(PTS-STARTPTS)/{:.4}", segs[0].2));
    }
    append_aspect_filter(&mut filters, w, h, &settings.aspect_ratio);
    if wants_interp && !gpu_interp {
        filters.insert(0, "tpad=stop_mode=clone:stop_duration=0.1".into());
        filters.push(format!("minterpolate=fps={fps}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1:scd=fdiff:scd_threshold=10"));
    } else if settings.fps != "source" && !gpu_interp && rife_guard.is_none() {
        filters.push(format!("fps={fps}"));
    }
    let dl = File::create(output.with_extension("decode.log")).map_err(|e| e.to_string())?;
    let el = File::create(output.with_extension("encode.log")).map_err(|e| e.to_string())?;
    let pix_fmt = if gpu.is_some() { "bgra" } else { "bgr24" };
    let stride = if gpu.is_some() { 4usize } else { 3usize };
    let mut decode_cmd = command("ffmpeg");
    if let Some(ref rg) = rife_guard {
        decode_cmd
            .args(["-v", "error", "-framerate", &fps.to_string(), "-i"])
            .arg(rg.out_pattern())
            .args(["-an", "-vf", &filters.join(",")]);
    } else if let Some(ref ai) = ai_guard {
        decode_cmd
            .args(["-v", "error", "-framerate", &meta.fps.to_string(), "-i"])
            .arg(ai.out_pattern())
            .args(["-an", "-vf", &filters.join(",")]);
    } else if segs.len() > 1 {
        let fc = build_video_concat_filter(&segs, &filters);
        decode_cmd
            .args(["-v", "error", "-i"])
            .arg(source)
            .args(["-filter_complex", &fc, "-map", "[vout]", "-an"]);
    } else {
        decode_cmd
            .args(["-v", "error", "-ss", &start.to_string(), "-i"])
            .arg(source)
            .args(["-t", &raw_duration.to_string(), "-an", "-vf", &filters.join(",")]);
    }
    let mut decoder = ManagedChild(
        decode_cmd
            .args([
                "-pix_fmt",
                pix_fmt,
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::from(dl))
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let mut finishing = Vec::new();
    if settings.restore_quality == "enhance" {
        finishing.push("cas=0.35".into());
    }
    if settings.sharpness > 0.0 {
        finishing.push(format!("unsharp=5:5:{}:5:5:0", settings.sharpness));
    }
    if settings.brightness != 0.0 || settings.contrast != 1.0 || settings.saturation != 1.0 {
        finishing.push(format!(
            "eq=brightness={}:contrast={}:saturation={}",
            settings.brightness, settings.contrast, settings.saturation
        ));
    }
    append_fade_video_filters(&mut finishing, settings, out_duration);
    let audio_filters = build_audio_post_filters(settings, out_duration, if segs.len() == 1 { segs[0].2 } else { 1.0 });
    let mut encode = command("ffmpeg");
    encode.args([
        "-y",
        "-v",
        "error",
        "-f",
        "rawvideo",
        "-pixel_format",
        pix_fmt,
        "-video_size",
        &format!("{w}x{h}"),
        "-framerate",
        &fps.to_string(),
        "-i",
        "pipe:0",
    ]);
    let has_multi_audio = segs.len() > 1 && settings.preserve_audio && meta.audio;
    if has_multi_audio {
        encode.arg("-i").arg(source);
        let mut fc = build_audio_concat_filter(1, &segs, &audio_filters);
        if !finishing.is_empty() {
            fc = format!("[0:v]{}[vfin];{fc}", finishing.join(","));
            encode.args(["-filter_complex", &fc, "-map", "[vfin]", "-map", "[aout]"]);
        } else {
            encode.args(["-filter_complex", &fc, "-map", "0:v:0", "-map", "[aout]"]);
        }
    } else if segs.len() > 1 {
        encode.args(["-map", "0:v:0", "-an"]);
        if !finishing.is_empty() {
            encode.args(["-vf", &finishing.join(",")]);
        }
    } else {
        encode
            .args(["-ss", &start.to_string(), "-i"])
            .arg(source)
            .args(["-map", "0:v:0", "-map", "1:a:0?", "-t", &raw_duration.to_string()]);
        if !settings.preserve_audio {
            encode.args(["-an"]);
        } else if !audio_filters.is_empty() {
            encode.args(["-af", &audio_filters.join(",")]);
        }
        if !finishing.is_empty() {
            encode.args(["-vf", &finishing.join(",")]);
        }
    }
    configure_encoder_with_target(&mut encode, settings, encoder_kind, out_duration, meta.audio);
    let mut encoder = ManagedChild(
        encode
            .arg(output)
            .stdin(Stdio::piped())
            .stderr(Stdio::from(el))
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let mut reader = decoder
        .0
        .stdout
        .take()
        .ok_or("Pipe de leitura indisponível.")?;
    let mut writer = encoder
        .0
        .stdin
        .take()
        .ok_or("Pipe de escrita indisponível.")?;
    let engine = MotionEngine::configured_full(
        settings.analysis_width,
        settings.search_radius,
        &settings.weighting,
        settings.gamma,
        settings.gaussian_sigma,
        settings.vector_smoothing,
        settings.dehalo,
        settings.hud_protect,
        settings.subframe_blend,
    );
    let mut previous: Option<Vec<u8>> = None;
    let mut frame = vec![0u8; w * h * stride];
    let mut gpu_output = Vec::with_capacity(w * h * stride);
    let mut interp_buf = Vec::with_capacity(w * h * stride);
    let mut count = 0usize;
    let total_expected = (out_duration * fps).max(1.0);
    let interp_ratio = if gpu_interp { fps / meta.fps } else { 1.0 };
    let mut interp_accum = 0.0f64;
    let mut dup_pending = 0usize;
    // 1-frame lookahead for 3-frame symmetric shutter window (prev, curr, next)
    let mut win_prev: Option<Vec<u8>> = None;
    let mut win_curr: Option<Vec<u8>> = None;

    let mut push_window = |next_frame: Option<&[u8]>,
                           gpu: &mut Option<crate::gpu::GpuMotion>,
                           gpu_output: &mut Vec<u8>,
                           count: &mut usize|
     -> Result<(), String> {
        if let Some(ref curr) = win_curr {
            if let Some(gpu) = gpu {
                gpu.apply_bgra_into(win_prev.as_deref(), curr, next_frame, gpu_output)?;
                writer.write_all(gpu_output).map_err(|e| e.to_string())?;
            } else {
                let blurred = engine.apply_with_next(
                    win_prev.as_deref(),
                    curr,
                    next_frame,
                    w,
                    h,
                    settings.shutter,
                    settings.samples,
                    settings.protection,
                );
                writer.write_all(&blurred).map_err(|e| e.to_string())?;
            }
            *count += 1;
            update((*count as f64 / total_expected * 100.0).min(99.0));
        }
        if let Some(nf) = next_frame {
            win_prev = win_curr.take();
            win_curr = Some(nf.to_vec());
        } else {
            win_prev = None;
            win_curr = None;
        }
        Ok(())
    };

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let mut filled = 0;
        while filled < frame.len() {
            let n = reader
                .read(&mut frame[filled..])
                .map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        if filled != frame.len() {
            return Err("Quadro incompleto ao decodificar.".into());
        }

        if settings.deduplicate
            && let Some(ref prev) = previous
            && dup_pending < 6
            && crate::motion::is_duplicate_frame_strided(
                prev,
                &frame,
                w,
                h,
                settings.analysis_width,
                stride,
            )
        {
            dup_pending += 1;
            continue;
        }

        if dup_pending > 0
            && let Some(ref prev) = previous
        {
            let steps = dup_pending + 1;
            for s_idx in 1..steps {
                let t = s_idx as f32 / steps as f32;
                if let Some(g) = &mut gpu {
                    g.interpolate_bgra_into(prev, &frame, t, &mut interp_buf)?;
                } else {
                    blend_linear(prev, &frame, t, &mut interp_buf);
                }
                push_window(Some(&interp_buf), &mut gpu, &mut gpu_output, &mut count)?;
            }
            dup_pending = 0;
        }

        if gpu_interp && let Some(ref prev) = previous {
            interp_accum += interp_ratio;
            let n_out = interp_accum.floor() as usize;
            interp_accum -= n_out as f64;
            for k in 0..n_out {
                let t = (k + 1) as f32 / n_out as f32;
                if k + 1 == n_out {
                    push_window(Some(&frame), &mut gpu, &mut gpu_output, &mut count)?;
                } else {
                    if let Some(g) = &mut gpu {
                        g.interpolate_bgra_into(prev, &frame, t, &mut interp_buf)?;
                    } else {
                        blend_linear(prev, &frame, t, &mut interp_buf);
                    }
                    push_window(Some(&interp_buf), &mut gpu, &mut gpu_output, &mut count)?;
                }
            }
        } else {
            push_window(Some(&frame), &mut gpu, &mut gpu_output, &mut count)?;
        }

        if let Some(ref mut prev) = previous {
            std::mem::swap(prev, &mut frame);
        } else {
            previous = Some(frame.clone());
        }
    }
    if dup_pending > 0
        && let Some(ref prev) = previous
    {
        for _ in 0..dup_pending {
            push_window(Some(prev), &mut gpu, &mut gpu_output, &mut count)?;
        }
    }
    push_window(None, &mut gpu, &mut gpu_output, &mut count)?;
    drop(writer);
    let ds = decoder.0.wait().map_err(|e| e.to_string())?;
    let es = encoder.0.wait().map_err(|e| e.to_string())?;
    if !ds.success() || !es.success() || count == 0 {
        let logs = [
            output.with_extension("decode.log"),
            output.with_extension("encode.log"),
        ]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");
        return Err(logs.chars().take(1600).collect());
    }
    let mut result = probe(output)?;
    result.processor = Some(
        gpu.as_ref()
            .map(|g| format!("GPU · {}", g.name))
            .unwrap_or_else(|| "CPU · Rayon".into()),
    );
    result.encoder = Some(encoder_label(settings, encoder_kind));
    result.source_bytes = meta.bytes;
    result.render_seconds = clock.elapsed().as_secs_f64();
    Ok(result)
}

pub fn probe_encoder(codec: &str) -> Result<(), String> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Result<(), String>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(existing) = cache.lock().unwrap().get(codec).cloned() {
        return existing;
    }
    let result = command("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=size=320x240:rate=30",
            "-frames:v",
            "1",
            "-c:v",
            codec,
            "-f",
            "null",
            "-",
        ])
        .output()
        .map_err(|e| e.to_string())
        .and_then(|out| {
            if out.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "Encoder {codec} indisponível: {}",
                    String::from_utf8_lossy(&out.stderr)
                        .chars()
                        .take(300)
                        .collect::<String>()
                ))
            }
        });
    cache
        .lock()
        .unwrap()
        .insert(codec.to_string(), result.clone());
    result
}

pub fn encoder_availability(kind: &str, hevc: bool) -> Result<(), String> {
    let codec = match (kind, hevc) {
        ("nvenc", true) => "hevc_nvenc",
        ("nvenc", false) => "h264_nvenc",
        ("amf", true) => "hevc_amf",
        ("amf", false) => "h264_amf",
        ("qsv", true) => "hevc_qsv",
        ("qsv", false) => "h264_qsv",
        _ => return Ok(()),
    };
    probe_encoder(codec)
}

pub fn realesrgan_availability() -> Result<(), String> {
    static STATUS: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    STATUS
        .get_or_init(|| match command("realesrgan-ncnn-vulkan").arg("-h").output() {
            Ok(_) => Ok(()),
            Err(_) => Err(
                "realesrgan-ncnn-vulkan não encontrado em bin/ ou no PATH.".into(),
            ),
        })
        .clone()
}

pub fn rife_availability() -> Result<(), String> {
    static STATUS: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    STATUS
        .get_or_init(|| match command("rife-ncnn-vulkan").arg("-h").output() {
            Ok(_) => Ok(()),
            Err(_) => Err("rife-ncnn-vulkan não encontrado em bin/ ou no PATH.".into()),
        })
        .clone()
}

fn resolve_encoder(s: &Settings) -> Result<&'static str, String> {
    if s.codec == "prores" && !s.preview && !compressed(s) {
        return Ok("cpu");
    }
    let hevc = compressed(s);
    match s.video_encoder.as_str() {
        "cpu" => Ok("cpu"),
        "nvenc" => {
            encoder_availability("nvenc", hevc)?;
            Ok("nvenc")
        }
        "amf" => {
            encoder_availability("amf", hevc)?;
            Ok("amf")
        }
        "qsv" => {
            encoder_availability("qsv", hevc)?;
            Ok("qsv")
        }
        _ => {
            for kind in ["nvenc", "amf", "qsv"] {
                if encoder_availability(kind, hevc).is_ok() {
                    return Ok(kind);
                }
            }
            Ok("cpu")
        }
    }
}

fn resolve_segments(s: &Settings, meta: &Meta) -> Vec<(f64, f64, f64)> {
    if !s.preview && !s.segments.is_empty() {
        return s
            .segments
            .iter()
            .map(|seg| {
                (
                    seg.start.clamp(0.0, meta.duration),
                    seg.end.min(meta.duration),
                    seg.speed.clamp(0.25, 4.0),
                )
            })
            .collect();
    }
    let (start, dur) = compute_window(s, meta);
    vec![(start, (start + dur).min(meta.duration), 1.0)]
}

fn target_dimensions(w: usize, h: usize, aspect: &str) -> (usize, usize) {
    let even = |v: f64| ((v.round() as usize).max(2) / 2) * 2;
    match aspect {
        "9:16" => (even(h as f64 * 9.0 / 16.0), even(h as f64)),
        "1:1" => {
            let side = w.min(h);
            (even(side as f64), even(side as f64))
        }
        "4:5" => (even(h as f64 * 4.0 / 5.0), even(h as f64)),
        _ => (even(w as f64), even(h as f64)),
    }
}

fn append_aspect_filter(filters: &mut Vec<String>, w: usize, h: usize, aspect: &str) {
    if aspect == "source" {
        filters.push(format!("scale={w}:{h}"));
    } else {
        filters.push(format!(
            "scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"
        ));
    }
}

fn atempo_chain(speed: f64) -> String {
    let mut parts = Vec::new();
    let mut rem = speed.clamp(0.25, 4.0);
    while rem > 2.0 {
        parts.push("atempo=2.0".to_string());
        rem /= 2.0;
    }
    while rem < 0.5 {
        parts.push("atempo=0.5".to_string());
        rem /= 0.5;
    }
    if (rem - 1.0).abs() > 0.005 {
        parts.push(format!("atempo={rem:.4}"));
    }
    parts.join(",")
}

fn append_fade_video_filters(filters: &mut Vec<String>, s: &Settings, out_duration: f64) {
    if s.fade_in > 0.01 {
        let d = s.fade_in.min(out_duration * 0.45);
        filters.push(format!("fade=t=in:st=0:d={d:.3}"));
    }
    if s.fade_out > 0.01 {
        let d = s.fade_out.min(out_duration * 0.45);
        let st = (out_duration - d).max(0.0);
        filters.push(format!("fade=t=out:st={st:.3}:d={d:.3}"));
    }
}

fn build_audio_post_filters(s: &Settings, out_duration: f64, single_speed: f64) -> Vec<String> {
    let mut af = Vec::new();
    if (single_speed - 1.0).abs() > 0.01 {
        let chain = atempo_chain(single_speed);
        if !chain.is_empty() {
            af.push(chain);
        }
    }
    if (s.audio_gain - 1.0).abs() > 0.01 {
        af.push(format!("volume={:.3}", s.audio_gain));
    }
    if s.audio_normalize {
        af.push("loudnorm".into());
    }
    if s.fade_in > 0.01 {
        let d = s.fade_in.min(out_duration * 0.45);
        af.push(format!("afade=t=in:st=0:d={d:.3}"));
    }
    if s.fade_out > 0.01 {
        let d = s.fade_out.min(out_duration * 0.45);
        let st = (out_duration - d).max(0.0);
        af.push(format!("afade=t=out:st={st:.3}:d={d:.3}"));
    }
    af
}

fn build_video_concat_filter(segs: &[(f64, f64, f64)], extra_filters: &[String]) -> String {
    let mut parts = Vec::new();
    let mut labels = String::new();
    for (i, (a, b, spd)) in segs.iter().enumerate() {
        let pts = if (*spd - 1.0).abs() > 0.01 {
            format!("(PTS-STARTPTS)/{spd:.4}")
        } else {
            "PTS-STARTPTS".into()
        };
        parts.push(format!(
            "[0:v]trim=start={a:.4}:end={b:.4},setpts={pts}[v{i}]"
        ));
        labels.push_str(&format!("[v{i}]"));
    }
    if extra_filters.is_empty() {
        parts.push(format!("{labels}concat=n={}:v=1:a=0[vout]", segs.len()));
    } else {
        parts.push(format!("{labels}concat=n={}:v=1:a=0[vc]", segs.len()));
        parts.push(format!("[vc]{}[vout]", extra_filters.join(",")));
    }
    parts.join(";")
}

fn build_audio_concat_filter(
    input_idx: usize,
    segs: &[(f64, f64, f64)],
    post_af: &[String],
) -> String {
    let mut parts = Vec::new();
    let mut labels = String::new();
    for (i, (a, b, spd)) in segs.iter().enumerate() {
        let tempo = if (*spd - 1.0).abs() > 0.01 {
            let c = atempo_chain(*spd);
            if c.is_empty() { String::new() } else { format!(",{c}") }
        } else {
            String::new()
        };
        parts.push(format!(
            "[{input_idx}:a]atrim=start={a:.4}:end={b:.4},asetpts=PTS-STARTPTS{tempo}[a{i}]"
        ));
        labels.push_str(&format!("[a{i}]"));
    }
    if post_af.is_empty() {
        parts.push(format!("{labels}concat=n={}:v=0:a=1[aout]", segs.len()));
    } else {
        parts.push(format!("{labels}concat=n={}:v=0:a=1[ac]", segs.len()));
        parts.push(format!("[ac]{}[aout]", post_af.join(",")));
    }
    parts.join(";")
}

fn compute_window(s: &Settings, meta: &Meta) -> (f64, f64) {
    if s.preview {
        let start = s.start.clamp(0.0, (meta.duration - 0.1).max(0.0));
        let end = if s.trim_end > start {
            s.trim_end.min(meta.duration)
        } else {
            meta.duration
        };
        (start, (end - start).min(s.preview_duration).max(0.1))
    } else {
        let start = s.trim_start.clamp(0.0, (meta.duration - 0.1).max(0.0));
        let end = if s.trim_end > start {
            s.trim_end.min(meta.duration)
        } else {
            meta.duration
        };
        (start, (end - start).max(0.1))
    }
}

fn append_restore_pre_filters(filters: &mut Vec<String>, s: &Settings) {
    match s.restore_quality.as_str() {
        "deblock" => {
            filters.push("deblock=filter=weak:block=4,deband".into());
        }
        "enhance" => {
            filters.push("deblock=filter=strong:block=4,hqdn3d=1.5:1.5:3:3,deband".into());
        }
        _ => {}
    }
}

struct AiUpscaleGuard {
    root: std::path::PathBuf,
    out_dir: std::path::PathBuf,
}

impl AiUpscaleGuard {
    fn out_pattern(&self) -> std::path::PathBuf {
        self.out_dir.join("%06d.png")
    }
}

impl Drop for AiUpscaleGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn run_ai_upscale(
    source: &Path,
    start: f64,
    duration: f64,
    hdr: bool,
    cancel: &AtomicBool,
    update: &impl Fn(f64),
) -> Result<AiUpscaleGuard, String> {
    realesrgan_availability()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("luma-ai-{}-{stamp}", std::process::id()));
    let in_dir = root.join("in");
    let out_dir = root.join("out");
    std::fs::create_dir_all(&in_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let guard = AiUpscaleGuard { root, out_dir: out_dir.clone() };

    let mut extract = command("ffmpeg");
    extract
        .args(["-v", "error", "-ss", &start.to_string(), "-i"])
        .arg(source)
        .args(["-t", &duration.to_string(), "-an"]);
    if hdr {
        extract.args([
            "-vf",
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv",
        ]);
    }
    let status = extract
        .arg(in_dir.join("%06d.png"))
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Falha ao extrair quadros para o upscaling por IA.".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    update(10.0);

    let mut ai = ManagedChild(
        command("realesrgan-ncnn-vulkan")
            .arg("-i")
            .arg(&in_dir)
            .arg("-o")
            .arg(&out_dir)
            .args(["-s", "2", "-n", "realesrgan-x4plus", "-f", "png"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let total = std::fs::read_dir(&in_dir)
        .map(|d| d.count())
        .unwrap_or(1)
        .max(1);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Some(status) = ai.0.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("realesrgan-ncnn-vulkan falhou ao processar os quadros.".into());
            }
            break;
        }
        let done = std::fs::read_dir(&out_dir).map(|d| d.count()).unwrap_or(0);
        update(10.0 + (done as f64 / total as f64) * 35.0);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(guard)
}

fn run_rife_interp(
    source: &Path,
    start: f64,
    duration: f64,
    meta: &Meta,
    target_fps: f64,
    cancel: &AtomicBool,
    update: &impl Fn(f64),
) -> Result<AiUpscaleGuard, String> {
    rife_availability()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("luma-rife-{}-{stamp}", std::process::id()));
    let in_dir = root.join("in");
    let out_dir = root.join("out");
    std::fs::create_dir_all(&in_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let guard = AiUpscaleGuard {
        root,
        out_dir: out_dir.clone(),
    };

    let mut extract = command("ffmpeg");
    extract
        .args(["-v", "error", "-ss", &start.to_string(), "-i"])
        .arg(source)
        .args(["-t", &duration.to_string(), "-an"]);
    if meta.hdr {
        extract.args([
            "-vf",
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv",
        ]);
    }
    let status = extract
        .arg(in_dir.join("%06d.png"))
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Falha ao extrair quadros para o RIFE IA.".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    update(10.0);

    let in_count = std::fs::read_dir(&in_dir).map(|d| d.count()).unwrap_or(1).max(1);
    let mult = (target_fps / meta.fps.max(1.0)).max(1.0);
    let target_frames = ((in_count as f64) * mult).round().max(in_count as f64) as usize;

    let mut ai = ManagedChild(
        command("rife-ncnn-vulkan")
            .arg("-i")
            .arg(&in_dir)
            .arg("-o")
            .arg(&out_dir)
            .args([
                "-n",
                &target_frames.to_string(),
                "-f",
                "%06d.png",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Some(status) = ai.0.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("rife-ncnn-vulkan falhou ao interpolar os quadros.".into());
            }
            break;
        }
        let done = std::fs::read_dir(&out_dir).map(|d| d.count()).unwrap_or(0);
        update(10.0 + (done as f64 / target_frames.max(1) as f64) * 35.0);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(guard)
}

fn compressed(s: &Settings) -> bool {
    !s.preview && (s.compression_enabled || s.compress_only)
}
fn configure_encoder_with_target(
    cmd: &mut Command,
    s: &Settings,
    encoder: &str,
    out_duration: f64,
    has_audio: bool,
) {
    if !s.preview && s.target_mb > 0.0 && (s.codec != "prores" || compressed(s)) {
        let compact = compressed(s);
        let aggressive = s.compression_mode == "aggressive";
        let audio_kbps = if s.preserve_audio && has_audio {
            if compact && aggressive { 96.0 } else { 128.0 }
        } else {
            0.0
        };
        let total_kbps = (s.target_mb * 8192.0 * 0.92) / out_duration.max(0.2);
        let video_kbps = (total_kbps - audio_kbps).max(120.0).round() as u32;
        let max_kbps = ((video_kbps as f64) * 1.5).round() as u32;
        let buf_kbps = video_kbps * 2;
        let preset = &s.encoder_preset;
        match encoder {
            "nvenc" => {
                let p = match preset.as_str() {
                    "slow" => "p7",
                    "medium" => "p5",
                    _ => "p3",
                };
                cmd.args([
                    "-c:v",
                    if compact { "hevc_nvenc" } else { "h264_nvenc" },
                    "-preset",
                    p,
                    "-rc",
                    "vbr",
                    "-b:v",
                    &format!("{video_kbps}k"),
                    "-maxrate",
                    &format!("{max_kbps}k"),
                    "-bufsize",
                    &format!("{buf_kbps}k"),
                ]);
            }
            "amf" => {
                let qm = match preset.as_str() {
                    "slow" => "quality",
                    "medium" => "balanced",
                    _ => "speed",
                };
                cmd.args([
                    "-c:v",
                    if compact { "hevc_amf" } else { "h264_amf" },
                    "-quality",
                    qm,
                    "-rc",
                    "vbr_latency",
                    "-b:v",
                    &format!("{video_kbps}k"),
                    "-maxrate",
                    &format!("{max_kbps}k"),
                ]);
            }
            "qsv" => {
                let qp = match preset.as_str() {
                    "slow" => "veryslow",
                    "medium" => "medium",
                    _ => "veryfast",
                };
                cmd.args([
                    "-c:v",
                    if compact { "hevc_qsv" } else { "h264_qsv" },
                    "-preset",
                    qp,
                    "-b:v",
                    &format!("{video_kbps}k"),
                    "-maxrate",
                    &format!("{max_kbps}k"),
                ]);
            }
            _ => {
                cmd.args([
                    "-c:v",
                    if compact { "libx265" } else { "libx264" },
                    "-preset",
                    preset,
                    "-b:v",
                    &format!("{video_kbps}k"),
                    "-maxrate",
                    &format!("{max_kbps}k"),
                    "-bufsize",
                    &format!("{buf_kbps}k"),
                ]);
            }
        }
        if compact {
            cmd.args(["-tag:v", "hvc1"]);
        }
        cmd.args([
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            if compact && aggressive { "96k" } else { "128k" },
            "-movflags",
            "+faststart",
        ]);
        return;
    }
    configure_encoder(cmd, s, encoder);
}

fn configure_encoder(cmd: &mut Command, s: &Settings, encoder: &str) {
    if s.codec == "prores" && !s.preview && !compressed(s) {
        cmd.args([
            "-c:v",
            "prores_ks",
            "-profile:v",
            "3",
            "-pix_fmt",
            "yuv422p10le",
            "-c:a",
            "pcm_s16le",
        ]);
        return;
    }
    let compact = compressed(s);
    let aggressive = s.compression_mode == "aggressive";
    let quality = if compact {
        if aggressive { 32 } else { 18 }
    } else {
        s.crf
    };
    let preset = if compact {
        if aggressive { "medium" } else { "slow" }
    } else {
        &s.encoder_preset
    };
    match encoder {
        "nvenc" => {
            let preset = match preset {
                "slow" => "p7",
                "medium" => "p5",
                _ => "p3",
            };
            cmd.args([
                "-c:v",
                if compact { "hevc_nvenc" } else { "h264_nvenc" },
                "-preset",
                preset,
                "-rc",
                "vbr",
                "-cq",
                &quality.to_string(),
                "-b:v",
                "0",
            ]);
        }
        "amf" => {
            let quality_mode = match preset {
                "slow" => "quality",
                "medium" => "balanced",
                _ => "speed",
            };
            cmd.args([
                "-c:v",
                if compact { "hevc_amf" } else { "h264_amf" },
                "-quality",
                quality_mode,
                "-rc",
                "cqp",
                "-qp_i",
                &quality.to_string(),
                "-qp_p",
                &quality.to_string(),
            ]);
        }
        "qsv" => {
            let qsv_preset = match preset {
                "slow" => "veryslow",
                "medium" => "medium",
                _ => "veryfast",
            };
            cmd.args([
                "-c:v",
                if compact { "hevc_qsv" } else { "h264_qsv" },
                "-preset",
                qsv_preset,
                "-global_quality",
                &quality.to_string(),
            ]);
        }
        _ => {
            cmd.args([
                "-c:v",
                if compact { "libx265" } else { "libx264" },
                "-preset",
                preset,
                "-crf",
                &quality.to_string(),
            ]);
        }
    }
    if compact {
        cmd.args(["-tag:v", "hvc1"]);
    }
    cmd.args([
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        if compact && aggressive { "96k" } else { "192k" },
        "-movflags",
        "+faststart",
    ]);
}
fn encoder_label(s: &Settings, encoder: &str) -> String {
    let codec = if compressed(s) {
        "HEVC"
    } else if s.codec == "prores" && !s.preview {
        "ProRes"
    } else {
        "H.264"
    };
    let hw = match encoder {
        "nvenc" => "NVIDIA NVENC",
        "amf" => "AMD AMF",
        "qsv" => "Intel QSV",
        _ => "CPU",
    };
    format!("{codec} · {hw}")
}
fn blend_linear(a: &[u8], b: &[u8], t: f32, out: &mut Vec<u8>) {
    out.resize(a.len(), 0);
    let inv = 1.0 - t;
    for (dst, (&pa, &pb)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
        *dst = (pa as f32 * inv + pb as f32 * t).round().clamp(0.0, 255.0) as u8;
    }
}

fn render_direct(
    source: &Path,
    output: &Path,
    s: &Settings,
    cancel: Arc<AtomicBool>,
    update: impl Fn(f64),
    meta: &Meta,
    clock: std::time::Instant,
) -> Result<Meta, String> {
    let segs = resolve_segments(s, meta);
    let has_speed = segs.iter().any(|(_, _, spd)| (*spd - 1.0).abs() > 0.01);
    let out_duration: f64 = segs
        .iter()
        .map(|(a, b, spd)| ((b - a) / *spd).max(0.0))
        .sum::<f64>()
        .max(0.1);
    let raw_duration: f64 = segs
        .iter()
        .map(|(a, b, _)| (b - a).max(0.0))
        .sum::<f64>()
        .max(0.1);
    let encoder_kind = resolve_encoder(s)?;
    let scale = if s.preview {
        (960.0 / meta.width as f64).min(1.0)
    } else {
        1.0
    };
    let base_w = (meta.width as f64 * scale).round() as usize / 2 * 2;
    let base_h = (meta.height as f64 * scale).round() as usize / 2 * 2;
    let (w, h) = target_dimensions(base_w, base_h, &s.aspect_ratio);
    let mut filters = Vec::new();
    if segs.len() == 1 && has_speed {
        filters.push(format!("setpts=(PTS-STARTPTS)/{:.4}", segs[0].2));
    }
    if w != meta.width || h != meta.height || s.aspect_ratio != "source" {
        append_aspect_filter(&mut filters, w, h, &s.aspect_ratio);
    }
    if !s.trim_only && !s.compress_only {
        append_restore_pre_filters(&mut filters, s);
        let fps = s.fps.parse::<f64>().unwrap_or(meta.fps);
        if fps > meta.fps + 0.1 && s.interp_mode != "off" {
            filters.push("tpad=stop_mode=clone:stop_duration=0.1".into());
            filters.push(format!("minterpolate=fps={fps}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1:scd=fdiff:scd_threshold=10"));
        } else if s.fps != "source" {
            filters.push(format!("fps={fps}"));
        }
        if s.restore_quality == "enhance" {
            filters.push("cas=0.35".into());
        }
        if s.sharpness > 0.0 {
            filters.push(format!("unsharp=5:5:{}:5:5:0", s.sharpness));
        }
        if s.brightness != 0.0 || s.contrast != 1.0 || s.saturation != 1.0 {
            filters.push(format!(
                "eq=brightness={}:contrast={}:saturation={}",
                s.brightness, s.contrast, s.saturation
            ));
        }
    }
    append_fade_video_filters(&mut filters, s, out_duration);
    let audio_filters = build_audio_post_filters(s, out_duration, if segs.len() == 1 { segs[0].2 } else { 1.0 });
    let mut cmd = command("ffmpeg");
    if segs.len() > 1 {
        let mut fc = build_video_concat_filter(&segs, &filters);
        let with_audio = s.preserve_audio && meta.audio;
        if with_audio {
            fc = format!("{fc};{}", build_audio_concat_filter(0, &segs, &audio_filters));
            cmd.args(["-y", "-v", "error", "-i"])
                .arg(source)
                .args(["-filter_complex", &fc, "-map", "[vout]", "-map", "[aout]"]);
        } else {
            cmd.args(["-y", "-v", "error", "-i"])
                .arg(source)
                .args(["-filter_complex", &fc, "-map", "[vout]", "-an"]);
        }
    } else {
        let start = segs[0].0;
        cmd.args(["-y", "-v", "error", "-ss", &start.to_string(), "-i"])
            .arg(source)
            .args([
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-t",
                &raw_duration.to_string(),
            ]);
        if !filters.is_empty() {
            cmd.args(["-vf", &filters.join(",")]);
        }
        if !s.preserve_audio {
            cmd.arg("-an");
        } else if !audio_filters.is_empty() {
            cmd.args(["-af", &audio_filters.join(",")]);
        }
    }
    configure_encoder_with_target(&mut cmd, s, encoder_kind, out_duration, meta.audio);
    let log_path = output.with_extension("encode.log");
    let log = File::create(&log_path).map_err(|e| e.to_string())?;
    let mut child = ManagedChild(
        cmd.args(["-progress", "pipe:1", "-nostats"])
            .arg(output)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or("Pipe de progresso indisponível.")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(time) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<f64>().ok())
                && tx.send(time).is_err()
            {
                break;
            }
        }
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Ok(time) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
            update((time / (out_duration * 10000.0)).clamp(0.0, 99.0));
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err(std::fs::read_to_string(&log_path)
                    .unwrap_or_else(|_| "Falha no encoder.".into())
                    .chars()
                    .take(1600)
                    .collect());
            }
            break;
        }
    }
    let mut result = probe(output)?;
    result.processor = Some("FFmpeg · caminho direto".into());
    result.encoder = Some(encoder_label(s, encoder_kind));
    result.source_bytes = meta.bytes;
    result.render_seconds = clock.elapsed().as_secs_f64();
    Ok(result)
}
