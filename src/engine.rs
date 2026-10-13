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
    if video["color_transfer"] == "smpte2084" || video["color_transfer"] == "arib-std-b67" {
        return Err("Converta o vídeo HDR para SDR antes de usar esta versão.".into());
    }
    Ok(Meta {
        width,
        height,
        fps,
        duration,
        codec: video["codec_name"].as_str().unwrap_or("unknown").into(),
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        source_bytes: 0,
        render_seconds: 0.0,
        processor: None,
        encoder: None,
    })
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
            video_encoder: "cpu".into(),
            compression_enabled: false,
            compression_mode: "quality".into(),
            compress_only: false,
        }
    }
}
impl Settings {
    pub fn validate_options(&self) -> Result<(), String> {
        if !["quality", "aggressive"].contains(&self.compression_mode.as_str())
            || !["cpu", "gpu"].contains(&self.processing.as_str())
            || !["cpu", "nvenc"].contains(&self.video_encoder.as_str())
            || !(1.0..=3.0).contains(&self.gamma)
            || !(0.1..=2.0).contains(&self.gaussian_sigma)
            || !(-1.0..=1.0).contains(&self.brightness)
            || !(0.5..=2.0).contains(&self.contrast)
            || !(0.0..=2.0).contains(&self.saturation)
            || !(1.0..=10.0).contains(&self.preview_duration)
            || self.shutter > 360
            || ![5, 9, 15].contains(&self.samples)
            || !(0.0..=0.5).contains(&self.sharpness)
            || !["source", "24", "30", "60", "120"].contains(&self.fps.as_str())
            || !["h264", "prores"].contains(&self.codec.as_str())
            || !(14..=28).contains(&self.crf)
            || ![320, 480, 720].contains(&self.analysis_width)
            || ![8, 16, 24].contains(&self.search_radius)
            || !["gaussian", "uniform", "pyramid", "ascending", "descending"]
                .contains(&self.weighting.as_str())
            || !["fast", "medium", "slow"].contains(&self.encoder_preset.as_str())
        {
            return Err("Configuração inválida ou fora dos limites.".into());
        }
        Ok(())
    }
    pub fn validate(&self, meta: &Meta) -> Result<(), String> {
        self.validate_options()?;
        if self.shutter > 360
            || ![5, 9, 15].contains(&self.samples)
            || !(0.0..=0.5).contains(&self.sharpness)
            || !["source", "24", "30", "60", "120"].contains(&self.fps.as_str())
            || !["h264", "prores"].contains(&self.codec.as_str())
            || !(14..=28).contains(&self.crf)
            || !self.start.is_finite()
            || self.start < 0.0
            || self.start >= meta.duration
        {
            return Err("Confira os ajustes e a posição de início.".into());
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
    if settings.compress_only || settings.shutter == 0 {
        return render_direct(source, output, settings, cancel, update, &meta, clock);
    }
    let start = if settings.preview {
        settings.start
    } else {
        0.0
    };
    let duration = if settings.preview {
        (meta.duration - start).min(settings.preview_duration)
    } else {
        meta.duration
    };
    let scale = if settings.preview {
        (960.0 / meta.width as f64).min(1.0)
    } else {
        1.0
    };
    let w = (meta.width as f64 * scale).round() as usize / 2 * 2;
    let h = (meta.height as f64 * scale).round() as usize / 2 * 2;
    let mut gpu = if settings.processing == "gpu" {
        Some(crate::gpu::GpuMotion::new(w, h, settings)?.streaming())
    } else {
        None
    };
    let use_nvenc = settings.video_encoder == "nvenc"
        && (settings.codec != "prores" || settings.preview || settings.compression_enabled);
    if use_nvenc {
        if compressed(settings) {
            hevc_nvenc_availability()?;
        } else {
            nvenc_availability()?;
        }
    }
    let fps = settings.fps.parse::<f64>().unwrap_or(meta.fps);
    let mut filters = vec![format!("scale={w}:{h}")];
    if fps > meta.fps + 0.1 {
        filters.insert(0, "tpad=stop_mode=clone:stop_duration=0.1".into());
        filters.push(format!("minterpolate=fps={fps}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1:scd=fdiff:scd_threshold=10"));
    } else if settings.fps != "source" {
        filters.push(format!("fps={fps}"));
    }
    let dl = File::create(output.with_extension("decode.log")).map_err(|e| e.to_string())?;
    let el = File::create(output.with_extension("encode.log")).map_err(|e| e.to_string())?;
    let mut decoder = ManagedChild(
        command("ffmpeg")
            .args(["-v", "error", "-ss", &start.to_string(), "-i"])
            .arg(source)
            .args([
                "-t",
                &duration.to_string(),
                "-an",
                "-vf",
                &filters.join(","),
                "-pix_fmt",
                "bgr24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::from(dl))
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let mut encode = command("ffmpeg");
    encode
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "bgr24",
            "-video_size",
            &format!("{w}x{h}"),
            "-framerate",
            &fps.to_string(),
            "-i",
            "pipe:0",
            "-ss",
            &start.to_string(),
            "-i",
        ])
        .arg(source)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0?",
            "-t",
            &duration.to_string(),
        ]);
    if !settings.preserve_audio {
        encode.args(["-an"]);
    }
    configure_encoder(&mut encode, settings, use_nvenc);
    let mut finishing = Vec::new();
    if settings.sharpness > 0.0 {
        finishing.push(format!("unsharp=5:5:{}:5:5:0", settings.sharpness));
    }
    if settings.brightness != 0.0 || settings.contrast != 1.0 || settings.saturation != 1.0 {
        finishing.push(format!(
            "eq=brightness={}:contrast={}:saturation={}",
            settings.brightness, settings.contrast, settings.saturation
        ));
    }
    if !finishing.is_empty() {
        encode.args(["-vf", &finishing.join(",")]);
    }
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
    let engine = MotionEngine::configured(
        settings.analysis_width,
        settings.search_radius,
        &settings.weighting,
        settings.gamma,
        settings.gaussian_sigma,
    );
    let mut previous: Option<Vec<u8>> = None;
    let mut frame = vec![0u8; w * h * 3];
    let mut gpu_output = Vec::with_capacity(w * h * 3);
    let mut count = 0usize;
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
        if let Some(gpu) = &mut gpu {
            gpu.apply_into(previous.as_deref(), &frame, &mut gpu_output)?;
            writer.write_all(&gpu_output).map_err(|e| e.to_string())?;
        } else {
            let blurred = engine.apply(
                previous.as_deref(),
                &frame,
                w,
                h,
                settings.shutter,
                settings.samples,
                settings.protection,
            );
            writer.write_all(&blurred).map_err(|e| e.to_string())?;
        }
        if let Some(ref mut prev) = previous {
            std::mem::swap(prev, &mut frame);
        } else {
            previous = Some(frame.clone());
        }
        count += 1;
        update((count as f64 / (duration * fps) * 100.0).min(99.0));
    }
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
    result.encoder = Some(encoder_label(settings, use_nvenc));
    result.source_bytes = meta.bytes;
    result.render_seconds = clock.elapsed().as_secs_f64();
    Ok(result)
}

