//! The AI upscale network on the graphics card (wgpu: DirectX 12 / Vulkan / Metal), so
//! AMD, NVIDIA and Intel cards all work without extra drivers or libraries.
//!
//! Safety first, every card and driver is different:
//! - software "cards" (WARP, llvmpipe) are skipped unless asked for (tests);
//! - `self_test` compares the card with the processor on a small image before use;
//! - every tile is checked for errors, device loss and a time limit, and runs in small
//!   pieces (one layer per submission) so Windows never resets the card;
//! - any problem makes `upscale_on` finish the remaining tiles on the processor.
//!
//! One compute shader does a 3 x 3 convolution (+ bias, + PReLU) for a block of 8 x 8
//! pixels: the input block (10 x 10 pixels, 16 channels at a time) goes to workgroup
//! memory, each invocation sums 16 output channels of one pixel.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use crate::upscale::{Model, PAD};

/// the convolution shader (WGSL); public for the translation test
pub const SHADER: &str = r#"
struct P { w: u32, h: u32, cin4: u32, cout4: u32, wofs: u32, bofs: u32, pofs: u32, prelu: u32 }
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> wt: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> p: P;

const CH: u32 = 4u; // input vec4s (16 channels) per step
var<workgroup> blk: array<vec4<f32>, 400>; // 10 x 10 pixels x CH

@compute @workgroup_size(8, 8, 4)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>,
        @builtin(local_invocation_index) li: u32) {
    let bx = i32(wg.x * 8u) - 1;
    let by = i32(wg.y * 8u) - 1;
    let x = wg.x * 8u + l.x;
    let y = wg.y * 8u + l.y;
    let g = l.z * 4u; // first output vec4 of this invocation
    let cin = p.cin4 * 4u;
    var a0 = vec4<f32>(0.0);
    var a1 = vec4<f32>(0.0);
    var a2 = vec4<f32>(0.0);
    var a3 = vec4<f32>(0.0);
    for (var c0 = 0u; c0 < p.cin4; c0 += CH) {
        let n = min(CH, p.cin4 - c0);
        workgroupBarrier();
        for (var i = li; i < 100u * CH; i += 256u) {
            let q = i / CH;
            let c = i % CH;
            let sx = bx + i32(q % 10u);
            let sy = by + i32(q / 10u);
            var v = vec4<f32>(0.0);
            if (c < n && sx >= 0 && sy >= 0 && sx < i32(p.w) && sy < i32(p.h)) {
                v = src[(u32(sy) * p.w + u32(sx)) * p.cin4 + c0 + c];
            }
            blk[i] = v;
        }
        workgroupBarrier();
        if (g < p.cout4) {
            for (var t = 0u; t < 9u; t++) {
                let base = ((l.y + t / 3u) * 10u + l.x + t % 3u) * CH;
                for (var c = 0u; c < n; c++) {
                    let v = blk[base + c];
                    var wi = p.wofs + (t * cin + (c0 + c) * 4u) * p.cout4 + g;
                    for (var k = 0u; k < 4u; k++) {
                        let s = v[k];
                        a0 += s * wt[wi];
                        a1 += s * wt[wi + 1u];
                        a2 += s * wt[wi + 2u];
                        a3 += s * wt[wi + 3u];
                        wi += p.cout4;
                    }
                }
            }
        }
    }
    if (g < p.cout4 && x < p.w && y < p.h) {
        let o = (y * p.w + x) * p.cout4 + g;
        var r = array<vec4<f32>, 4>(a0, a1, a2, a3);
        for (var k = 0u; k < 4u; k++) {
            var v = r[k] + wt[p.bofs + g + k];
            if (p.prelu != 0u) {
                v = select(v, v * wt[p.pofs + g + k], v < vec4<f32>(0.0));
            }
            dst[o + k] = v;
        }
    }
}
"#;

/// largest tile side (pixels incl. margin) and the feature buffer it needs
const MAX_SIDE: usize = 512;
/// a tile layer that takes longer than this counts as a hung card
const LAYER_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    name: String,
    /// input pixels per tile side (without margin)
    tile: AtomicUsize,
    /// tests: tiles left before a simulated failure
    fail_after: AtomicUsize,
    /// first error the driver reported (validation, out of memory, device lost)
    err: Arc<Mutex<Option<String>>>,
    /// one upscale at a time on the card
    busy: Mutex<()>,
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    pollster::block_on(f)
}

