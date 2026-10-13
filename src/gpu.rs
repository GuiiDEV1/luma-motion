use crate::{engine::Settings, motion};
use rayon::prelude::*;
use std::sync::OnceLock;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
    padding: u32,
}

pub struct GpuMotion {
    device: wgpu::Device,
    queue: wgpu::Queue,
    group: wgpu::BindGroup,
    motion: wgpu::ComputePipeline,
    integrate: wgpu::ComputePipeline,
    previous: wgpu::Buffer,
    current: wgpu::Buffer,
    params: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
    width: usize,
    height: usize,
    settings: Settings,
    pub name: String,
    packed: Vec<u32>,
    streaming: bool,
    previous_ready: bool,
}
impl GpuMotion {
    pub fn new(width: usize, height: usize, settings: &Settings) -> Result<Self, String> {
        pollster::block_on(Self::init(width, height, settings))
    }
    async fn init(width: usize, height: usize, settings: &Settings) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("Nenhuma GPU compatível encontrada. Selecione CPU nas configurações.")?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err("O adaptador disponível é software. Selecione CPU.".into());
        }
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Luma GPU"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("GPU indisponível: {e}. Selecione CPU."))?;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let size = (width * height * 4) as u64;
        let buffer = |label, bytes, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        let previous = buffer(
            "previous",
            size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let current = buffer(
            "current",
            size,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let output = buffer(
            "output",
            size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = buffer(
            "readback",
            size,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let params = buffer(
            "params",
            48,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let gw = width.min(settings.analysis_width);
        let gh = (height * gw / width).max(1);
        let vectors = buffer(
            "vectors",
            (gw.div_ceil(8) * gh.div_ceil(8) * 16) as u64,
            wgpu::BufferUsages::STORAGE,
        );
        let linear: Vec<f32> = (0..256)
            .map(|i| (i as f32 / 255.0).powf(settings.gamma))
            .collect();
        let encoded: Vec<u32> = (0..=4096)
            .map(|i| ((i as f32 / 4096.0).powf(1.0 / settings.gamma) * 255.0).round() as u32)
            .collect();
        let weights = motion::sample_weights(
            &settings.weighting,
            settings.gaussian_sigma,
            settings.samples,
        );
        let table = |label, bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let linear = table("linear", bytemuck::cast_slice(&linear));
        let encoded = table("encoded", bytemuck::cast_slice(&encoded));
        let weights = table("weights", bytemuck::cast_slice(&weights));
        let entries: Vec<_> = (0..8)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 6 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 2 && binding != 3,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Luma compute bindings"),
            entries: &entries,
        });
        let buffers = [
            &previous, &current, &vectors, &output, &linear, &encoded, &params, &weights,
        ];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &entries,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Luma motion shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                module: &shader,
                entry_point,
            })
        };
        let motion = pipeline("motion");
        let integrate = pipeline("integrate");
        if let Some(error) = device.pop_error_scope().await {
            return Err(format!("Falha ao preparar o motor GPU: {error}"));
        }
        Ok(Self {
            device,
            queue,
            group,
            motion,
            integrate,
            previous,
            current,
            params,
            output,
            readback,
            width,
            height,
            settings: settings.clone(),
            packed: vec![0; width * height],
            streaming: false,
            previous_ready: false,
            name: format!("{} · {:?}", info.name, info.backend),
        })
    }
    pub fn streaming(mut self) -> Self {
        self.streaming = true;
        self
    }
    pub fn apply(&mut self, previous: Option<&[u8]>, current: &[u8]) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        self.apply_into(previous, current, &mut output)?;
        Ok(output)
    }
    pub fn apply_into(
        &mut self,
        previous: Option<&[u8]>,
        current: &[u8],
        output: &mut Vec<u8>,
    ) -> Result<(), String> {
        let Some(previous) = previous else {
            self.previous_ready = false;
            output.clear();
            output.extend_from_slice(current);
            return Ok(());
        };
        let s = &self.settings;
        let gw = self.width.min(s.analysis_width);
        let gh = (self.height * gw / self.width).max(1);
        let cut = s.protection
            && motion::is_scene_cut(previous, current, self.width, self.height, s.analysis_width);
        if s.shutter == 0 || cut {
            self.previous_ready = false;
            output.clear();
            output.extend_from_slice(current);
            return Ok(());
        }
        let pack = |frame: &[u8], packed: &mut [u32]| {
            packed
                .par_iter_mut()
                .zip(frame.par_chunks_exact(3))
                .for_each(|(out, v)| {
                    *out = u32::from(v[0]) | (u32::from(v[1]) << 8) | (u32::from(v[2]) << 16);
                });
        };
        if !self.streaming || !self.previous_ready {
            pack(previous, &mut self.packed);
            self.queue
                .write_buffer(&self.previous, 0, bytemuck::cast_slice(&self.packed));
        }
        pack(current, &mut self.packed);
        self.queue
            .write_buffer(&self.current, 0, bytemuck::cast_slice(&self.packed));
        let params = Params {
            w: self.width as u32,
            h: self.height as u32,
            gw: gw as u32,
            gh: gh as u32,
            fw: gw.div_ceil(8) as u32,
            fh: gh.div_ceil(8) as u32,
            radius: s.search_radius as u32,
            samples: s.samples as u32,
            shutter: s.shutter as f32,
            protect: u32::from(s.protection),
            cut: 0,
            padding: 0,
        };
        self.queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut commands = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = commands.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("motion field"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.motion);
            pass.set_bind_group(0, &self.group, &[]);
            pass.dispatch_workgroups((params.fw * params.fh).div_ceil(64), 1, 1);
        }
        {
            let mut pass = commands.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("exposure"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.integrate);
            pass.set_bind_group(0, &self.group, &[]);
            pass.dispatch_workgroups(params.w.div_ceil(16), params.h.div_ceil(16), 1);
        }
        commands.copy_buffer_to_buffer(
            &self.output,
            0,
            &self.readback,
            0,
            (self.width * self.height * 4) as u64,
        );
        if self.streaming {
            commands.copy_buffer_to_buffer(
                &self.current,
                0,
                &self.previous,
                0,
                (self.width * self.height * 4) as u64,
            );
        }
        self.queue.submit(Some(commands.finish()));
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        let scope = pollster::block_on(self.device.pop_error_scope());
        if let Some(error) = scope {
            self.readback.unmap();
            return Err(format!("Erro GPU: {error}"));
        }
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Leitura da GPU falhou: {e}"))?;
        let bytes = slice.get_mapped_range();
        output.resize(current.len(), 0);
        output
            .par_chunks_exact_mut(3)
            .zip(bytes.par_chunks_exact(4))
            .for_each(|(out, pixel)| out.copy_from_slice(&pixel[..3]));
        drop(bytes);
        self.readback.unmap();
        self.previous_ready = self.streaming;
        Ok(())
    }
}
pub fn availability() -> Result<String, String> {
    static STATUS: OnceLock<Result<String, String>> = OnceLock::new();
    STATUS
        .get_or_init(|| {
            let mut gpu = GpuMotion::new(
                96,
                64,
                &Settings {
                    protection: false,
                    ..Settings::default()
                },
            )?;
            let a = vec![0; 96 * 64 * 3];
            let b = vec![100; 96 * 64 * 3];
            gpu.apply(Some(&a), &b)?;
            Ok(gpu.name)
        })
        .clone()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires a physical GPU; run cargo test gpu::tests -- --ignored --nocapture"]
    fn gpu_matches_cpu_on_motion_static_and_cuts() {
        let settings = Settings {
            shutter: 360,
            samples: 15,
            ..Settings::default()
        };
        let mut gpu =
            GpuMotion::new(96, 64, &settings).expect("GPU compute test needs a hardware adapter");
        println!("GPU tested: {}", gpu.name);
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
        let cpu = motion::MotionEngine::configured(480, 16, "gaussian", 2.2, 0.65).apply(
            Some(&a),
            &b,
            96,
            64,
            360,
            15,
            true,
        );
        let output = gpu.apply(Some(&a), &b).unwrap();
        assert_ne!(output, b);
        assert_eq!(gpu.apply(Some(&a), &b).unwrap(), output); // buffer reuse and deterministic output

        let error = cpu
            .iter()
            .zip(&output)
            .map(|(a, b)| a.abs_diff(*b) as u32)
            .sum::<u32>() as f64
            / cpu.len() as f64;
        assert!(error < 2.0, "CPU/GPU mean difference {error}");
        assert_eq!(gpu.apply(Some(&b), &b).unwrap(), b);
        assert_eq!(gpu.apply(None, &b).unwrap(), b);
        for mode in ["uniform", "pyramid", "ascending", "descending"] {
            let configured = Settings {
                weighting: mode.into(),
                gamma: 1.0,
                protection: false,
                ..settings.clone()
            };
            let mut gpu = GpuMotion::new(96, 64, &configured).unwrap();
            let output = gpu.apply(Some(&a), &b).unwrap();
            let cpu = motion::MotionEngine::configured(480, 16, mode, 1.0, 0.65).apply(
                Some(&a),
                &b,
                96,
                64,
                360,
                15,
                false,
            );
            let error = cpu
                .iter()
                .zip(&output)
                .map(|(a, b)| a.abs_diff(*b) as u32)
                .sum::<u32>() as f64
                / cpu.len() as f64;
            assert!(error < 2.0, "{mode} CPU/GPU mean difference {error}");
        }
        let mut stream = GpuMotion::new(96, 64, &settings).unwrap().streaming();
        assert_eq!(stream.apply(None, &a).unwrap(), a);
        assert_eq!(stream.apply(Some(&a), &b).unwrap(), output);
        assert_eq!(
            stream.apply(Some(&b), &a).unwrap(),
            gpu.apply(Some(&b), &a).unwrap()
        );
        let white = vec![255; b.len()];
        assert_eq!(stream.apply(Some(&a), &white).unwrap(), white);
        assert_eq!(
            stream.apply(Some(&white), &b).unwrap(),
            gpu.apply(Some(&white), &b).unwrap()
        );
        assert_eq!(gpu.apply(Some(&vec![0; b.len()]), &white).unwrap(), white);
    }
}