pub fn nvenc_availability() -> Result<(), String> {
    static STATUS: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    STATUS
        .get_or_init(|| {
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
                    "h264_nvenc",
                    "-f",
                    "null",
                    "-",
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if result.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "NVENC indisponível. Use encoder CPU. {}",
                    String::from_utf8_lossy(&result.stderr)
                        .chars()
                        .take(500)
                        .collect::<String>()
                ))
            }
        })
        .clone()
}

fn compressed(s: &Settings) -> bool {
    !s.preview && (s.compression_enabled || s.compress_only)
}
fn configure_encoder(cmd: &mut Command, s: &Settings, nvenc: bool) {
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
    if nvenc {
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
    } else {
        cmd.args([
            "-c:v",
            if compact { "libx265" } else { "libx264" },
            "-preset",
            preset,
            "-crf",
            &quality.to_string(),
        ]);
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
fn encoder_label(s: &Settings, nvenc: bool) -> String {
    let codec = if compressed(s) {
        "HEVC"
    } else if s.codec == "prores" && !s.preview {
        "ProRes"
    } else {
        "H.264"
    };
    format!("{codec} · {}", if nvenc { "NVIDIA NVENC" } else { "CPU" })
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
    let start = if s.preview { s.start } else { 0.0 };
    let duration = if s.preview {
        (meta.duration - start).min(s.preview_duration)
    } else {
        meta.duration
    };
    let nvenc = s.video_encoder == "nvenc" && (s.codec != "prores" || s.preview || compressed(s));
    if nvenc {
        if compressed(s) {
            hevc_nvenc_availability()?;
        } else {
            nvenc_availability()?;
        }
    }
    let mut cmd = command("ffmpeg");
    cmd.args(["-y", "-v", "error", "-ss", &start.to_string(), "-i"])
        .arg(source)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-t",
            &duration.to_string(),
        ]);
    let scale = if s.preview {
        (960.0 / meta.width as f64).min(1.0)
    } else {
        1.0
    };
    let w = (meta.width as f64 * scale).round() as usize / 2 * 2;
    let h = (meta.height as f64 * scale).round() as usize / 2 * 2;
    let mut filters = Vec::new();
    if w != meta.width || h != meta.height {
        filters.push(format!("scale={w}:{h}"));
    }
    if !s.compress_only {
        let fps = s.fps.parse::<f64>().unwrap_or(meta.fps);
        if fps > meta.fps + 0.1 {
            filters.push("tpad=stop_mode=clone:stop_duration=0.1".into());
            filters.push(format!("minterpolate=fps={fps}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1:scd=fdiff:scd_threshold=10"));
        } else if s.fps != "source" {
            filters.push(format!("fps={fps}"));
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
    if !filters.is_empty() {
        cmd.args(["-vf", &filters.join(",")]);
    }
    if !s.preserve_audio {
        cmd.arg("-an");
    }
    configure_encoder(&mut cmd, s, nvenc);
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
            update((time / (duration * 10000.0)).clamp(0.0, 99.0));
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
    result.encoder = Some(encoder_label(s, nvenc));
    result.source_bytes = meta.bytes;
    result.render_seconds = clock.elapsed().as_secs_f64();
    Ok(result)
}
pub fn hevc_nvenc_availability() -> Result<(), String> {
    static STATUS: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    STATUS
        .get_or_init(|| {
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
                    "hevc_nvenc",
                    "-f",
                    "null",
                    "-",
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if result.status.success() {
                Ok(())
            } else {
                Err(
                    "HEVC NVENC indisponível nesta GPU. Selecione encoder CPU para comprimir."
                        .into(),
                )
            }
        })
        .clone()
}