impl Gpu {
    /// the best graphics card (dedicated before built-in); `allow_software` also accepts
    /// software renderers (only for tests: they are slower than the processor path)
    pub fn open(allow_software: bool) -> Result<Gpu> {
        let backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let rank = |i: &wgpu::AdapterInfo| {
            let kind = match i.device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Other => 2,
                wgpu::DeviceType::Cpu => 3,
            };
            // on Windows DirectX 12 is always there and well tested; elsewhere Vulkan / Metal
            let api = match i.backend {
                wgpu::Backend::Dx12 if cfg!(windows) => 0,
                wgpu::Backend::Metal | wgpu::Backend::Vulkan => 1,
                _ => 2,
            };
            (kind, api)
        };
        let mut adapters: Vec<wgpu::Adapter> = block(instance.enumerate_adapters(backends))
            .into_iter()
            .filter(|a| allow_software || a.get_info().device_type != wgpu::DeviceType::Cpu)
            .collect();
        adapters.sort_by_key(|a| rank(&a.get_info()));
        let mut why = Vec::new();
        for a in adapters {
            match Gpu::with_adapter(a) {
                Ok(g) => return Ok(g),
                Err(e) => why.push(format!("{e:#}")),
            }
        }
        if why.is_empty() {
            bail!("tidak ada kartu grafis yang bisa dipakai");
        }
        bail!("kartu grafis tidak bisa dipakai: {}", why.join("; "))
    }

    fn with_adapter(adapter: wgpu::Adapter) -> Result<Gpu> {
        let info = adapter.get_info();
        let name = format!("{} ({:?})", info.name.trim(), info.backend);
        let have = adapter.limits();
        if have.max_compute_invocations_per_workgroup < 256
            || have.max_compute_workgroup_size_z < 4
            || have.max_compute_workgroup_storage_size < 6400
            || have.max_storage_buffers_per_shader_stage < 3
        {
            bail!("{name}: terlalu terbatas untuk AI");
        }
        // feature buffers: side^2 pixels x 64 channels x 4 bytes
        let limit = have.max_storage_buffer_binding_size.min(have.max_buffer_size);
        let mut side = MAX_SIDE;
        while side > 2 * PAD + 64 && (side * side * 256) as u64 > limit {
            side -= 32;
        }
        if (side * side * 256) as u64 > limit {
            bail!("{name}: memori kartu grafis terlalu kecil");
        }
        let (device, queue) = block(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("studiozea-ai"),
            required_limits: have.clone(),
            ..Default::default()
        }))
        .with_context(|| format!("{name}: tidak bisa dibuka"))?;
        let err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let e1 = err.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let mut s = e1.lock().unwrap_or_else(|p| p.into_inner());
            s.get_or_insert_with(|| e.to_string());
        }));
        let e2 = err.clone();
        device.set_device_lost_callback(move |_, msg| {
            let mut s = e2.lock().unwrap_or_else(|p| p.into_inner());
            s.get_or_insert_with(|| format!("kartu grafis terputus ({msg})"));
        });
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("conv3x3"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("conv3x3"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = block(scope.pop()) {
            bail!("{name}: shader ditolak ({e})");
        }
        Ok(Gpu { device, queue, pipeline, name, tile: AtomicUsize::new(side - 2 * PAD), fail_after: AtomicUsize::new(usize::MAX), err, busy: Mutex::new(()) })
    }

    /// card name and API, e.g. "NVIDIA GeForce RTX 3060 (Dx12)"
    pub fn name(&self) -> &str {
        &self.name
    }

    /// input pixels per tile side for `upscale_on`
    pub fn tile(&self) -> usize {
        self.tile.load(Ordering::Relaxed)
    }

    /// tests only: smaller tiles, and fail (like a lost card) after `fail_after` tiles
    #[doc(hidden)]
    pub fn testing(&self, tile: usize, fail_after: usize) {
        self.tile.store(tile.clamp(8, MAX_SIDE - 2 * PAD), Ordering::Relaxed);
        self.fail_after.store(fail_after, Ordering::Relaxed);
    }

    /// run a small synthetic image on the card and on the processor: the card is used only
    /// when both agree (at most 2 / 255 apart, tiny average difference)
    pub fn self_test(&self) -> Result<()> {
        let (w, h) = (45, 37);
        let rgb: Vec<f32> = (0..w * h * 3)
            .map(|i| {
                let (p, c) = (i / 3, i % 3);
                let (x, y) = ((p % w) as f32, (p / w) as f32);
                let v = 0.5 + 0.4 * ((x * (0.3 + c as f32 * 0.2)).sin() * (y * 0.25).cos());
                if (x - 20.0).powi(2) + (y - 18.0).powi(2) < 64.0 { 0.95 - 0.3 * c as f32 } else { v }
            })
            .collect();
        let model = Model::new(0.5);
        let keep = [0, 0, w, h];
        let cpu = model.forward(&rgb, w, h, keep);
        let feat = self.features(&model, &rgb, w, h)?;
        let gpu = crate::upscale::shuffle(&feat, |x, y| y * w + x, &rgb, w, keep);
        let (mut max, mut sum) = (0.0f32, 0.0f64);
        for (a, b) in cpu.iter().zip(&gpu) {
            let d = (a.clamp(0.0, 1.0) - b.clamp(0.0, 1.0)).abs();
            if !d.is_finite() {
                bail!("{}: hasil tes tidak valid", self.name);
            }
            max = max.max(d);
            sum += d as f64;
        }
        let mean = sum / cpu.len() as f64;
        if max > 2.0 / 255.0 || mean > 0.05 / 255.0 {
            bail!("{}: hasil tes berbeda dari prosesor (maks. {:.1}/255)", self.name, max * 255.0);
        }
        Ok(())
    }

    fn check(&self) -> Result<()> {
        match self.err.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            Some(e) => Err(anyhow!("{e}")),
            None => Ok(()),
        }
    }

    /// the network's last layer (48 channels per pixel, pixel-major) for a w x h tile
    /// with zero padding at its edge; same numbers as `Model::forward` before the shuffle
    pub(crate) fn features(&self, net: &Model, rgb: &[f32], w: usize, h: usize) -> Result<Vec<f32>> {
        let _one = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        self.check()?;
        if self.fail_after.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)).is_err() {
            bail!("kartu grafis terputus (uji)");
        }
        if w * h > MAX_SIDE * MAX_SIDE || w == 0 || h == 0 {
            bail!("potongan gambar terlalu besar untuk kartu grafis");
        }
        let d = &self.device;
        let scope = d.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = d.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let r = self.run(net, rgb, w, h);
        let e_oom = block(oom.pop());
        let e_val = block(scope.pop());
        self.check()?;
        if let Some(e) = e_oom {
            bail!("memori kartu grafis penuh ({e})");
        }
        if let Some(e) = e_val {
            bail!("kartu grafis menolak perintah ({e})");
        }
        r
    }

    fn run(&self, net: &Model, rgb: &[f32], w: usize, h: usize) -> Result<Vec<f32>> {
        use wgpu::util::DeviceExt;
        let d = &self.device;
        // weights of all layers as vec4 of output channels: [tap][cin (padded to 4)][cout / 4]
        let mut wt: Vec<f32> = Vec::new();
        let mut params: Vec<[u32; 8]> = Vec::new();
        for (i, (cin, cout, raw, bias)) in net.raw.iter().enumerate() {
            let (cin4, cout4) = (cin.div_ceil(4), cout / 4);
            let wofs = wt.len() / 4;
            let mut lw = vec![0.0f32; 9 * cin4 * 4 * cout];
            for co in 0..*cout {
                for ci in 0..*cin {
                    for t in 0..9 {
                        lw[(t * cin4 * 4 + ci) * cout + co] = raw[(co * cin + ci) * 9 + t];
                    }
                }
            }
            wt.extend_from_slice(&lw);
            let bofs = wt.len() / 4;
            wt.extend_from_slice(bias);
            let (pofs, prelu) = match net.prelu(i) {
                Some(p) => {
                    let o = wt.len() / 4;
                    wt.extend_from_slice(p);
                    (o, 1)
                }
                None => (0, 0),
            };
            params.push([w as u32, h as u32, cin4 as u32, cout4 as u32, wofs as u32, bofs as u32, pofs as u32, prelu]);
        }
        let bytes = |v: &[f32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let weights = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("weights"), contents: &bytes(&wt), usage: storage });
        let mut input = vec![0.0f32; w * h * 4];
        for (p, q) in input.chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
            p[..3].copy_from_slice(q);
        }
        let input = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("input"), contents: &bytes(&input), usage: storage });
        let feat_size = (w * h * 64 * 4) as u64;
        let fa = d.create_buffer(&wgpu::BufferDescriptor { label: Some("fa"), size: feat_size, usage: storage, mapped_at_creation: false });
        let fb = d.create_buffer(&wgpu::BufferDescriptor { label: Some("fb"), size: feat_size, usage: storage, mapped_at_creation: false });
        let out_size = (w * h * 48 * 4) as u64;
        let read = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = self.pipeline.get_bind_group_layout(0);
        let n = params.len();
        for (i, p) in params.iter().enumerate() {
            let (src, dst) = match i {
                0 => (&input, &fa),
                _ if i % 2 == 1 => (&fa, &fb),
                _ => (&fb, &fa),
            };
            let pb: Vec<u8> = p.iter().flat_map(|v| v.to_le_bytes()).collect();
            let ub = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("params"), contents: &pb, usage: wgpu::BufferUsages::UNIFORM });
            let bg = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: dst.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: weights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: ub.as_entire_binding() },
                ],
            });
            let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(w.div_ceil(8) as u32, h.div_ceil(8) as u32, 1);
            }
            if i + 1 == n {
                enc.copy_buffer_to_buffer(dst, 0, &read, 0, out_size);
            }
            // one layer per submission, waited for: short GPU jobs (no Windows reset) and
            // a stuck card is noticed
            self.queue.submit([enc.finish()]);
            d.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(LAYER_TIMEOUT) })
                .map_err(|e| anyhow!("kartu grafis tidak merespons ({e})"))?;
            self.check()?;
        }
        let slice = read.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(LAYER_TIMEOUT) })
            .map_err(|e| anyhow!("kartu grafis tidak merespons ({e})"))?;
        rx.recv_timeout(LAYER_TIMEOUT)
            .map_err(|_| anyhow!("hasil kartu grafis tidak terbaca"))?
            .map_err(|e| anyhow!("hasil kartu grafis tidak terbaca ({e})"))?;
        let out = {
            let view = slice.get_mapped_range().map_err(|e| anyhow!("hasil kartu grafis tidak terbaca ({e})"))?;
            view.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<f32>>()
        };
        read.unmap();
        if out.iter().any(|v| !v.is_finite()) {
            bail!("hasil kartu grafis tidak valid");
        }
        Ok(out)
    }
}
