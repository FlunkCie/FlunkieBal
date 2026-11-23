//! The park's plants and stones: Quaternius' Stylized Nature MegaKit (CC0), made into `assets/nature.glb` and
//! `assets/nature.png` by `tools/nature.py`. Drawn instanced like the other meshes (`DrawList::nature`, by
//! their number in `MODELS`), with their own textures (a layer each), cut out where the texture is see-through,
//! and swaying in the wind (leaves, grass and flowers, more the higher up).

use glam::Vec3;

/// The models, in the file's order: `DrawList::nature` names them by their number here (`id`).
pub const MODELS: [&str; 27] = [
    "CommonTree_1", "CommonTree_2", "CommonTree_3", "CommonTree_4", "CommonTree_5", "TwistedTree_1", "TwistedTree_3",
    "Bush_Common", "Bush_Common_Flowers", "Grass_Common_Short", "Grass_Common_Tall", "Grass_Wispy_Short", "Grass_Wispy_Tall",
    "Fern_1", "Clover_1", "Clover_2", "Flower_3_Group", "Flower_4_Group", "Plant_1", "Plant_7",
    "Rock_Medium_1", "Rock_Medium_2", "Rock_Medium_3", "Pebble_Round_1", "Pebble_Round_2", "Pebble_Round_3", "Mushroom_Common",
];
/// A model's number.
pub fn id(name: &str) -> u16 {
    MODELS.iter().position(|m| *m == name).unwrap_or(0) as u16
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NVertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
    pub col: [f32; 3],
    /// texture layer, and how much the wind moves it
    pub ls: [f32; 2],
}

pub struct Nature {
    pub verts: Vec<NVertex>,
    pub indices: Vec<u32>,
    /// per model: first index, count; and a sphere round it (centre, radius)
    pub ranges: Vec<(u32, u32)>,
    pub bounds: Vec<(Vec3, f32)>,
    /// the texture layers: size, how many, RGBA one after the other
    pub tex: (u32, u32, Vec<u8>),
}

impl Nature {
    pub fn load(glb: &[u8], png: &[u8]) -> Option<Nature> {
        let jl = u32::from_le_bytes(glb[12..16].try_into().ok()?) as usize;
        let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).ok()?;
        let bin = &glb[20 + jl + 8..];
        let views = j["bufferViews"].as_array()?;
        let accs = j["accessors"].as_array()?;
        let read = |ai: usize| -> Vec<f32> {
            let a = &accs[ai];
            let v = &views[a["bufferView"].as_u64().unwrap() as usize];
            let n = match a["type"].as_str().unwrap() {
                "VEC2" => 2,
                "VEC3" => 3,
                "VEC4" => 4,
                _ => 1,
            };
            let count = a["count"].as_u64().unwrap() as usize;
            let norm = a["normalized"].as_bool().unwrap_or(false);
            let base = v["byteOffset"].as_u64().unwrap_or(0) as usize;
            (0..count * n)
                .map(|i| match a["componentType"].as_u64().unwrap() {
                    5120 => {
                        let x = bin[base + i] as i8 as f32;
                        if norm { (x / 127.0).max(-1.0) } else { x }
                    }
                    5121 => bin[base + i] as f32 / if norm { 255.0 } else { 1.0 },
                    5125 => u32::from_le_bytes(bin[base + i * 4..base + i * 4 + 4].try_into().unwrap()) as f32,
                    _ => f32::from_le_bytes(bin[base + i * 4..base + i * 4 + 4].try_into().unwrap()),
                })
                .collect()
        };
        let mut n = Nature { verts: Vec::new(), indices: Vec::new(), ranges: Vec::new(), bounds: Vec::new(), tex: (0, 0, Vec::new()) };
        for name in MODELS {
            let mesh = j["meshes"].as_array()?.iter().find(|m| m["name"] == name)?;
            let at = &mesh["primitives"][0]["attributes"];
            let get = |k: &str| at[k].as_u64().map(|a| read(a as usize));
            let (ps, ns, us, cs, ls) = (get("POSITION")?, get("NORMAL")?, get("TEXCOORD_0")?, get("COLOR_0")?, get("TEXCOORD_1")?);
            let base = n.verts.len() as u32;
            let first = n.indices.len() as u32;
            let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
            for i in 0..ps.len() / 3 {
                let p = Vec3::new(ps[i * 3], ps[i * 3 + 1], ps[i * 3 + 2]);
                lo = lo.min(p);
                hi = hi.max(p);
                n.verts.push(NVertex { pos: p.to_array(), nrm: [ns[i * 3], ns[i * 3 + 1], ns[i * 3 + 2]], uv: [us[i * 2], us[i * 2 + 1]], col: [cs[i * 3], cs[i * 3 + 1], cs[i * 3 + 2]], ls: [ls[i * 2], ls[i * 2 + 1]] });
            }
            // indices made global: WebGL2 has no base-vertex draws
            n.indices.extend(read(mesh["primitives"][0]["indices"].as_u64()? as usize).iter().map(|x| *x as u32 + base));
            n.ranges.push((first, n.indices.len() as u32 - first));
            let c = (lo + hi) * 0.5;
            n.bounds.push((c, (hi - lo).length() * 0.5));
        }
        let (w, h, px) = crate::model::decode_png(png)?;
        n.tex = (w, h / w, px);
        Some(n)
    }
}
