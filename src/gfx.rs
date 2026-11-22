//! wgpu renderer. Draws the 3D scene offscreen (sun shadow map, 4× MSAA) inside egui's
//! paint callback, then copies it to the screen under the egui UI.

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Inst {
    pub m: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub params: [f32; 4], // kind, emissive, casts shadow, extra
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct Globals {
    pub view_proj: [[f32; 4]; 4],
    pub light_vp: [[f32; 4]; 4],
    pub cam_pos: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_col: [f32; 4],
    pub sky_top: [f32; 4],
    pub sky_hor: [f32; 4],
    pub ground_col: [f32; 4],
    pub fog: [f32; 4],
    pub misc: [f32; 4],
    pub grade: [f32; 4],
    /// Up to six lamps: position + range, colour (w unused).
    pub lp: [[f32; 4]; NLIGHTS],
    pub lc: [[f32; 4]; NLIGHTS],
    /// Up to two rooms (min, max corners; max.w = 1 when used) for corner darkening.
    pub rooms: [[f32; 4]; 4],
}
pub const NLIGHTS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Mesh {
    Cube,
    Sphere,
    Hemi,
    Cyl,
    Cone,
    Plane,
    Quad,
    Fan,
    RingThin,
    RingThick,
    Disc,
    Trunk,
    Leaves,
    Grass,
    /// the same with far fewer blades, for what's further from the pitch
    GrassFar,
}
const MESH_COUNT: usize = 15;

pub mod kind {
    pub const LIT: f32 = 0.0;
    pub const UNLIT: f32 = 1.0;
    pub const SOFT: f32 = 2.0;
    pub const GRASS: f32 = 3.0;
    pub const GAZE: f32 = 4.0;
    pub const SKY: f32 = 5.0;
    pub const WATER: f32 = 6.0;
    pub const BALL: f32 = 7.0;
    pub const BARK: f32 = 13.0;
    pub const LEAF: f32 = 14.0;
    pub const BLADE: f32 = 15.0;
    pub const PLASTER: f32 = 16.0;
    pub const ASPHALT: f32 = 17.0;
    pub const BEAM: f32 = 18.0;
}

/// One skinned person: joint matrices (world), material colours, referee stripes.
#[derive(Clone)]
pub struct SkinDraw {
    pub bones: Vec<Mat4>,
    pub colors: [[f32; 4]; 6],
    pub stripe: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkinUniform {
    bones: [[[f32; 4]; 4]; MAX_BONES],
    colors: [[f32; 4]; 6],
    misc: [f32; 4],
}
pub const MAX_BONES: usize = 32;
const SKIN_STRIDE: u64 = 2304; // SkinUniform (2160 bytes) rounded up to 256
const MAX_SKINS: usize = 24;

#[derive(Default, Clone)]
pub struct DrawList {
    pub opaque: Vec<(Mesh, Inst)>,
    pub blend: Vec<(Mesh, Inst)>,
    pub add: Vec<(Mesh, Inst)>,
    pub skinned: Vec<SkinDraw>,
}
impl DrawList {
    pub fn clear(&mut self) {
        self.opaque.clear();
        self.blend.clear();
        self.add.clear();
        self.skinned.clear();
    }
    /// A lit, shadow-casting part.
    pub fn solid(&mut self, mesh: Mesh, m: Mat4, rgb: [f32; 3]) {
        self.opaque.push((mesh, inst(m, [rgb[0], rgb[1], rgb[2], 1.0], [kind::LIT, 0.0, 1.0, 0.0])));
    }
    pub fn push(&mut self, mesh: Mesh, m: Mat4, color: [f32; 4], params: [f32; 4]) {
        self.opaque.push((mesh, inst(m, color, params)));
    }
}
pub fn inst(m: Mat4, color: [f32; 4], params: [f32; 4]) -> Inst {
    Inst { m: m.to_cols_array_2d(), color, params }
}

pub struct FrameData {
    pub globals: Globals,
    pub list: DrawList,
    pub clear: [f32; 3],
    /// Fraction of the normal render resolution (drops on slow phones to keep the frame rate).
    pub scale: f32,
}

// ---------- meshes ----------
struct MeshBuf {
    v: Vec<Vertex>,
    i: Vec<u32>,
    ranges: [(u32, u32, i32); MESH_COUNT], // first index, count, base vertex
}
impl MeshBuf {
    fn add(&mut self, id: Mesh, v: Vec<Vertex>, i: Vec<u32>) {
        let base = self.v.len() as i32;
        let first = self.i.len() as u32;
        // indices are made global here: WebGL2 has no base-vertex draws
        self.ranges[id as usize] = (first, i.len() as u32, 0);
        self.v.extend(v);
        self.i.extend(i.into_iter().map(|x| x + base as u32));
    }
}
fn vx(p: Vec3, n: Vec3, u: f32, v: f32) -> Vertex {
    Vertex { pos: p.to_array(), nrm: n.to_array(), uv: [u, v] }
}
fn sphere(seg: u32, rings: u32, top_only: bool) -> (Vec<Vertex>, Vec<u32>) {
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    let rmax = if top_only { rings / 2 } else { rings };
    for r in 0..=rmax {
        let th = r as f32 / rings as f32 * std::f32::consts::PI;
        for s in 0..=seg {
            let ph = s as f32 / seg as f32 * std::f32::consts::TAU;
            let n = Vec3::new(th.sin() * ph.sin(), th.cos(), th.sin() * ph.cos());
            v.push(vx(n, n, s as f32 / seg as f32, r as f32 / rings as f32));
        }
    }
    for r in 0..rmax {
        for s in 0..seg {
            let a = r * (seg + 1) + s;
            let b = a + seg + 1;
            ix.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    if top_only {
        // close the bottom
        let c = v.len() as u32;
        v.push(vx(Vec3::ZERO, Vec3::NEG_Y, 0.5, 0.5));
        let start = rmax * (seg + 1);
        for s in 0..seg {
            ix.extend([c, start + s + 1, start + s]);
        }
    }
    (v, ix)
}
/// Unit cylinder or cone: radius 1 (bottom) → `top` (top), height 1, centred on the origin.
fn cylinder(seg: u32, top: f32) -> (Vec<Vertex>, Vec<u32>) {
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    let slope = 1.0 - top;
    for s in 0..=seg {
        let ph = s as f32 / seg as f32 * std::f32::consts::TAU;
        let (sx, cz) = (ph.sin(), ph.cos());
        let n = Vec3::new(sx, slope, cz).normalize();
        v.push(vx(Vec3::new(sx, -0.5, cz), n, s as f32 / seg as f32, 0.0));
        v.push(vx(Vec3::new(sx * top, 0.5, cz * top), n, s as f32 / seg as f32, 1.0));
    }
    for s in 0..seg {
        let a = s * 2;
        ix.extend([a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    for (y, r, n) in [(-0.5f32, 1.0f32, Vec3::NEG_Y), (0.5, top, Vec3::Y)] {
        if r <= 0.0 {
            continue;
        }
        let c = v.len() as u32;
        v.push(vx(Vec3::new(0.0, y, 0.0), n, 0.5, 0.5));
        for s in 0..=seg {
            let ph = s as f32 / seg as f32 * std::f32::consts::TAU;
            v.push(vx(Vec3::new(ph.sin() * r, y, ph.cos() * r), n, 0.5 + ph.sin() * 0.5, 0.5 + ph.cos() * 0.5));
        }
        for s in 0..seg {
            if y < 0.0 {
                ix.extend([c, c + 2 + s, c + 1 + s]);
            } else {
                ix.extend([c, c + 1 + s, c + 2 + s]);
            }
        }
    }
    (v, ix)
}
fn cube() -> (Vec<Vertex>, Vec<u32>) {
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    let faces = [(Vec3::X, Vec3::Y), (Vec3::NEG_X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::NEG_Y, Vec3::Z), (Vec3::Z, Vec3::Y), (Vec3::NEG_Z, Vec3::Y)];
    for (n, up) in faces {
        let r = up.cross(n);
        let c = v.len() as u32;
        for (a, b) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            v.push(vx((n + r * a + up * b) * 0.5, n, a * 0.5 + 0.5, b * 0.5 + 0.5));
        }
        ix.extend([c, c + 1, c + 2, c, c + 2, c + 3]);
    }
    (v, ix)
}
fn ring(inner: f32, outer: f32, seg: u32) -> (Vec<Vertex>, Vec<u32>) {
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    for s in 0..=seg {
        let ph = s as f32 / seg as f32 * std::f32::consts::TAU;
        let (x, z) = (ph.sin(), ph.cos());
        v.push(vx(Vec3::new(x * inner, 0.0, z * inner), Vec3::Y, s as f32 / seg as f32, 0.0));
        v.push(vx(Vec3::new(x * outer, 0.0, z * outer), Vec3::Y, s as f32 / seg as f32, 1.0));
    }
    for s in 0..seg {
        let a = s * 2;
        ix.extend([a, a + 1, a + 2, a + 2, a + 1, a + 3]);
    }
    (v, ix)
}
/// The referee's gaze fan: u across the angle, v outward.
fn fan() -> (Vec<Vertex>, Vec<u32>) {
    let (n, m) = (28u32, 10u32);
    let (half, r0, r1) = (crate::game::GAZE_HALF, 0.35f32, 8.8f32);
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    for i in 0..=n {
        let u = i as f32 / n as f32;
        let ang = -half + 2.0 * half * u;
        for j in 0..=m {
            let t = j as f32 / m as f32;
            let r = r0 + (r1 - r0) * t;
            v.push(vx(Vec3::new(ang.sin() * r, 0.0, ang.cos() * r), Vec3::Y, u, t));
        }
    }
    for i in 0..n {
        for j in 0..m {
            let k = i * (m + 1) + j;
            ix.extend([k, k + m + 1, k + 1, k + 1, k + m + 1, k + m + 2]);
        }
    }
    (v, ix)
}
/// Tiny deterministic random numbers for baking meshes.
struct Bake(u32);
impl Bake {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn dir(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0);
            let l = v.length_squared();
            if l > 0.01 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }
}
/// A clump of leaves filling a unit sphere: each leaf is a card the shader cuts into a leaf shape.
/// uv.x = leaf number + across (the fraction), uv.y = along the leaf. Normals point out of the
/// clump, so the canopy shades like a soft volume rather than a pile of flat cards.
fn leaf_cluster(n: u32) -> (Vec<Vertex>, Vec<u32>) {
    let mut r = Bake(0x9e3779b9);
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    for k in 0..n {
        let d = r.dir();
        let c = d * (0.3 + 0.7 * r.f().sqrt());
        let face = (d * 0.9 + r.dir() * 0.7).normalize();
        let side = face.cross(r.dir()).normalize();
        let up = side.cross(face);
        let (w, l) = (0.07 + r.f() * 0.04, 0.16 + r.f() * 0.08);
        let nrm = (d * 0.75 + face * 0.25).normalize();
        let base = v.len() as u32;
        let kf = k as f32;
        for (a, b) in [(-1.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = c + side * (a * w) + up * ((b - 0.35) * l);
            v.push(vx(p, nrm, kf + (a * 0.5 + 0.5) * 0.999, b));
        }
        ix.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (v, ix)
}
/// A 4 m × 4 m patch of grass blades. uv.x = a random number per blade, uv.y = height along it.
fn grass_patch(n: u32) -> (Vec<Vertex>, Vec<u32>) {
    let mut r = Bake(0x51ed270b);
    let (mut v, mut ix) = (Vec::new(), Vec::new());
    for _ in 0..n {
        let (x, z) = (r.f() * 4.0 - 2.0, r.f() * 4.0 - 2.0);
        let yaw = r.f() * std::f32::consts::TAU;
        let (side, fwd) = (Vec3::new(yaw.cos(), 0.0, yaw.sin()), Vec3::new(-yaw.sin(), 0.0, yaw.cos()));
        let h = 0.055 + r.f() * 0.085;
        let w = 0.007 + r.f() * 0.006;
        let bend = (r.f() - 0.3) * 0.5;
        let rnd = r.f();
        let nrm = (fwd * 0.5 + Vec3::Y).normalize();
        let base = v.len() as u32;
        let root = Vec3::new(x, 0.0, z);
        for (k, (t, ww)) in [(0.0f32, 1.0f32), (0.5, 0.75)].iter().enumerate() {
            let c = root + Vec3::Y * (h * t) + fwd * (bend * h * t * t);
            v.push(vx(c - side * (w * ww), nrm, rnd, *t));
            v.push(vx(c + side * (w * ww), nrm, rnd, *t));
            let _ = k;
        }
        v.push(vx(root + Vec3::Y * h + fwd * (bend * h), nrm, rnd, 1.0));
        ix.extend([base, base + 1, base + 3, base, base + 3, base + 2, base + 2, base + 3, base + 4]);
    }
    (v, ix)
}
fn build_meshes() -> MeshBuf {
    let mut b = MeshBuf { v: Vec::new(), i: Vec::new(), ranges: [(0, 0, 0); MESH_COUNT] };
    let (v, i) = cube();
    b.add(Mesh::Cube, v, i);
    let (v, i) = sphere(14, 10, false);
    b.add(Mesh::Sphere, v, i);
    let (v, i) = sphere(14, 10, true);
    b.add(Mesh::Hemi, v, i);
    let (v, i) = cylinder(14, 1.0);
    b.add(Mesh::Cyl, v, i);
    let (v, i) = cylinder(12, 0.0);
    b.add(Mesh::Cone, v, i);
    let pl = vec![vx(Vec3::new(-0.5, 0.0, -0.5), Vec3::Y, 0.0, 0.0), vx(Vec3::new(0.5, 0.0, -0.5), Vec3::Y, 1.0, 0.0), vx(Vec3::new(0.5, 0.0, 0.5), Vec3::Y, 1.0, 1.0), vx(Vec3::new(-0.5, 0.0, 0.5), Vec3::Y, 0.0, 1.0)];
    b.add(Mesh::Plane, pl, vec![0, 2, 1, 0, 3, 2]);
    let q = vec![vx(Vec3::new(-0.5, -0.5, 0.0), Vec3::Z, 0.0, 1.0), vx(Vec3::new(0.5, -0.5, 0.0), Vec3::Z, 1.0, 1.0), vx(Vec3::new(0.5, 0.5, 0.0), Vec3::Z, 1.0, 0.0), vx(Vec3::new(-0.5, 0.5, 0.0), Vec3::Z, 0.0, 0.0)];
    b.add(Mesh::Quad, q, vec![0, 1, 2, 0, 2, 3]);
    let (v, i) = fan();
    b.add(Mesh::Fan, v, i);
    let (v, i) = ring(0.97, 1.02, 64);
    b.add(Mesh::RingThin, v, i);
    let (v, i) = ring(0.9, 1.0, 64);
    b.add(Mesh::RingThick, v, i);
    let (v, i) = ring(0.0, 1.0, 32);
    b.add(Mesh::Disc, v, i);
    let (v, i) = cylinder(10, 0.6);
    b.add(Mesh::Trunk, v, i);
    let (v, i) = leaf_cluster(150);
    b.add(Mesh::Leaves, v, i);
    let (v, i) = grass_patch(1800);
    b.add(Mesh::Grass, v, i);
    let (v, i) = grass_patch(350);
    b.add(Mesh::GrassFar, v, i);
    b
}

// ---------- GPU state ----------
const MSAA_WANTED: u32 = 4;
const SHADOW: u32 = 2048;
const COLOR_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// What the scene is really rendered into, where the device can: light brighter than white survives.
const HDR_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const BLOOM_LEVELS: usize = 5;
const DEPTH_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Gfx {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    ranges: [(u32, u32, i32); MESH_COUNT],
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    globals_buf: wgpu::Buffer,
    shadow_globals_buf: wgpu::Buffer,
    scene_bg: wgpu::BindGroup,
    shadow_bg: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    p_opaque: wgpu::RenderPipeline,
    p_blend: wgpu::RenderPipeline,
    p_add: wgpu::RenderPipeline,
    p_shadow: wgpu::RenderPipeline,
    p_blit: wgpu::RenderPipeline,
    p_bright: wgpu::RenderPipeline,
    p_down: wgpu::RenderPipeline,
    p_up: wgpu::RenderPipeline,
    bloom_bgl: wgpu::BindGroupLayout,
    hdr: bool,
    color_fmt: wgpu::TextureFormat,
    encode: f32,
    blit_bgl: wgpu::BindGroupLayout,
    blit_smp: wgpu::Sampler,
    blit_cfg: wgpu::Buffer,
    msaa: u32,
    targets: Option<Targets>,
    skin_v: wgpu::Buffer,
    skin_i: wgpu::Buffer,
    skin_n: u32,
    skin_buf: wgpu::Buffer,
    skin_bg: wgpu::BindGroup,
    p_skin: wgpu::RenderPipeline,
    p_skin_shadow: wgpu::RenderPipeline,
}
struct Targets {
    size: [u32; 2],
    color_ms: Option<wgpu::TextureView>,
    resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
    blit_bg: wgpu::BindGroup,
    /// the chain, biggest first: its view, and the bind group to read it
    bloom: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
    resolved_bg: Option<wgpu::BindGroup>,
    /// every texture above, to free at once when the size changes (a browser would wait for its
    /// garbage collector, and a few resizes in a row run the GPU out of memory)
    texs: Vec<wgpu::Texture>,
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VA: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    const IA: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
    [
        wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &VA },
        wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Inst>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &IA },
    ]
}

impl Gfx {
    pub fn new(rs: &egui_wgpu::RenderState, model: &crate::model::Model) -> Self {
        let device = &rs.device;
        let hf = rs.adapter.get_texture_format_features(HDR_FMT);
        let hdr = hf.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING) && hf.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE) && hf.flags.contains(wgpu::TextureFormatFeatureFlags::BLENDABLE);
        let color_fmt = if hdr { HDR_FMT } else { COLOR_FMT };
        let fmt_ok = rs.adapter.get_texture_format_features(color_fmt).flags.sample_count_supported(MSAA_WANTED);
        let msaa = if fmt_ok { MSAA_WANTED } else { 1 };
        // phones without Vulkan: the emulator's GLES draws nothing at all with multisampled targets, and such phones can't spare the time anyway
        let msaa = if rs.adapter.get_info().backend == wgpu::Backend::Gl && cfg!(target_os = "android") { 1 } else { msaa };
        let mb = build_meshes();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("verts"), contents: bytemuck::cast_slice(&mb.v), usage: wgpu::BufferUsages::VERTEX });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("idx"), contents: bytemuck::cast_slice(&mb.i), usage: wgpu::BufferUsages::INDEX });
        let inst_cap = 4096;
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("inst"), size: (inst_cap * std::mem::size_of::<Inst>()) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let gsize = std::mem::size_of::<Globals>() as u64;
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: gsize, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let shadow_globals_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sglobals"), size: gsize, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });

        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow"),
            size: wgpu::Extent3d { width: SHADOW, height: SHADOW, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_smp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow smp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let uni = |b: u32, vis| wgpu::BindGroupLayoutEntry { binding: b, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene bgl"),
            entries: &[
                uni(0, vf),
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
            ],
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow bgl"), entries: &[uni(0, vf)] });
        let scene_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene bg"),
            layout: &scene_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_smp) },
            ],
        });
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("shadow bg"), layout: &shadow_bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_globals_buf.as_entire_binding() }] });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("scene"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let blit_sh = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()) });
        let scene_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("scene pl"), bind_group_layouts: &[Some(&scene_bgl)], immediate_size: 0 });
        let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadow pl"), bind_group_layouts: &[Some(&shadow_bgl)], immediate_size: 0 });
        let layouts = vertex_layouts();
        let bufs = [Some(layouts[0].clone()), Some(layouts[1].clone())];
        let prim = wgpu::PrimitiveState { cull_mode: None, ..Default::default() };
        let mk = |label: &str, blend: Option<wgpu::BlendState>, write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&scene_pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &bufs },
                primitive: prim,
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FMT, depth_write_enabled: Some(write), depth_compare: Some(wgpu::CompareFunction::GreaterEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: wgpu::MultisampleState { count: msaa, ..Default::default() },
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: color_fmt, blend, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let p_opaque = mk("opaque", None, true);
        let p_blend = mk("blend", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let p_add = mk("add", Some(additive), false);
        let p_shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &bufs },
            primitive: prim,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FMT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_shadow"), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        });
        // ---- skinned people ----
        let skin_v = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("skin v"), contents: bytemuck::cast_slice(&model.verts), usage: wgpu::BufferUsages::VERTEX });
        let skin_i = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("skin i"), contents: bytemuck::cast_slice(&model.indices), usage: wgpu::BufferUsages::INDEX });
        let skin_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("skin u"), size: SKIN_STRIDE * MAX_SKINS as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let skin_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin bgl"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<SkinUniform>() as u64) }, count: None }],
        });
        let skin_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skin bg"),
            layout: &skin_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &skin_buf, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<SkinUniform>() as u64) }) }],
        });
        const SA: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32];
        let skin_layout = [Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::model::SkinVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &SA })];
        let skin_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("skin pl"), bind_group_layouts: &[Some(&scene_bgl), Some(&skin_bgl)], immediate_size: 0 });
        let skin_shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("skin shadow pl"), bind_group_layouts: &[Some(&shadow_bgl), Some(&skin_bgl)], immediate_size: 0 });
        let p_skin = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin"),
            layout: Some(&skin_pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_skin"), compilation_options: Default::default(), buffers: &skin_layout },
            primitive: prim,
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FMT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::GreaterEqual), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState { count: msaa, ..Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: color_fmt, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        });
        let p_skin_shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin shadow"),
            layout: Some(&skin_shadow_pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_skin_shadow"), compilation_options: Default::default(), buffers: &skin_layout },
            primitive: prim,
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FMT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 } }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                uni(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let bloom_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let bloom_sh = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("bloom"), source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()) });
        let bloom_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("bloom pl"), bind_group_layouts: &[Some(&bloom_bgl)], immediate_size: 0 });
        let bloom_p = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bloom"),
                layout: Some(&bloom_pl),
                vertex: wgpu::VertexState { module: &bloom_sh, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &bloom_sh, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: if hdr { HDR_FMT } else { COLOR_FMT }, blend, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let (p_bright, p_down, p_up) = (bloom_p("fs_bright", None), bloom_p("fs_down", None), bloom_p("fs_up", Some(wgpu::BlendState { color: add, alpha: add })));
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("blit pl"), bind_group_layouts: &[Some(&blit_bgl)], immediate_size: 0 });
        let p_blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_pl),
            vertex: wgpu::VertexState { module: &blit_sh, entry_point: Some("vs_blit"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &blit_sh, entry_point: Some("fs_blit"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: rs.target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        });
        let blit_smp = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("blit smp"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let encode = if rs.target_format.is_srgb() { 0.0f32 } else { 1.0 };
        let blit_cfg = device.create_buffer(&wgpu::BufferDescriptor { label: Some("blit cfg"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        log::info!("renderer ready: {:?}, msaa {msaa}, hdr {hdr}, target {:?}", rs.adapter.get_info().backend, rs.target_format);
        Gfx { vbuf, ibuf, ranges: mb.ranges, inst_buf, inst_cap, globals_buf, shadow_globals_buf, scene_bg, shadow_bg, shadow_view, p_opaque, p_blend, p_add, p_shadow, p_blit, p_bright, p_down, p_up, bloom_bgl, hdr, color_fmt, encode, blit_bgl, blit_smp, blit_cfg, msaa, targets: None, skin_n: model.indices.len() as u32, skin_v, skin_i, skin_buf, skin_bg, p_skin, p_skin_shadow }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if self.targets.as_ref().map_or(false, |t| t.size == size) {
            return;
        }
        if let Some(old) = self.targets.take() {
            for t in old.texs {
                t.destroy();
            }
        }
        let ext = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texs = std::cell::RefCell::new(Vec::new());
        let tex = |label, fmt, samples, usage| {
            let t = device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: ext, mip_level_count: 1, sample_count: samples, dimension: wgpu::TextureDimension::D2, format: fmt, usage, view_formats: &[] });
            let v = t.create_view(&Default::default());
            texs.borrow_mut().push(t);
            v
        };
        let color_ms = (self.msaa > 1).then(|| tex("color ms", self.color_fmt, self.msaa, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let resolved = tex("resolved", self.color_fmt, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = tex("depth", DEPTH_FMT, self.msaa, wgpu::TextureUsages::RENDER_ATTACHMENT);
        // the bloom chain starts at half size and halves from there
        let read = |v: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom bg"),
                layout: &self.bloom_bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.blit_smp) }],
            })
        };
        let mut bloom = Vec::new();
        if self.hdr {
            let (mut w, mut h) = (size[0], size[1]);
            for _ in 0..BLOOM_LEVELS {
                w = (w / 2).max(1);
                h = (h / 2).max(1);
                let t = device.create_texture(&wgpu::TextureDescriptor { label: Some("bloom"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: HDR_FMT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
                let v = t.create_view(&Default::default());
                texs.borrow_mut().push(t);
                let bg = read(&v);
                bloom.push((v, bg));
            }
        }
        let resolved_bg = self.hdr.then(|| read(&resolved));
        let bloom_view = bloom.first().map_or(&resolved, |b| &b.0);
        let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bg"),
            layout: &self.blit_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolved) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.blit_smp) },
                wgpu::BindGroupEntry { binding: 2, resource: self.blit_cfg.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(bloom_view) },
            ],
        });
        let texs = texs.into_inner();
        self.targets = Some(Targets { size, color_ms, resolved, depth, blit_bg, bloom, resolved_bg, texs });
    }

    fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, size: [u32; 2], f: &FrameData) {
        self.ensure_targets(device, size);
        // order instances: shadow casters together, then everything else, each grouped by mesh
        let mut opaque: Vec<&(Mesh, Inst)> = f.list.opaque.iter().collect();
        opaque.sort_by_key(|(m, i)| (i.params[2] < 0.5, *m));
        let mut blend: Vec<&(Mesh, Inst)> = f.list.blend.iter().collect();
        let cam = Vec3::from_slice(&f.globals.cam_pos[..3]);
        blend.sort_by(|a, b| {
            let da = Vec3::new(a.1.m[3][0], a.1.m[3][1], a.1.m[3][2]).distance_squared(cam);
            let db = Vec3::new(b.1.m[3][0], b.1.m[3][1], b.1.m[3][2]).distance_squared(cam);
            db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut all: Vec<Inst> = Vec::with_capacity(opaque.len() + blend.len() + f.list.add.len());
        let mut runs: Vec<(u8, Mesh, u32, u32)> = Vec::new(); // pass(0 opaque caster,1 opaque,2 blend,3 add), mesh, start, count
        let mut push = |pass: u8, m: Mesh, i: &Inst, all: &mut Vec<Inst>, runs: &mut Vec<(u8, Mesh, u32, u32)>| {
            let idx = all.len() as u32;
            all.push(*i);
            if let Some(last) = runs.last_mut() {
                if last.0 == pass && last.1 == m && last.2 + last.3 == idx {
                    last.3 += 1;
                    return;
                }
            }
            runs.push((pass, m, idx, 1));
        };
        for (m, i) in opaque {
            push(if i.params[2] >= 0.5 { 0 } else { 1 }, *m, i, &mut all, &mut runs);
        }
        for (m, i) in blend {
            push(2, *m, i, &mut all, &mut runs);
        }
        for (m, i) in &f.list.add {
            push(3, *m, i, &mut all, &mut runs);
        }
        if all.len() > self.inst_cap {
            self.inst_cap = all.len().next_power_of_two();
            self.inst_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("inst"), size: (self.inst_cap * std::mem::size_of::<Inst>()) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        }
        if !all.is_empty() {
            queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&all));
        }
        // cam_pos.w tells the scene shader whether the film curve comes later, in the final copy
        let mut gl = f.globals;
        gl.cam_pos[3] = if self.hdr { 2.0 } else { 1.0 };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&gl));
        let b = gl.misc[2];
        queue.write_buffer(&self.blit_cfg, 0, bytemuck::cast_slice(&[self.encode, if self.hdr { 1.0 } else { 0.0 }, 0.05, 0.0, b, gl.grade[0], gl.grade[1], gl.grade[2]]));
        let nskin = f.list.skinned.len().min(MAX_SKINS);
        for (k, sd) in f.list.skinned.iter().take(nskin).enumerate() {
            let mut u = SkinUniform { bones: [[[0.0; 4]; 4]; MAX_BONES], colors: sd.colors, misc: [sd.stripe, 0.0, 0.0, 0.0] };
            for (b, m) in sd.bones.iter().take(MAX_BONES).enumerate() {
                u.bones[b] = m.to_cols_array_2d();
            }
            queue.write_buffer(&self.skin_buf, k as u64 * SKIN_STRIDE, bytemuck::bytes_of(&u));
        }
        queue.write_buffer(&self.shadow_globals_buf, 0, bytemuck::bytes_of(&f.globals));

        let draw = |rp: &mut wgpu::RenderPass<'_>, ranges: &[(u32, u32, i32); MESH_COUNT], pass: u8| {
            for r in runs.iter().filter(|r| r.0 == pass) {
                let (first, count, base) = ranges[r.1 as usize];
                rp.draw_indexed(first..first + count, base, r.2..r.2 + r.3);
            }
        };
        // 1) sun shadow map
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.shadow_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.p_shadow);
            rp.set_bind_group(0, &self.shadow_bg, &[]);
            rp.set_vertex_buffer(0, self.vbuf.slice(..));
            rp.set_vertex_buffer(1, self.inst_buf.slice(..));
            rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            draw(&mut rp, &self.ranges, 0);
            rp.set_pipeline(&self.p_skin_shadow);
            rp.set_vertex_buffer(0, self.skin_v.slice(..));
            rp.set_index_buffer(self.skin_i.slice(..), wgpu::IndexFormat::Uint32);
            for k in 0..nskin {
                rp.set_bind_group(1, &self.skin_bg, &[(k as u64 * SKIN_STRIDE) as u32]);
                rp.draw_indexed(0..self.skin_n, 0, 0..1);
            }
        }
        // 2) the scene
        let t = self.targets.as_ref().unwrap();
        let (view, resolve) = match &t.color_ms {
            Some(ms) => (ms, Some(&t.resolved)),
            None => (&t.resolved, None),
        };
        let c = f.clear;
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: c[0] as f64, g: c[1] as f64, b: c[2] as f64, a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            // reversed-Z: 1 is near, 0 is infinitely far; with a float depth buffer that keeps small gaps apart at any distance
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_bind_group(0, &self.scene_bg, &[]);
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_vertex_buffer(1, self.inst_buf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.set_pipeline(&self.p_opaque);
        draw(&mut rp, &self.ranges, 0);
        draw(&mut rp, &self.ranges, 1);
        rp.set_pipeline(&self.p_skin);
        rp.set_vertex_buffer(0, self.skin_v.slice(..));
        rp.set_index_buffer(self.skin_i.slice(..), wgpu::IndexFormat::Uint32);
        for k in 0..nskin {
            rp.set_bind_group(1, &self.skin_bg, &[(k as u64 * SKIN_STRIDE) as u32]);
            rp.draw_indexed(0..self.skin_n, 0, 0..1);
        }
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_vertex_buffer(1, self.inst_buf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.set_pipeline(&self.p_blend);
        draw(&mut rp, &self.ranges, 2);
        rp.set_pipeline(&self.p_add);
        draw(&mut rp, &self.ranges, 3);
        drop(rp);
        // 3) bloom: down the chain, keeping what's bright, then back up adding it together
        if let (true, Some(first)) = (self.hdr, t.resolved_bg.as_ref()) {
            let pass = |enc: &mut wgpu::CommandEncoder, p: &wgpu::RenderPipeline, from: &wgpu::BindGroup, to: &wgpu::TextureView, keep: bool| {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("bloom"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: to,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: if keep { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(p);
                rp.set_bind_group(0, from, &[]);
                rp.draw(0..3, 0..1);
            };
            pass(enc, &self.p_bright, first, &t.bloom[0].0, false);
            for i in 1..t.bloom.len() {
                pass(enc, &self.p_down, &t.bloom[i - 1].1, &t.bloom[i].0, false);
            }
            for i in (0..t.bloom.len() - 1).rev() {
                pass(enc, &self.p_up, &t.bloom[i + 1].1, &t.bloom[i].0, true);
            }
        }
    }
}

/// One frame's 3D scene, handed to egui as a paint callback.
pub struct SceneCallback(pub Arc<FrameData>);

impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(&self, device: &wgpu::Device, queue: &wgpu::Queue, sd: &egui_wgpu::ScreenDescriptor, enc: &mut wgpu::CommandEncoder, res: &mut egui_wgpu::CallbackResources) -> Vec<wgpu::CommandBuffer> {
        if let Some(gfx) = res.get_mut::<Gfx>() {
            // phones with a very sharp screen render the 3D a little below native resolution
            let k = (1.75 / sd.pixels_per_point).min(1.0) * self.0.scale.clamp(0.4, 1.0);
            let size = [(sd.size_in_pixels[0] as f32 * k) as u32, (sd.size_in_pixels[1] as f32 * k) as u32];
            gfx.render(device, queue, enc, size, &self.0);
        }
        Vec::new()
    }
    fn paint(&self, _info: egui::PaintCallbackInfo, rp: &mut wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let Some(gfx) = res.get::<Gfx>() else { return };
        let Some(t) = gfx.targets.as_ref() else { return };
        rp.set_pipeline(&gfx.p_blit);
        rp.set_bind_group(0, &t.blit_bg, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod shader_tests {
    /// The scene shader must validate and translate to GLSL ES 3.0 (what WebGL2 runs).
    #[test]
    fn shaders_validate_and_translate() {
        for (name, src) in [("scene", include_str!("shader.wgsl")), ("blit", include_str!("blit.wgsl")), ("bloom", include_str!("bloom.wgsl"))] {
            let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            for ep in &module.entry_points {
                let mut out = String::new();
                let opts = naga::back::glsl::Options { version: naga::back::glsl::Version::Embedded { version: 300, is_webgl: true }, ..Default::default() };
                let pipe = naga::back::glsl::PipelineOptions { shader_stage: ep.stage, entry_point: ep.name.clone(), multiview: None };
                let mut w = naga::back::glsl::Writer::new(&mut out, &module, &info, &opts, &pipe, naga::proc::BoundsCheckPolicies::default()).unwrap_or_else(|e| panic!("{name}/{}: {e:?}", ep.name));
                w.write().unwrap_or_else(|e| panic!("{name}/{}: {e:?}", ep.name));
            }
        }
    }
}
