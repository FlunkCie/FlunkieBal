//! The people: Quaternius' Universal Base Characters with his Modular Character Outfits (both CC0), made
//! into `assets/people_m.glb`, `people_f.glb` and the texture atlas `people.png` by `tools/quaternius.py`:
//! a rig per body (male, female), its head, hair styles and outfit parts as meshes, and posed from the
//! game's simple procedural skeleton: each mapped bone takes the world rotation of its skeleton joint,
//! corrected so that the rig's rest pose (a T-pose) matches "limbs hanging straight down". The game's
//! poses were made for a shorter person; `scene::person` fits them to `Legs`.

use crate::body;
use glam::{Mat4, Quat, Vec3};

pub const K: f32 = 1.0; // rig units → metres
/// How much longer the arms are than the models' own (see `Rig::load`).
const ARM: f32 = 1.2;

/// Joint keys of the procedural skeleton (person-local rotations).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Root,
    Hips,
    Spine,
    Neck,
    Head,
    Sh0,
    El0,
    Wr0,
    Sh1,
    El1,
    Wr1,
    Hip0,
    Knee0,
    Hip1,
    Knee1,
}
pub const NKEYS: usize = 15;

fn key_of(name: &str) -> Option<Key> {
    Some(match name {
        "root" => Key::Root,
        "pelvis" => Key::Hips,
        "spine_01" | "spine_02" | "spine_03" | "clavicle_l" | "clavicle_r" => Key::Spine,
        "neck_01" => Key::Neck,
        "Head" => Key::Head,
        "upperarm_r" => Key::Sh0,
        "lowerarm_r" => Key::El0,
        "hand_r" => Key::Wr0,
        "upperarm_l" => Key::Sh1,
        "lowerarm_l" => Key::El1,
        "hand_l" => Key::Wr1,
        "thigh_r" => Key::Hip0,
        "calf_r" => Key::Knee0,
        "thigh_l" => Key::Hip1,
        "calf_l" => Key::Knee1,
        _ => return None,
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub joints: [f32; 4],
    pub weights: [f32; 4],
    /// where in the atlas (`Model::atlas`)
    pub uv: [f32; 2],
    /// the slot of `SkinDraw.colors` its texels may take (by how much: the atlas' alpha), 255: none
    pub slot: f32,
}

struct Node {
    name: String,
    parent: Option<usize>,
    t: Vec3,
    r: Quat,
    s: Vec3,
    key: Option<Key>,
}

/// All the people's meshes, their rigs and their texture.
pub struct Model {
    pub verts: Vec<SkinVertex>,
    pub indices: Vec<u32>,
    /// index range (first, count) of every mesh, by rig and name
    pub parts: Vec<(usize, String, u32, u32)>,
    pub rigs: Vec<Rig>,
    /// width, height, RGBA (sRGB colour; alpha: how much takes the person's colour)
    pub atlas: (u32, u32, Vec<u8>),
    pub stripe_f: f32,
}

/// One skeleton (male or female).
pub struct Rig {
    nodes: Vec<Node>,
    stretch: Vec<Vec3>, // per node: its own vertices stretched along it (the arms)
    order: Vec<usize>,  // parents before children
    joints: Vec<usize>, // skin joint → node
    ibm: Vec<Mat4>,
    rest_q: Vec<Quat>,
    c: [Quat; NKEYS],
    feet_n: [usize; 2], // foot nodes, left and right
    off_body: Vec3,
    body: usize,
    hand_r: usize,
    arm_r: [usize; 2],
    head: usize,
    pub legs: Legs,
}

/// Leg geometry in metres.
#[derive(Clone, Copy, Debug)]
pub struct Legs {
    pub thigh: f32,
    pub shin: f32,
    pub ankle: f32,
    /// the hip joint's offset: sideways, below the hips point, forwards
    pub hip_x: f32,
    pub hip_drop: f32,
    pub hip_z: f32,
    /// the hips point's height standing, and sitting on a 40 cm crate
    pub stand: f32,
    pub seat: f32,
}

pub struct Posed {
    pub feet: [Vec3; 2],   // left, right ankle (foot bone) positions in world space
    pub joints: Vec<Mat4>, // world-space joint matrices (person transform folded in)
    pub hand_r: Vec3,
    /// the right arm's shoulder and elbow joints (world)
    pub arm_r: [Vec3; 2],
    pub head: Mat4, // head frame in world space, metres, facing +Z
}

impl Model {
    /// The rigs' files in order (male, female) and the atlas (PNG).
    pub fn load(glbs: &[&[u8]], png: &[u8]) -> Option<Model> {
        let mut m = Model { verts: Vec::new(), indices: Vec::new(), parts: Vec::new(), rigs: Vec::new(), atlas: decode_png(png)?, stripe_f: K / 0.12 };
        for (r, glb) in glbs.iter().enumerate() {
            let rig = Rig::load(glb, r, &mut m)?;
            m.rigs.push(rig);
        }
        Some(m)
    }

    /// What a person wearing this is drawn with: index ranges.
    pub fn dress(&self, w: body::Wear) -> Vec<(u32, u32)> {
        let rig = w.rig().min(self.rigs.len() - 1);
        (0..body::SLOTS.len()).filter_map(|k| w.piece(k)).filter_map(|c| self.part(rig, &c.piece[2..])).collect()
    }

    /// The index range of a rig's mesh by name, if there is one.
    pub fn part(&self, rig: usize, name: &str) -> Option<(u32, u32)> {
        self.parts.iter().find(|p| p.0 == rig && p.1 == name).map(|p| (p.2, p.3))
    }
}

/// A PNG as RGBA8.
pub(crate) fn decode_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let px = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    Some((info.width, info.height, px))
}

impl Rig {
    /// The rig, its meshes appended to `m` (vertices, indices, `parts`).
    fn load(glb: &[u8], rig: usize, m: &mut Model) -> Option<Rig> {
        let jl = u32::from_le_bytes(glb[12..16].try_into().ok()?) as usize;
        let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).ok()?;
        let bin = &glb[20 + jl + 8..];
        let views = j["bufferViews"].as_array()?;
        let accs = j["accessors"].as_array()?;
        // an accessor as f32s: floats, (normalized) bytes and shorts
        let read = |ai: usize| -> Vec<f32> {
            let a = &accs[ai];
            let v = &views[a["bufferView"].as_u64().unwrap() as usize];
            let n = match a["type"].as_str().unwrap() {
                "VEC2" => 2,
                "VEC3" => 3,
                "VEC4" => 4,
                "MAT4" => 16,
                _ => 1,
            };
            let count = a["count"].as_u64().unwrap() as usize;
            let norm = a["normalized"].as_bool().unwrap_or(false);
            let base = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
            (0..count * n)
                .map(|i| match a["componentType"].as_u64().unwrap() {
                    5120 => {
                        let x = bin[base + i] as i8 as f32;
                        if norm { (x / 127.0).max(-1.0) } else { x }
                    }
                    5121 => bin[base + i] as f32 / if norm { 255.0 } else { 1.0 },
                    5123 => u16::from_le_bytes([bin[base + i * 2], bin[base + i * 2 + 1]]) as f32 / if norm { 65535.0 } else { 1.0 },
                    _ => f32::from_le_bytes(bin[base + i * 4..base + i * 4 + 4].try_into().unwrap()),
                })
                .collect()
        };
        // nodes
        // the bones: the meshes' own nodes come after them
        let all = j["nodes"].as_array()?;
        let jn = &all[..all.iter().position(|n| n.get("mesh").is_some()).unwrap_or(all.len())];
        let mut nodes: Vec<Node> = jn
            .iter()
            .map(|n| {
                let f = |k: &str, d: &[f32]| -> Vec<f32> { n[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect()).unwrap_or(d.to_vec()) };
                let t = f("translation", &[0.0, 0.0, 0.0]);
                let r = f("rotation", &[0.0, 0.0, 0.0, 1.0]);
                let s = f("scale", &[1.0, 1.0, 1.0]);
                let name = n["name"].as_str().unwrap_or("").to_string();
                Node { key: key_of(&name), name, parent: None, t: Vec3::new(t[0], t[1], t[2]), r: Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(), s: Vec3::new(s[0], s[1], s[2]) }
            })
            .collect();
        for (i, n) in jn.iter().enumerate() {
            if let Some(ch) = n["children"].as_array() {
                for c in ch {
                    nodes[c.as_u64().unwrap() as usize].parent = Some(i);
                }
            }
        }
        // longer arms than the models have (theirs are short for a person): the game's poses (a sip, picking
        // up the can) are made for these; a longer bone moves what hangs off it, its own vertices stretch along
        let mut stretch = vec![Vec3::ONE; nodes.len()];
        for i in 0..nodes.len() {
            if nodes[i].name.starts_with("upperarm_") || nodes[i].name.starts_with("lowerarm_") {
                stretch[i] = Vec3::new(1.0, ARM, 1.0);
                for c in 0..nodes.len() {
                    if nodes[c].parent == Some(i) {
                        nodes[c].t *= ARM;
                    }
                }
            }
        }
        let mut order = Vec::new();
        let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].parent.is_none()).collect();
        stack.reverse();
        while let Some(i) = stack.pop() {
            order.push(i);
            if let Some(ch) = jn[i]["children"].as_array() {
                for c in ch.iter().rev() {
                    stack.push(c.as_u64().unwrap() as usize);
                }
            }
        }
        // rest globals
        let mut g = vec![Mat4::IDENTITY; nodes.len()];
        for &i in &order {
            let l = Mat4::from_scale_rotation_translation(nodes[i].s, nodes[i].r, nodes[i].t);
            g[i] = match nodes[i].parent {
                Some(p) => g[p] * l,
                None => l,
            };
        }
        let idx = |name: &str| nodes.iter().position(|n| n.name == name).unwrap();
        let pos = |name: &str| g[idx(name)].transform_point3(Vec3::ZERO);
        let rest_q: Vec<Quat> = g.iter().map(|m| m.to_scale_rotation_translation().1.normalize()).collect();
        // limb corrections so that the rest pose hangs straight down
        let mut c = [Quat::IDENTITY; NKEYS];
        let down = Vec3::NEG_Y;
        let mut chain = |a: &str, b: &str, cc: &str, k1: Key, k2: Key| {
            let d1 = (pos(b) - pos(a)).normalize();
            let q1 = Quat::from_rotation_arc(d1, down);
            let d2 = q1 * (pos(cc) - pos(b)).normalize();
            let q2 = Quat::from_rotation_arc(d2, down) * q1;
            c[k1 as usize] = q1;
            c[k2 as usize] = q2;
        };
        chain("upperarm_r", "lowerarm_r", "hand_r", Key::Sh0, Key::El0);
        chain("upperarm_l", "lowerarm_l", "hand_l", Key::Sh1, Key::El1);
        chain("thigh_r", "calf_r", "foot_r", Key::Hip0, Key::Knee0);
        chain("thigh_l", "calf_l", "foot_l", Key::Hip1, Key::Knee1);
        c[Key::Wr0 as usize] = c[Key::El0 as usize];
        c[Key::Wr1 as usize] = c[Key::El1 as usize];
        let (hp, ul, ll, ft) = (pos("pelvis") * K, pos("thigh_r") * K, pos("calf_r") * K, pos("foot_r") * K);
        // the hips point is where the game's skeleton has its hips: at the hip joints' height
        let stand = ul.y;
        // the pelvis goes where the pose puts the hips (this rig stands a little behind its origin)
        let off_body = Vec3::new(hp.x, hp.y - stand, 0.0);
        // sitting, the hip joints are a thigh's thickness above the seat (measured: `poses_touch_the_world`)
        let seat = 0.4 + 0.132;
        let legs = Legs { thigh: ul.distance(ll), shin: ll.distance(ft), ankle: ft.y, hip_x: ul.x.abs(), hip_drop: stand - ul.y, hip_z: ul.z - hp.z, stand, seat };
        // skin
        let skin = &j["skins"][0];
        let joints: Vec<usize> = skin["joints"].as_array()?.iter().map(|x| x.as_u64().unwrap() as usize).collect();
        let ibm: Vec<Mat4> = read(skin["inverseBindMatrices"].as_u64()? as usize).chunks(16).map(Mat4::from_cols_slice).collect();
        // every mesh, one after the other
        for mesh in j["meshes"].as_array()? {
            let first = m.indices.len() as u32;
            for p in mesh["primitives"].as_array()? {
                let at = &p["attributes"];
                let get = |k: &str| at[k].as_u64().map(|a| read(a as usize));
                let (ps, ns, js, ws, us, cs) = (get("POSITION")?, get("NORMAL")?, get("JOINTS_0")?, get("WEIGHTS_0")?, get("TEXCOORD_0")?, get("COLOR_0")?);
                let base = m.verts.len() as u32;
                for i in 0..ps.len() / 3 {
                    m.verts.push(SkinVertex {
                        pos: [ps[i * 3], ps[i * 3 + 1], ps[i * 3 + 2]],
                        nrm: [ns[i * 3], ns[i * 3 + 1], ns[i * 3 + 2]],
                        joints: [js[i * 4], js[i * 4 + 1], js[i * 4 + 2], js[i * 4 + 3]],
                        weights: [ws[i * 4], ws[i * 4 + 1], ws[i * 4 + 2], ws[i * 4 + 3]],
                        uv: [us[i * 2], us[i * 2 + 1]],
                        slot: cs[i * 4],
                    });
                }
                m.indices.extend(read(p["indices"].as_u64()? as usize).iter().map(|x| *x as u32 + base));
            }
            m.parts.push((rig, mesh["name"].as_str().unwrap_or("").to_string(), first, m.indices.len() as u32 - first));
        }
        let (body, hand_r, head) = (idx("pelvis"), idx("hand_r"), idx("Head"));
        let arm_r = [idx("upperarm_r"), idx("lowerarm_r")];
        let feet_n = [idx("foot_l"), idx("foot_r")];
        Some(Rig { nodes, stretch, order, joints, ibm, rest_q, c, feet_n, off_body, body, hand_r, arm_r, head, legs })
    }

    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Pose the rig. `j` holds person-local joint rotations, `hips` the person-local hip position
    /// (metres), `person` the person's world transform (position and yaw). `build` widens or narrows
    /// body parts (`body::Build`): each bone's own vertices, not what hangs off it.
    pub fn pose(&self, j: &[Quat; NKEYS], hips: Vec3, person: Mat4, feet: Quat, build: &body::Build) -> Posed {
        let n = self.nodes.len();
        let mut g = vec![Mat4::IDENTITY; n];
        for &i in &self.order {
            let nd = &self.nodes[i];
            let pg = nd.parent.map(|p| g[p]).unwrap_or(Mat4::IDENTITY);
            match nd.key {
                Some(k) => {
                    let wq = j[k as usize] * self.c[k as usize] * self.rest_q[i];
                    let (ps, _, _) = pg.to_scale_rotation_translation();
                    let mut t = pg.transform_point3(nd.t);
                    if i == self.body {
                        t = (hips + j[Key::Hips as usize] * self.off_body) / K;
                    }
                    g[i] = Mat4::from_scale_rotation_translation(ps * nd.s, wq, t);
                }
                // the foot hangs off the shin but stays level with the ground
                None if self.feet_n.contains(&i) => {
                    let (sc, _, tr) = (pg * Mat4::from_scale_rotation_translation(nd.s, nd.r, nd.t)).to_scale_rotation_translation();
                    g[i] = Mat4::from_scale_rotation_translation(sc, feet * self.rest_q[i], tr);
                }
                None => g[i] = pg * Mat4::from_scale_rotation_translation(nd.s, nd.r, nd.t),
            }
        }
        let pk = person * Mat4::from_scale(Vec3::splat(K));
        let joints = self.joints.iter().zip(self.ibm.iter()).map(|(&nd, ib)| pk * g[nd] * Mat4::from_scale(build.scale_of(&self.nodes[nd].name) * self.stretch[nd]) * *ib).collect();
        let hand_r = pk.transform_point3(g[self.hand_r].transform_point3(Vec3::ZERO));
        let feet = [pk.transform_point3(g[self.feet_n[0]].transform_point3(Vec3::ZERO)), pk.transform_point3(g[self.feet_n[1]].transform_point3(Vec3::ZERO))];
        let ht = g[self.head].transform_point3(Vec3::ZERO);
        // the head frame: at the head bone, oriented like the person's head joint (+Z forward)
        let head = person * Mat4::from_rotation_translation(j[Key::Head as usize], ht * K);
        let arm_r = self.arm_r.map(|n| pk.transform_point3(g[n].transform_point3(Vec3::ZERO)));
        Posed { feet, joints, hand_r, arm_r, head }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn load() -> Model {
        Model::load(&[include_bytes!("../assets/people_m.glb"), include_bytes!("../assets/people_f.glb")], include_bytes!("../assets/people.png")).unwrap()
    }
    #[test]
    fn rest_pose_proportions() {
        let m = load();
        for (r, rig) in m.rigs.iter().enumerate() {
            let p = rig.pose(&[Quat::IDENTITY; NKEYS], Vec3::new(0.0, rig.legs.stand, 0.0), Mat4::IDENTITY, Quat::IDENTITY, &body::BUILDS[0]);
            let head = p.head.transform_point3(Vec3::ZERO);
            println!("rig {r}: joints {} hand {:?} head {:?} legs {:?}", rig.joint_count(), p.hand_r, head, rig.legs);
            // standing: hand by the hip on the right (-X), the head bone up high
            assert!(p.hand_r.y > 0.6 && p.hand_r.y < 1.1 && p.hand_r.x < -0.1, "hand {:?}", p.hand_r);
            assert!(head.y > 1.4 && head.y < 1.8, "head {:?}", head);
            // the head and shoes end up in a person-sized box, the soles on the ground
            let (mut lo, mut hi) = (Vec3::splat(9.0), Vec3::splat(-9.0));
            for name in ["Casual_Head", "Casual_Feet"] {
                let (first, n) = m.part(r, name).unwrap();
                for &i in &m.indices[first as usize..(first + n) as usize] {
                    let v = &m.verts[i as usize];
                    let mut w = Vec3::ZERO;
                    for k in 0..4 {
                        w += p.joints[v.joints[k] as usize].transform_point3(Vec3::from_array(v.pos)) * v.weights[k];
                    }
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
            println!("bbox {lo:?} {hi:?}");
            assert!(lo.y > -0.05 && lo.y < 0.05 && hi.y < 2.0 && hi.y > 1.6, "bbox {lo:?} {hi:?}");
        }
    }
}
