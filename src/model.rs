//! The low-poly human (Quaternius "BaseHuman", CC0), loaded from the embedded GLB and posed
//! from the game's simple procedural skeleton, the same way the JavaScript version did it:
//! each mapped bone takes the world rotation of its skeleton joint, corrected so that the
//! model's rest pose matches "limbs hanging straight down".

use glam::{Mat4, Quat, Vec3};

pub const K: f32 = 0.375; // model units → metres
pub const SK_HIP: f32 = crate::anim::STAND_HIP;

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
        "Bone" => Key::Root,
        "Body" | "Hips" => Key::Hips,
        "Abdomen" | "Torso" | "Shoulder.L" | "Shoulder.R" => Key::Spine,
        "Neck" => Key::Neck,
        "Head" => Key::Head,
        "UpperArm.R" => Key::Sh0,
        "LowerArm.R" => Key::El0,
        "Palm.R" | "MiddleHand.R" | "Fingers.R" | "Thumb1.R" | "Thumb2.R" => Key::Wr0,
        "UpperArm.L" => Key::Sh1,
        "LowerArm.L" => Key::El1,
        "Palm.L" | "MiddleHand.L" | "Fingers.L" | "Thumb1.L" | "Thumb2.L" => Key::Wr1,
        "UpperLeg.R" => Key::Hip0,
        "LowerLeg.R" => Key::Knee0,
        "UpperLeg.L" => Key::Hip1,
        "LowerLeg.L" => Key::Knee1,
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
    pub mat: f32, // material slot
}

pub const MATS: [&str; 6] = ["Shirt", "Skin", "Pants", "Eyes", "Socks", "Hair"];

struct Node {
    name: String,
    parent: Option<usize>,
    t: Vec3,
    r: Quat,
    s: Vec3,
    key: Option<Key>,
}

pub struct Model {
    pub verts: Vec<SkinVertex>,
    pub indices: Vec<u32>,
    nodes: Vec<Node>,
    order: Vec<usize>, // parents before children
    joints: Vec<usize>, // skin joint → node
    ibm: Vec<Mat4>,
    rest_q: Vec<Quat>,
    c: [Quat; NKEYS],
    rel_foot: [(usize, usize, Mat4); 2], // (lower leg node, foot node, foot relative to lower leg)
    off_body: Vec3,
    body: usize,
    palm_r: usize,
    head: usize,
    pub stripe_f: f32,
    /// leg geometry in metres: thigh, shin, ankle height, hip joint offset (x, y below the hips point, z)
    pub legs: Legs,
}

#[derive(Clone, Copy, Debug)]
pub struct Legs {
    pub thigh: f32,
    pub shin: f32,
    pub ankle: f32,
    pub hip_x: f32,
    pub hip_drop: f32,
    pub hip_z: f32,
}

pub struct Posed {
    pub feet: [Vec3; 2],    // left, right ankle (foot bone) positions in world space
    pub joints: Vec<Mat4>,  // world-space joint matrices (person transform folded in)
    pub hand_r: Vec3,
    pub head: Mat4,         // head frame in world space, metres, facing +Z
}

fn rd_f32(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Model {
    pub fn load(glb: &[u8]) -> Option<Model> {
        let jl = u32::from_le_bytes(glb[12..16].try_into().ok()?) as usize;
        let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).ok()?;
        let bin_off = 20 + jl + 8;
        let bin = &glb[bin_off..];
        let views = j["bufferViews"].as_array()?;
        let accs = j["accessors"].as_array()?;
        // read an accessor as f32 components (handles normalized ints) or raw ints
        let read = |ai: usize| -> (Vec<f32>, usize) {
            let a = &accs[ai];
            let v = &views[a["bufferView"].as_u64().unwrap() as usize];
            let ct = a["componentType"].as_u64().unwrap();
            let n = match a["type"].as_str().unwrap() {
                "SCALAR" => 1,
                "VEC2" => 2,
                "VEC3" => 3,
                "VEC4" => 4,
                "MAT4" => 16,
                _ => 1,
            };
            let count = a["count"].as_u64().unwrap() as usize;
            let norm = a["normalized"].as_bool().unwrap_or(false);
            let csz = match ct {
                5120 | 5121 => 1,
                5122 | 5123 => 2,
                _ => 4,
            };
            let base = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
            let stride = v["byteStride"].as_u64().map(|s| s as usize).unwrap_or(n * csz);
            let mut out = Vec::with_capacity(count * n);
            for i in 0..count {
                for k in 0..n {
                    let o = base + i * stride + k * csz;
                    let x = match ct {
                        5120 => {
                            let r = bin[o] as i8 as f32;
                            if norm { (r / 127.0).max(-1.0) } else { r }
                        }
                        5121 => {
                            let r = bin[o] as f32;
                            if norm { r / 255.0 } else { r }
                        }
                        5122 => {
                            let r = i16::from_le_bytes([bin[o], bin[o + 1]]) as f32;
                            if norm { (r / 32767.0).max(-1.0) } else { r }
                        }
                        5123 => {
                            let r = u16::from_le_bytes([bin[o], bin[o + 1]]) as f32;
                            if norm { r / 65535.0 } else { r }
                        }
                        5125 => u32::from_le_bytes([bin[o], bin[o + 1], bin[o + 2], bin[o + 3]]) as f32,
                        _ => rd_f32(bin, o),
                    };
                    out.push(x);
                }
            }
            (out, n)
        };
        // nodes
        let jn = j["nodes"].as_array()?;
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
        chain("UpperArm.R", "LowerArm.R", "Palm.R", Key::Sh0, Key::El0);
        chain("UpperArm.L", "LowerArm.L", "Palm.L", Key::Sh1, Key::El1);
        chain("UpperLeg.R", "LowerLeg.R", "Foot.R", Key::Hip0, Key::Knee0);
        chain("UpperLeg.L", "LowerLeg.L", "Foot.L", Key::Hip1, Key::Knee1);
        c[Key::Wr0 as usize] = c[Key::El0 as usize];
        c[Key::Wr1 as usize] = c[Key::El1 as usize];
        let rel = |leg: &str, foot: &str| (idx(leg), idx(foot), g[idx(leg)].inverse() * g[idx(foot)]);
        let rel_foot = [rel("LowerLeg.R", "Foot.R"), rel("LowerLeg.L", "Foot.L")];
        let off_body = pos("Body") * K - Vec3::new(0.0, SK_HIP, 0.0);
        // skin
        let skin = &j["skins"][0];
        let joints: Vec<usize> = skin["joints"].as_array()?.iter().map(|x| x.as_u64().unwrap() as usize).collect();
        let (ib, _) = read(skin["inverseBindMatrices"].as_u64()? as usize);
        let ibm: Vec<Mat4> = ib.chunks(16).map(Mat4::from_cols_slice).collect();
        // mesh: all primitives merged, tagged with their material slot
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        let mut shirt_half = 0.0f32;
        for p in j["meshes"][0]["primitives"].as_array()? {
            let mname = j["materials"][p["material"].as_u64()? as usize]["name"].as_str().unwrap_or("");
            let slot = MATS.iter().position(|m| *m == mname).unwrap_or(1) as f32;
            let at = &p["attributes"];
            let (ps, _) = read(at["POSITION"].as_u64()? as usize);
            let (ns, _) = read(at["NORMAL"].as_u64()? as usize);
            let (js, _) = read(at["JOINTS_0"].as_u64()? as usize);
            let (ws, _) = read(at["WEIGHTS_0"].as_u64()? as usize);
            let (is, _) = read(p["indices"].as_u64()? as usize);
            let base = verts.len() as u32;
            for i in 0..ps.len() / 3 {
                let w = [ws[i * 4], ws[i * 4 + 1], ws[i * 4 + 2], ws[i * 4 + 3]];
                let sum = (w[0] + w[1] + w[2] + w[3]).max(1e-6);
                let n = Vec3::new(ns[i * 3], ns[i * 3 + 1], ns[i * 3 + 2]).normalize_or(Vec3::Y);
                if slot == 0.0 {
                    shirt_half = shirt_half.max(ps[i * 3].abs());
                }
                verts.push(SkinVertex {
                    pos: [ps[i * 3], ps[i * 3 + 1], ps[i * 3 + 2]],
                    nrm: n.to_array(),
                    joints: [js[i * 4], js[i * 4 + 1], js[i * 4 + 2], js[i * 4 + 3]],
                    weights: [w[0] / sum, w[1] / sum, w[2] / sum, w[3] / sum],
                    mat: slot,
                });
            }
            indices.extend(is.iter().map(|x| *x as u32 + base));
        }
        let (ul, ll, ft) = (pos("UpperLeg.R") * K, pos("LowerLeg.R") * K, pos("Foot.R") * K);
        let legs = Legs { thigh: ul.distance(ll), shin: ll.distance(ft), ankle: ft.y, hip_x: ul.x.abs(), hip_drop: SK_HIP - ul.y, hip_z: ul.z };
        let body = idx("Body");
        let palm_r = idx("MiddleHand.R");
        let head = idx("Head");
        Some(Model { verts, indices, nodes, order, joints, ibm, rest_q, c, rel_foot, off_body, body, palm_r, head, stripe_f: if shirt_half > 0.0 { 11.0 / shirt_half } else { 13.5 }, legs })
    }

    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Pose the model. `j` holds person-local joint rotations, `hips` the person-local hip position
    /// (metres), `person` the person's world transform (position and yaw).
    pub fn pose(&self, j: &[Quat; NKEYS], hips: Vec3, person: Mat4, feet: Quat) -> Posed {
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
                None => g[i] = pg * Mat4::from_scale_rotation_translation(nd.s, nd.r, nd.t),
            }
        }
        for (leg, foot, rel) in self.rel_foot {
            // the foot hangs off the shin but stays level with the ground
            let (sc, _, tr) = (g[leg] * rel).to_scale_rotation_translation();
            g[foot] = Mat4::from_scale_rotation_translation(sc, feet * self.rest_q[foot], tr);
        }
        let pk = person * Mat4::from_scale(Vec3::splat(K));
        let joints = self.joints.iter().zip(self.ibm.iter()).map(|(&nd, ib)| pk * g[nd] * *ib).collect();
        let hand_r = pk.transform_point3(g[self.palm_r].transform_point3(Vec3::ZERO));
        let feet = [pk.transform_point3(g[self.rel_foot[1].1].transform_point3(Vec3::ZERO)), pk.transform_point3(g[self.rel_foot[0].1].transform_point3(Vec3::ZERO))];
        let (_, hq, ht) = g[self.head].to_scale_rotation_translation();
        let _ = hq;
        // the head frame: at the head bone, oriented like the person's head joint (+Z forward)
        let head = person * Mat4::from_rotation_translation(j[Key::Head as usize], ht * K);
        Posed { feet, joints, hand_r, head }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rest_pose_proportions() {
        let m = Model::load(include_bytes!("../assets/person.glb")).unwrap();
        let p = m.pose(&[Quat::IDENTITY; NKEYS], Vec3::new(0.0, SK_HIP, 0.0), Mat4::IDENTITY, Quat::IDENTITY);
        let head = p.head.transform_point3(Vec3::ZERO);
        println!("joints {} hand {:?} head {:?} stripe {}", m.joint_count(), p.hand_r, head, m.stripe_f);
        // a standing adult: hand at hip height on the right (-X), head bone high up
        assert!(p.hand_r.y > 0.6 && p.hand_r.y < 1.0 && p.hand_r.x < -0.1, "hand {:?}", p.hand_r);
        assert!(head.y > 1.35 && head.y < 1.75, "head {:?}", head);
        // every vertex ends up within a person-sized box
        let (mut lo, mut hi) = (Vec3::splat(9.0), Vec3::splat(-9.0));
        for v in &m.verts {
            let mut w = Vec3::ZERO;
            for k in 0..4 {
                w += p.joints[v.joints[k] as usize].transform_point3(Vec3::from_array(v.pos)) * v.weights[k];
            }
            lo = lo.min(w);
            hi = hi.max(w);
        }
        println!("bbox {lo:?} {hi:?}");
        assert!(lo.y > -0.05 && hi.y < 1.95 && hi.y > 1.5);
    }
}
