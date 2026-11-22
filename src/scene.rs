//! Turns the game state into draw instances: people (built from primitives and animated
//! procedurally), the Vondelpark set, the bottle, ball, beers, the referee's gaze and smoke.

use crate::game::{self, AMode, Game, Person, State, BR, H, R};
use crate::gfx::{inst, kind, DrawList, Mesh, SkinDraw};
use crate::model::Model;
use glam::{vec3, Mat4, Quat, Vec3};
use std::f32::consts::PI;

/// sRGB hex → linear RGB.
pub fn hex(c: u32) -> [f32; 3] {
    let f = |v: u32| ((v & 255) as f32 / 255.0).powf(2.2);
    [f(c >> 16), f(c >> 8), f(c)]
}
pub fn t(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(vec3(x, y, z))
}
pub fn s(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_scale(vec3(x, y, z))
}
pub fn su(v: f32) -> Mat4 {
    Mat4::from_scale(Vec3::splat(v))
}
pub fn rx(a: f32) -> Mat4 {
    Mat4::from_rotation_x(a)
}
pub fn ry(a: f32) -> Mat4 {
    Mat4::from_rotation_y(a)
}
pub fn rz(a: f32) -> Mat4 {
    Mat4::from_rotation_z(a)
}
pub fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k.clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
pub struct Look {
    pub shirt: [f32; 3],
    pub skin: [f32; 3],
    pub hair: [f32; 3],
    pub shorts: [f32; 3],
    pub shoes: [f32; 3],
    pub socks: [f32; 3],
    pub hair_style: u8,
    pub cap: Option<[f32; 3]>,
    pub stripes: bool,
    pub hat: u8,
}

const SKINS: [u32; 5] = [0xd9a98a, 0xc98e66, 0x9a6544, 0x6e4329, 0xe2b597];
const HAIRS: [u32; 6] = [0x2a1c13, 0x5a3a1e, 0xb08a4f, 0x1a1a1a, 0x7b4a2a, 0xd8c08a];
const PANTS: [u32; 5] = [0x2b3a57, 0x3c4c6e, 0x1f1f22, 0x4a4f3a, 0x55606e];
const SHOES: [u32; 5] = [0xeeeeee, 0x2a2a2a, 0x8a3a2a, 0xd8d0c0, 0x30507a];

pub fn player_look(team_color: [f32; 3], i: usize, n: usize) -> Look {
    // the three shirts of a team differ a touch, like the JavaScript version's teamShade
    let k = (i as f32 - 1.0) * 0.06;
    let shirt = [(team_color[0] * (1.0 + k)).min(1.0), (team_color[1] * (1.0 + k)).min(1.0), (team_color[2] * (1.0 + k)).min(1.0)];
    Look {
        shirt,
        skin: hex(SKINS[n % 5]),
        hair: hex(HAIRS[(n * 2 + 1) % 6]),
        shorts: hex(PANTS[n % 5]),
        shoes: hex(SHOES[(n + 2) % 5]),
        socks: hex(0xf0f0ea),
        hair_style: (n % 3) as u8,
        cap: None,
        stripes: false,
        hat: 0,
    }
}
pub fn ref_look() -> Look {
    Look { shirt: hex(0xf4f4f0), skin: hex(0xc98e66), hair: hex(0x1a1a1a), shorts: hex(0x161616), shoes: hex(0x161616), socks: hex(0x161616), hair_style: 0, cap: Some(hex(0x111111)), stripes: true, hat: 0 }
}

pub struct Rig {
    pub hand_r: Vec3,
    pub hand_r_m: Mat4,
    pub mouth: Vec3,
    pub head: Mat4,
    pub feet: [Vec3; 2],
}

/// Draw one person (the skinned model) and return where their hand and mouth are.
pub fn person(dl: &mut DrawList, model: &Model, p: &Person, lk: &Look, draw: bool) -> Rig {
    use crate::model::Key;
    let o = p.vis.pose;
    let q = |m: Mat4| Quat::from_mat4(&m).normalize();
    let mut j = [Quat::IDENTITY; crate::model::NKEYS];
    let pelvis = ry(o.pel_yaw) * rz(o.pel_roll);
    j[Key::Hips as usize] = q(pelvis);
    let spine = pelvis * rx(o.lean) * ry(o.twist) * rz(o.side);
    j[Key::Spine as usize] = q(spine);
    j[Key::Neck as usize] = q(spine * ry(o.head_yaw * 0.4));
    let head = spine * ry(o.head_yaw) * rx(o.head_pitch);
    j[Key::Head as usize] = q(head);
    for (ai, sd) in [(0usize, 1.0f32), (1, -1.0)] {
        let [fwd, out, elbow] = o.arm[ai];
        let sh = spine * rz(sd * out) * rx(-fwd);
        let el = sh * rx(-elbow);
        let (ks, ke, kw) = if ai == 0 { (Key::Sh1, Key::El1, Key::Wr1) } else { (Key::Sh0, Key::El0, Key::Wr0) };
        j[ks as usize] = q(sh);
        j[ke as usize] = q(el);
        j[kw as usize] = q(el);
    }
    // legs: two-bone IK from the hip joint to the foot target, so feet land on the ground
    let lg = model.legs;
    let pinv = pelvis.inverse();
    for (li, sd) in [(0usize, 1.0f32), (1, -1.0)] {
        let hj = o.hip + pelvis.transform_vector3(vec3(sd * lg.hip_x, -lg.hip_drop, lg.hip_z));
        let vp = pinv.transform_vector3(o.foot[li] - hj);
        let abd = vp.x.atan2(-vp.y).clamp(-0.5, 0.5);
        let vy = -(vp.x * vp.x + vp.y * vp.y).sqrt();
        let (l1, l2) = (lg.thigh, lg.shin);
        let d = (vy * vy + vp.z * vp.z).sqrt().clamp(0.2, (l1 + l2) * 0.999);
        let down = vp.z.atan2(-vy);
        let a1 = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0).acos();
        let knee = PI - ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0).acos();
        let hip = pelvis * rz(abd) * rx(-(down + a1));
        let kn = hip * rx(knee);
        let (kh, kk) = if li == 0 { (Key::Hip1, Key::Knee1) } else { (Key::Hip0, Key::Knee0) };
        j[kh as usize] = q(hip);
        j[kk as usize] = q(kn);
    }
    let root = t(p.pos.x, p.pos.y, p.pos.z) * ry(p.vis.yaw);
    let posed = model.pose(&j, o.hip, root, q(ry(o.pel_yaw * 0.5)));
    if draw {
        // where they meet the ground, less light gets in
        dl.blend.push((Mesh::Quad, inst(t(p.pos.x, 0.012, p.pos.z) * rx(-PI * 0.5) * su(0.85), [0.0, 0.0, 0.0, 0.62], [kind::SOFT, 0.0, 0.0, 0.0])));
        let c = |v: [f32; 3]| [v[0], v[1], v[2], 1.0];
        dl.skinned.push(SkinDraw {
            bones: posed.joints,
            colors: [c(lk.shirt), c(lk.skin), c(lk.shorts), [0.02, 0.018, 0.016, 1.0], c(lk.socks), c(lk.hair)],
            stripe: if lk.stripes { model.stripe_f } else { 0.0 },
        });
        if let Some(cap) = lk.cap {
            hat(dl, posed.head, 1, cap);
        } else if lk.hat > 0 {
            hat(dl, posed.head, lk.hat, [0.0; 3]);
        }
    }
    let mouth = posed.head.transform_point3(vec3(0.0, 0.075, 0.105));
    let hand_r_m = root * Mat4::from_translation(root.inverse().transform_point3(posed.hand_r)) * Mat4::from_quat(j[Key::Wr0 as usize]);
    Rig { hand_r: posed.hand_r, hand_r_m, mouth, head: posed.head, feet: posed.feet }
}

pub const HAT_NAMES: [&str; 6] = ["None", "Cap", "Bucket", "Shades", "Band", "Crown"];

/// What's in the hand during an emote: a smoking cigarette, or a Buzzball on its way to the lips.
fn emote_prop(dl: &mut DrawList, p: &Person, rig: &Rig, cam: &Cam, seed: usize) {
    use crate::anim::{emote_lift, EMOTES, EMOTE_BUZZ, EMOTE_SMOKE};
    let (i, et) = (p.anim.emote - 1, p.anim.emote_t);
    let dur = EMOTES[i as usize % EMOTES.len()].2;
    // in the hand once the arm is on its way, gone again as it comes back
    if et < 0.15 || et > dur - 0.2 {
        return;
    }
    let lift = emote_lift(i, et);
    if i == EMOTE_SMOKE {
        let cig = rig.hand_r_m * t(0.0, -0.02, 0.04) * rx(PI * 0.5);
        dl.solid(Mesh::Cyl, cig * s(0.0048, 0.07, 0.0048), hex(0xf4f1ea));
        let tip = cig.transform_point3(vec3(0.0, 0.037, 0.0));
        dl.add.push((Mesh::Quad, inst(billboard(cam.pos, tip, 0.04 + 0.04 * lift), [1.0, 0.45, 0.12, 0.45 + 0.5 * lift], [kind::SOFT, 0.0, 0.0, 0.0])));
        // what he breathes out rises from his mouth, right after the drag
        let u = et % 2.6;
        if u > 1.0 {
            for k in 0..5 {
                let a = u - 1.0 - k as f32 * 0.14;
                if a > 0.0 && a < 1.3 {
                    let at = rig.mouth + vec3(0.04 * (a * 3.0 + k as f32).sin(), 0.28 * a, 0.0);
                    dl.add.push((Mesh::Quad, inst(billboard(cam.pos, at, 0.05 + 0.16 * a), [0.85, 0.85, 0.85, 0.28 * (1.0 - a / 1.3)], [kind::SOFT, 0.0, 0.0, 0.0])));
                }
            }
        }
    } else if i == EMOTE_BUZZ {
        let up = Vec3::Y.lerp((rig.mouth - rig.hand_r).normalize_or(Vec3::Y), lift).normalize_or(Vec3::Y);
        crate::arenas::buzz(dl, Mat4::from_translation(rig.hand_r - up * 0.05) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, up)), seed);
    }
}
/// Hats sit on the head frame (origin at the base of the skull, +Z forward).
pub fn hat(dl: &mut DrawList, head: Mat4, h: u8, tint: [f32; 3]) {
    let m = head * t(0.0, HAT_Y, HAT_Z);
    match h {
        1 => {
            let c = if tint == [0.0; 3] { hex(0x1d3557) } else { tint };
            dl.solid(Mesh::Hemi, m * t(0.0, 0.0, -0.004) * s(0.104, 0.085, 0.112), c);
            dl.solid(Mesh::Cube, m * t(0.0, 0.006, 0.118) * rx(-0.12) * s(0.15, 0.012, 0.09), c);
        }
        2 => {
            let c = hex(0xb8a27a);
            dl.solid(Mesh::Cyl, m * t(0.0, 0.045, 0.0) * s(0.096, 0.085, 0.1), c);
            dl.solid(Mesh::Cone, m * t(0.0, 0.0, 0.0) * s(0.17, 0.03, 0.17), c);
        }
        3 => {
            let (lens, frame) = ([0.01, 0.01, 0.012], hex(0x222222));
            for sd in [-1.0f32, 1.0] {
                dl.push(Mesh::Cube, m * t(sd * 0.037, -0.058, 0.112) * ry(sd * 0.14) * s(0.058, 0.034, 0.01), [lens[0], lens[1], lens[2], 1.0], [kind::LIT, 0.0, 1.0, 0.0]);
                dl.solid(Mesh::Cube, m * t(sd * 0.074, -0.052, 0.06) * s(0.006, 0.008, 0.11), frame);
            }
            dl.solid(Mesh::Cube, m * t(0.0, -0.05, 0.116) * s(0.02, 0.006, 0.006), frame);
        }
        4 => {
            dl.solid(Mesh::Cyl, m * t(0.0, -0.012, 0.004) * s(0.1, 0.028, 0.112), hex(0xd43c3c));
            dl.solid(Mesh::Cyl, m * t(0.0, -0.012, 0.004) * s(0.101, 0.008, 0.113), hex(0xf2f2f2));
        }
        5 => {
            let gold = hex(0xe8b42a);
            dl.push(Mesh::Cyl, m * t(0.0, 0.02, 0.0) * s(0.078, 0.045, 0.078), [gold[0], gold[1], gold[2], 1.0], [kind::LIT, 0.15, 1.0, 0.0]);
            for i in 0..5 {
                let a = i as f32 / 5.0 * PI * 2.0;
                dl.push(Mesh::Cone, m * t(a.sin() * 0.072, 0.065, a.cos() * 0.072) * s(0.016, 0.05, 0.016), [gold[0], gold[1], gold[2], 1.0], [kind::LIT, 0.15, 1.0, 0.0]);
                dl.solid(Mesh::Sphere, m * t(a.sin() * 0.079, 0.02, a.cos() * 0.079) * su(0.009), hex(0xc8102e));
            }
        }
        _ => {}
    }
}
const HAT_Y: f32 = 0.2;
const HAT_Z: f32 = 0.01;

// ---------- props ----------
/// A beer crate with its open top showing: walls, handle slots and a dozen bottles. The opening is at +Y,
/// the bottom at y = 0; 40 by 30 cm and 30 cm high.
pub fn open_crate(dl: &mut DrawList, m: Mat4, col: [f32; 3]) {
    let dark = [col[0] * 0.55, col[1] * 0.55, col[2] * 0.55];
    const W: f32 = 0.018;
    dl.solid(Mesh::Cube, m * t(0.0, W * 0.5, 0.0) * s(0.4, W, 0.3), dark);
    for sd in [1.0f32, -1.0] {
        dl.solid(Mesh::Cube, m * t(sd * (0.2 - W * 0.5), 0.15, 0.0) * s(W, 0.3, 0.3), col);
        dl.solid(Mesh::Cube, m * t(0.0, 0.15, sd * (0.15 - W * 0.5)) * s(0.4, 0.3, W), col);
        dl.solid(Mesh::Cube, m * t(sd * 0.201, 0.24, 0.0) * s(0.004, 0.04, 0.12), dark);
    }
    // the bottles, necks up, seen from the front as a row of caps
    let glass = hex(0x4a2a12);
    for ix in 0..4 {
        for iz in 0..3 {
            let (x, z) = ((ix as f32 - 1.5) * 0.088, (iz as f32 - 1.0) * 0.086);
            dl.solid(Mesh::Cyl, m * t(x, 0.11, z) * s(0.034, 0.18, 0.034), glass);
            dl.solid(Mesh::Cyl, m * t(x, 0.225, z) * s(0.014, 0.06, 0.014), glass);
            dl.solid(Mesh::Cyl, m * t(x, 0.259, z) * s(0.016, 0.01, 0.016), [0.78, 0.72, 0.5]);
        }
    }
}
/// How much of a bottle's length is below the hand holding it, near the top.
pub const BOTTLE_GRIP: f32 = 0.13;
pub const BOTTLE_H: f32 = 0.25;
pub fn bottle(dl: &mut DrawList, m: Mat4, label: [f32; 3]) {
    // a half-litre beer bottle, its base at the origin: brown glass, a label, a long neck and a crown cap
    let glass = hex(0x4a2a12);
    dl.push(Mesh::Cyl, m * t(0.0, 0.085, 0.0) * s(0.032, 0.17, 0.032), [glass[0], glass[1], glass[2], 1.0], [kind::LIT, 0.05, 1.0, 0.0]);
    dl.solid(Mesh::Cyl, m * t(0.0, 0.085, 0.0) * s(0.0325, 0.085, 0.0325), label);
    dl.solid(Mesh::Cyl, m * t(0.0, 0.18, 0.0) * s(0.023, 0.03, 0.023), glass);
    dl.solid(Mesh::Cyl, m * t(0.0, 0.215, 0.0) * s(0.0135, 0.06, 0.0135), glass);
    dl.solid(Mesh::Cyl, m * t(0.0, 0.246, 0.0) * s(0.015, 0.008, 0.015), [0.8, 0.72, 0.45]);
}

/// A tree: a tapering trunk with a few boughs and a crown of leaf clumps around a dark core.
pub fn tree(dl: &mut DrawList, rng: &mut game::Rng, at: Vec3, h: f32, tr: f32, green: [f32; 3]) {
    let bark = [0.19 + rng.f() * 0.05, 0.15 + rng.f() * 0.03, 0.11];
    dl.push(Mesh::Trunk, Mat4::from_translation(at + Vec3::Y * (h * 0.5)) * s(tr, h, tr), [bark[0], bark[1], bark[2], 1.0], [kind::BARK, 0.0, 1.0, 0.0]);
    let crown = at + Vec3::Y * (h + 0.6);
    let cr = 1.7 + rng.f() * 0.9;
    let mut ends = Vec::new();
    let nb = 3 + (rng.f() * 2.0) as usize;
    for k in 0..nb {
        let y0 = h * (0.55 + rng.f() * 0.3);
        let yaw = k as f32 / nb as f32 * PI * 2.0 + rng.f() * 0.8;
        let tilt = 0.55 + rng.f() * 0.45;
        let len = cr * (0.75 + rng.f() * 0.35);
        let m = Mat4::from_translation(at + Vec3::Y * y0) * ry(yaw) * rz(tilt);
        dl.push(Mesh::Trunk, m * t(0.0, len * 0.5, 0.0) * s(tr * 0.42, len, tr * 0.42), [bark[0], bark[1], bark[2], 1.0], [kind::BARK, 0.0, 1.0, 0.0]);
        ends.push(m.transform_point3(Vec3::Y * len));
    }
    // a dark core so you never see through the crown
    let core = [green[0] * 0.3, green[1] * 0.32, green[2] * 0.25];
    dl.solid(Mesh::Sphere, Mat4::from_translation(crown) * s(cr * 0.55, cr * 0.45, cr * 0.55), core);
    let seed = rng.f() * 10.0;
    let mut clump = |dl: &mut DrawList, c: Vec3, sc: f32, rng: &mut game::Rng| {
        let k = 0.85 + rng.f() * 0.3;
        dl.push(Mesh::Leaves, Mat4::from_translation(c) * ry(rng.f() * 6.3) * s(sc, sc * 0.85, sc), [green[0] * k, green[1] * k, green[2] * k, 1.0], [kind::LEAF, 0.0, 1.0, seed]);
    };
    for e in &ends {
        clump(dl, *e + Vec3::Y * 0.2, 0.95 + rng.f() * 0.35, rng);
    }
    for _ in 0..(13 - ends.len().min(5)) {
        let d = vec3(rng.f() * 2.0 - 1.0, rng.f() * 1.6 - 0.5, rng.f() * 2.0 - 1.0).normalize_or_zero();
        clump(dl, crown + vec3(d.x * cr * 0.62, d.y * cr * 0.5, d.z * cr * 0.62), 1.0 + rng.f() * 0.5, rng);
    }
}
/// A bush: a few leaf clumps on a dark mound.
pub fn bush(dl: &mut DrawList, rng: &mut game::Rng, at: Vec3, sc: f32, green: [f32; 3]) {
    dl.solid(Mesh::Hemi, Mat4::from_translation(at) * s(sc * 1.1, sc * 0.8, sc * 0.95), [green[0] * 0.3, green[1] * 0.32, green[2] * 0.25]);
    let seed = rng.f() * 10.0;
    for _ in 0..3 {
        let o = vec3((rng.f() - 0.5) * sc, sc * (0.35 + rng.f() * 0.3), (rng.f() - 0.5) * sc * 0.8);
        let k = sc * (0.6 + rng.f() * 0.3);
        dl.push(Mesh::Leaves, Mat4::from_translation(at + o) * ry(rng.f() * 6.3) * s(k, k * 0.8, k), [green[0], green[1], green[2], 1.0], [kind::LEAF, 0.0, 1.0, seed]);
    }
}

/// Everything that never moves: sky, lawn, trees, paths, pond, benches.
pub fn build_env() -> DrawList {
    let mut dl = DrawList::default();
    let mut rng = game::Rng::new(1234);
    dl.push(Mesh::Sphere, su(220.0), [1.0; 4], [kind::SKY, 0.0, 0.0, 0.0]);
    dl.push(Mesh::Plane, t(0.0, 0.0, 0.0) * s(260.0, 1.0, 260.0), [1.0; 4], [kind::GRASS, 0.0, 0.0, 0.0]);
    // (the long path is painted into the lawn itself, see the lawn shader)
    let path = hex(0xb9a27e);
    for i in 0..10 {
        let x = -8.0 + i as f32 * 7.0;
        dl.push(Mesh::Cube, t(x, 0.005, 19.0 + (x * 0.09).sin() * 1.5) * s(7.3, 0.01, 2.2), [path[0], path[1], path[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    }
    // the pond
    dl.push(Mesh::Disc, t(20.0, 0.012, -22.0) * s(11.0, 1.0, 7.0), [0.10, 0.20, 0.22, 1.0], [kind::WATER, 0.0, 0.0, 0.0]);
    dl.push(Mesh::Disc, t(20.0, 0.008, -22.0) * s(11.6, 1.0, 7.6), [0.33, 0.29, 0.2, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    // (the blades growing on the lawn are made up on the GPU round the camera, see `gfx`)
    // trees, kept clear of the pitch and the paths
    let greens = [hex(0x2c5419), hex(0x3a6220), hex(0x284c1c), hex(0x456e24), hex(0x557024)];
    let mut placed = 0;
    let mut tries = 0;
    while placed < 85 && tries < 1200 {
        tries += 1;
        let ang = rng.f() * PI * 2.0;
        let r = 13.0 + rng.f() * rng.f() * 58.0;
        let (x, z) = (ang.sin() * r, ang.cos() * r);
        let px = -11.5 + (z * 0.07).sin() * 3.0;
        if (x - px).abs() < 2.8 || ((z - 19.0).abs() < 2.2 && x > -12.0) || ((x - 20.0) / 12.5).powi(2) + ((z + 22.0) / 8.5).powi(2) < 1.0 {
            continue;
        }
        placed += 1;
        let h = 3.2 + rng.f() * 2.8;
        let tr = 0.17 + rng.f() * 0.12;
        let g = greens[(rng.f() * 5.0) as usize % 5];
        tree(&mut dl, &mut rng, vec3(x, 0.0, z), h, tr, g);
    }
    // bushes along the edges
    for _ in 0..70 {
        let ang = rng.f() * PI * 2.0;
        let r = 12.5 + rng.f() * 38.0;
        let (x, z) = (ang.sin() * r, ang.cos() * r);
        if (x + 11.5 - (z * 0.07).sin() * 3.0).abs() < 2.6 {
            continue;
        }
        let sc = 0.6 + rng.f() * 0.7;
        let g = greens[(rng.f() * 5.0) as usize % 5];
        bush(&mut dl, &mut rng, vec3(x, 0.0, z), sc, g);
    }
    // flowers, stones and fallen leaves in the grass
    let petals = [hex(0xf2e14a), hex(0xe9e6df), hex(0xd8486a), hex(0x7a5ad0), hex(0xe9843a)];
    for _ in 0..90 {
        let ang = rng.f() * PI * 2.0;
        let r = 10.5 + rng.f() * 22.0;
        let (cx, cz) = (ang.sin() * r, ang.cos() * r);
        if (cx + 11.5 - (cz * 0.07).sin() * 3.0).abs() < 2.2 {
            continue;
        }
        let c = petals[(rng.f() * 5.0) as usize % 5];
        for _ in 0..5 {
            let (x, z) = (cx + (rng.f() - 0.5) * 0.8, cz + (rng.f() - 0.5) * 0.8);
            let h = 0.12 + rng.f() * 0.1;
            dl.solid(Mesh::Cyl, t(x, h * 0.5, z) * s(0.004, h, 0.004), hex(0x3b6a22));
            dl.solid(Mesh::Sphere, t(x, h, z) * s(0.028, 0.016, 0.028), c);
        }
    }
    for _ in 0..24 {
        let ang = rng.f() * PI * 2.0;
        let r = 11.0 + rng.f() * 30.0;
        let sc = 0.12 + rng.f() * 0.3;
        let g = 0.3 + rng.f() * 0.12;
        dl.solid(Mesh::Sphere, t(ang.sin() * r, sc * 0.3, ang.cos() * r) * ry(rng.f() * 6.0) * s(sc * 1.3, sc * 0.75, sc), [g, g, g * 0.95]);
    }
    // what people leave in a park: bikes against the trees, a blanket and a bag
    for k in 0..7 {
        let ang = 0.7 + k as f32 * 0.9;
        let r = 13.0 + (k as f32 * 1.7).sin().abs() * 9.0;
        let m = t(ang.sin() * r, 0.0, ang.cos() * r) * ry(ang * 2.3);
        let c = [hex(0x202226), hex(0x8a2020), hex(0x2a4a6a)][k % 3];
        for x in [-0.5f32, 0.5] {
            dl.solid(Mesh::Cyl, m * t(x, 0.33, 0.0) * rx(PI * 0.5) * s(0.33, 0.02, 0.33), hex(0x141416));
        }
        dl.solid(Mesh::Cube, m * t(0.0, 0.55, 0.0) * s(0.95, 0.03, 0.03), c);
        dl.solid(Mesh::Cube, m * t(-0.2, 0.42, 0.0) * rz(0.9) * s(0.5, 0.03, 0.03), c);
        dl.solid(Mesh::Cube, m * t(-0.25, 0.8, 0.0) * s(0.2, 0.05, 0.1), hex(0x1a1a1a));
        dl.solid(Mesh::Cube, m * t(0.45, 0.82, 0.0) * s(0.04, 0.03, 0.46), hex(0x8a8f94));
    }
    for (k, (x, z)) in [(14.0f32, 10.0f32), (-6.0, -18.5), (15.0, -9.0)].iter().enumerate() {
        let c = [hex(0xb8483a), hex(0x3a6ab8), hex(0xd8b04a)][k];
        dl.push(Mesh::Cube, t(*x, 0.01, *z) * ry(*x) * s(1.8, 0.012, 1.4), [c[0], c[1], c[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        open_crate(&mut dl, t(*x + 0.5, 0.0, *z + 0.3) * ry(*x), hex(0xc0392b));
    }
    // benches and lamp posts by the path
    for (i, z) in [-14.0f32, 3.0, 26.0].iter().enumerate() {
        let x = -11.5 + (z * 0.07).sin() * 3.0 + 2.0;
        let m = t(x, 0.0, *z) * ry(PI * 0.5 + (z * 0.07).cos() * 0.2);
        let wood = hex(0x7a5636);
        dl.solid(Mesh::Cube, m * t(0.0, 0.45, 0.0) * s(1.6, 0.05, 0.42), wood);
        dl.solid(Mesh::Cube, m * t(0.0, 0.75, -0.2) * rx(-0.15) * s(1.6, 0.32, 0.05), wood);
        for sd in [-0.7f32, 0.7] {
            dl.solid(Mesh::Cube, m * t(sd, 0.22, 0.0) * s(0.06, 0.45, 0.4), hex(0x2b2d2c));
        }
        let lx = x + 1.3;
        let lz = z + 3.0 + i as f32;
        dl.solid(Mesh::Cyl, t(lx, 1.7, lz) * s(0.05, 3.4, 0.05), hex(0x263328));
        dl.push(Mesh::Sphere, t(lx, 3.45, lz) * su(0.16), [1.0, 0.92, 0.75, 1.0], [kind::LIT, 0.6, 1.0, 0.0]);
    }
    // each team's stash behind their line: crates and a cooler
    for sd in [1.0f32, -1.0] {
        let z = sd * (game::LINE + 2.3);
        open_crate(&mut dl, t(3.3, 0.0, z) * ry(0.3 * sd), hex(if sd > 0.0 { 0xd9a826 } else { 0x2a6fa8 }));
        open_crate(&mut dl, t(3.25, 0.3, z) * ry(0.1 * sd), hex(0xc0392b));
        dl.solid(Mesh::Cube, t(-3.4, 0.16, z) * s(0.5, 0.32, 0.34), hex(0xe8e2d0));
        dl.solid(Mesh::Cube, t(-3.4, 0.335, z) * s(0.52, 0.04, 0.36), hex(if sd > 0.0 { 0xd43c3c } else { 0x2f86b8 }));
    }
    dl
}

// ---------- camera ----------
pub struct Cam {
    pub pos: Vec3,
    pub look: Vec3,
    pub fov: f32,
    pub aspect: f32,
}
impl Cam {
    pub fn view_proj(&self) -> Mat4 {
        Mat4::perspective_infinite_reverse_rh(self.fov, self.aspect, 0.05) * Mat4::look_at_rh(self.pos, self.look, Vec3::Y)
    }
    /// The point `depth` metres from the camera under screen position (x, y) in 0..1.
    pub fn screen_to_world(&self, x: f32, y: f32, depth: f32) -> Vec3 {
        let inv = self.view_proj().inverse();
        let p = inv.project_point3(vec3(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.5));
        self.pos + (p - self.pos).normalize() * depth
    }
    pub fn world_to_screen(&self, p: Vec3) -> Option<(f32, f32)> {
        let c = self.view_proj() * p.extend(1.0);
        if c.w <= 0.0 {
            return None;
        }
        Some((c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5))
    }
}

pub fn billboard(cam: Vec3, p: Vec3, size: f32) -> Mat4 {
    let f = (cam - p).normalize_or_zero();
    let r = Vec3::Y.cross(f).normalize_or(Vec3::X);
    let u = f.cross(r);
    Mat4::from_cols((r * size).extend(0.0), (u * size).extend(0.0), f.extend(0.0), p.extend(1.0))
}

pub struct SceneOpts {
    pub looks: [[Look; 3]; 2],
    pub drill: Option<game::Drill>,
    pub aim_ball: Option<Vec3>,
    pub aim_spin: f32,
    pub curve_armed: bool,
    pub ring_r: Option<f32>,
    pub hide_thrower: bool,
    pub trail: bool,
}

pub struct SceneOut {
    pub ref_mouth: Vec3,
    pub ref_tip: Vec3,
    pub heads: [[Vec3; 3]; 2],
}

const RING_C: [u32; 4] = [0, 0xff8a3d, 0xffd23f, 0x6fdc6f];
const CAN_COLS: [u32; 2] = [0x2f8f3a, 0xc8102e];

pub fn build(dl: &mut DrawList, model: &Model, g: &Game, cam: &Cam, o: &SceneOpts) -> SceneOut {
    let (ref_mouth, ref_tip, heads) = build_people(dl, model, g, cam, o);
    build_props(dl, g, cam, o);
    SceneOut { ref_mouth, ref_tip, heads }
}

fn build_people(dl: &mut DrawList, model: &Model, g: &Game, cam: &Cam, o: &SceneOpts) -> (Vec3, Vec3, [[Vec3; 3]; 2]) {
    // people
    let mut hand_of = [[Vec3::ZERO; 3]; 2];
    let mut mouth_of = [[Vec3::ZERO; 3]; 2];
    let mut heads = [[Vec3::ZERO; 3]; 2];
    // a drill shows only what it needs: your player (and in the running drill one thrower)
    let keep = |ti: usize, i: usize| match o.drill {
        None => true,
        Some(game::Drill::Run) => i == 0,
        Some(_) => ti == 0 && i == 0,
    };
    for ti in 0..2 {
        let tm = &g.teams[ti];
        for i in 0..3 {
            let p = &tm.players[i];
            let near = vec3(cam.pos.x - p.pos.x, 0.0, cam.pos.z - p.pos.z).length() < 0.75;
            let hidden = near || (o.hide_thrower && g.turn == ti && tm.idx == i) || !keep(ti, i);
            let lk = o.looks[ti][i];
            let rig = person(dl, model, p, &lk, p.visible && !hidden);
            hand_of[ti][i] = rig.hand_r;
            mouth_of[ti][i] = rig.mouth;
            heads[ti][i] = rig.head.transform_point3(vec3(0.0, 0.3, 0.0));
            if p.anim.emote > 0 && p.visible && !hidden {
                emote_prop(dl, p, &rig, cam, ti * 3 + i);
            }
        }
    }
    let show_ref = o.drill.is_none();
    let mut scratch = DrawList::default();
    let rr = person(if show_ref { &mut *dl } else { &mut scratch }, model, &g.referee, &ref_look(), true);
    let dl: &mut DrawList = if show_ref { dl } else { &mut scratch };
    // the referee's crate and cigarette
    // stood on a short end under his seat: the long side runs up, the opening faces where he faces
    let face = game::ref_face();
    let seat = game::REF_POS + vec3(face.sin(), 0.0, face.cos()) * crate::anim::SEAT_Z;
    open_crate(dl, t(seat.x, 0.0, seat.z) * ry(face) * t(0.0, 0.2, -0.15) * Mat4::from_mat3(glam::Mat3::from_cols(Vec3::Y, Vec3::Z, Vec3::X)), hex(0xe2b22a));
    let cig = rr.hand_r_m * t(0.0, -0.02, 0.04) * rx(PI * 0.5);
    let tip = cig.transform_point3(vec3(0.0, 0.037, 0.0));
    if let (true, Some((bt, _))) = (g.ref_hold, g.ref_bottle) {
        // the Strafbier, from the crate to the player: no cigarette in that hand now
        bottle(dl, t(rr.hand_r.x, rr.hand_r.y - BOTTLE_GRIP, rr.hand_r.z), hex(CAN_COLS[bt]));
    } else {
        dl.solid(Mesh::Cyl, cig * s(0.0048, 0.07, 0.0048), hex(0xf4f1ea));
        let glow = if g.referee.anim.drag() { 0.95 } else { 0.45 } + (g.clock * 9.3).sin() * 0.08;
        dl.add.push((Mesh::Quad, inst(billboard(cam.pos, tip, if g.referee.anim.drag() { 0.085 } else { 0.055 }), [1.0, 0.45, 0.12, glow], [kind::SOFT, 0.0, 0.0, 0.0])));
        dl.push(Mesh::Sphere, t(tip.x, tip.y, tip.z) * su(0.006), [1.0, 0.35, 0.08, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
    }
    let _ = &hand_of;
    HANDS.with(|h| *h.borrow_mut() = (hand_of, mouth_of));
    (rr.mouth, tip, heads)
}

thread_local! {
    static HANDS: std::cell::RefCell<([[Vec3; 3]; 2], [[Vec3; 3]; 2])> = const { std::cell::RefCell::new(([[Vec3::ZERO; 3]; 2], [[Vec3::ZERO; 3]; 2])) };
}

fn build_props(dl: &mut DrawList, g: &Game, cam: &Cam, o: &SceneOpts) {
    let (hand_of, mouth_of) = HANDS.with(|h| *h.borrow());
    let drink_only = o.drill == Some(game::Drill::Drink);

    // bottle: a 1.5 L PET bottle with some water in it, tipping about its base edge
    let td = g.tip_dir;
    let axis = vec3(td.z, 0.0, -td.x).normalize_or(Vec3::X);
    let pivot = td * R;
    let bm = Mat4::from_translation(pivot) * Mat4::from_quat(Quat::from_axis_angle(axis, g.theta)) * Mat4::from_translation(-pivot);
    // clear plastic you can see the water through; the water and cap are solid
    dl.blend.push((Mesh::Cyl, inst(bm * t(0.0, 0.11, 0.0) * s(R, 0.22, R), [0.82, 0.92, 1.0, 0.32], [kind::LIT, 0.15, 0.0, 0.0])));
    dl.blend.push((Mesh::Cone, inst(bm * t(0.0, 0.26, 0.0) * s(R, 0.08, R), [0.82, 0.92, 1.0, 0.32], [kind::LIT, 0.15, 0.0, 0.0])));
    dl.push(Mesh::Cyl, bm * t(0.0, 0.046, 0.0) * s(R * 0.93, 0.088, R * 0.93), [0.25, 0.5, 0.78, 1.0], [kind::LIT, 0.12, 1.0, 0.0]);
    dl.solid(Mesh::Cyl, bm * t(0.0, 0.302, 0.0) * s(0.015, 0.03, 0.015), hex(0xdfe8ee));
    dl.solid(Mesh::Cyl, bm * t(0.0, H - 0.003, 0.0) * s(0.016, 0.014, 0.016), hex(0x1f6fd1));
    dl.push(Mesh::Cyl, bm * t(0.0, 0.16, 0.0) * s(R * 1.015, 0.06, R * 1.015), [0.85, 0.2, 0.15, 1.0], [kind::LIT, 0.0, 1.0, 0.0]);

    // ball
    let mut ball = g.ball;
    if let Some((ti, i)) = g.holder {
        if !g.ball_free {
            ball = hand_of[ti][i] + vec3(0.0, -0.02, 0.0);
        }
    }
    let mut rot = g.ball_rot;
    if let Some(b) = o.aim_ball {
        ball = b;
        rot = Quat::from_rotation_z(-o.aim_spin);
    }
    if !drink_only {
    let em = if o.curve_armed { if (g.clock * 18.0).sin() > 0.0 { 0.6 } else { 0.35 } } else if g.ball_curve != 0.0 && g.ball_free { 0.25 } else { 0.0 };
    dl.push(Mesh::Sphere, Mat4::from_translation(ball) * Mat4::from_quat(rot) * su(BR), [0.55, 0.35, 0.0, 1.0], [kind::BALL, em, 1.0, 0.0]);
    }

    // beers: at their feet, in hand while sipping, knocked over where the ball hit them
    for ti in 0..2 {
        let tm = &g.teams[ti];
        let col = hex(CAN_COLS[ti]);
        for i in 0..3 {
            if o.drill.is_some() && !(drink_only && ti == 0 && i == 0) {
                continue;
            }
            let p = &tm.players[i];
            let v = tm.beers[i];
            // a finished beer leaves the pitch: it only stays in the hand for the last swallow
            let finishing = v <= 0.0 && p.visible && p.vis.w_sip > 0.02;
            if v <= 0.0 && !finishing {
                continue;
            }
            let mut n = ((v - 1e-6).ceil().max(1.0) as usize).min(4);
            // the Strafbier the referee hasn't brought yet
            if g.ref_bottle == Some((ti, i)) && n > 1 {
                n -= 1;
            }
            let sip = p.anim.sip && p.visible;
            for k in 0..n {
                if let Some((_, d)) = g.knocked.iter().find(|(key, _)| *key == (ti, i, k)) {
                    let cp = tm.can_pos(i, k);
                    let dir = vec3(d[0], 0.0, d[1]).normalize_or(Vec3::X);
                    let q = Quat::from_rotation_arc(Vec3::Y, dir);
                    bottle(dl, t(cp[0] + d[0] * 0.125, 0.033, cp[1] + d[1] * 0.125) * Mat4::from_quat(q) * t(0.0, -BOTTLE_H * 0.5, 0.0), col);
                    continue;
                }
                // the first bottle is in the hand while the team drinks (or sneaks a sip), and goes up to the lips
                // close to the grab it slides from the ground into the hand, whatever exact spot the hand is at
                let grab = ((p.vis.carry_t - (crate::anim::CAN_GRAB - 0.06)) / 0.08).clamp(0.0, 1.0);
                let in_hand = k == 0 && p.visible && (finishing || (v > 0.0 && grab > 0.0));
                if in_hand && !(o.hide_thrower && g.turn == ti && tm.idx == i) {
                    let hand = hand_of[ti][i];
                    let to_mouth = (mouth_of[ti][i] + vec3(0.0, 0.03, 0.0) - hand).normalize_or(Vec3::Y);
                    let up = Vec3::Y.lerp(to_mouth, p.vis.sip_k()).normalize_or(Vec3::Y);
                    let q = Quat::from_rotation_arc(Vec3::Y, up);
                    let cp = tm.can_pos(i, k);
                    let held = hand - up * BOTTLE_GRIP;
                    let at = if finishing { held } else { vec3(cp[0], 0.0, cp[1]).lerp(held, grab * grab * (3.0 - 2.0 * grab)) };
                    bottle(dl, Mat4::from_translation(at) * Mat4::from_quat(q), col);
                    continue;
                }
                let cp = tm.can_pos(i, k);
                let m = t(cp[0], 0.0, cp[1]) * ry(k as f32 * 0.9 + i as f32);
                bottle(dl, m, col);
            }
        }
    }

    // the referee's gaze on the lawn
    if g.gaze_op > 0.01 && o.drill.is_none() {
        let m = t(g.referee.pos.x, 0.035, g.referee.pos.z) * ry(g.ref_head_yaw());
        dl.add.push((Mesh::Fan, inst(m, [g.gaze_col[0], g.gaze_col[1], g.gaze_col[2], g.gaze_op], [kind::GAZE, 0.0, 0.0, 0.0])));
    }
    // aim rings round the bottle
    if let Some(r) = o.ring_r {
        dl.blend.push((Mesh::RingThin, inst(t(0.0, 0.03, 0.0), [1.0, 1.0, 1.0, 0.7], [kind::UNLIT, 0.0, 0.0, 0.0])));
        let c = hex(RING_C[game::ring_idx(r) as usize]);
        dl.blend.push((Mesh::RingThick, inst(t(0.0, 0.031, 0.0) * s(r, 1.0, r), [c[0], c[1], c[2], 0.85], [kind::UNLIT, 0.0, 0.0, 0.0])));
    }
    // smoke
    for p in g.smoke.iter().filter(|_| o.drill.is_none()) {
        let k = 1.0 - p.life / p.max;
        let size = p.s0 + (p.s1 - p.s0) * k.sqrt();
        let a = p.o * (1.0 - k) * (k * 8.0).min(1.0);
        dl.blend.push((Mesh::Quad, inst(billboard(cam.pos, p.pos, size * 2.0), [0.87, 0.89, 0.91, a], [kind::SOFT, 0.0, 0.0, 1.0])));
    }
    // beer splashes
    for sp in &g.splash {
        dl.push(Mesh::Sphere, t(sp.pos.x, sp.pos.y, sp.pos.z) * su(0.012 + sp.life * 0.01), [0.95, 0.75, 0.25, 1.0], [kind::LIT, 0.3, 0.0, 0.0]);
    }
    // ball trail
    if o.trail && g.state == State::Flight {
        let n = g.trail.len();
        for (i, p) in g.trail.iter().enumerate() {
            let k = (i + 1) as f32 / n.max(1) as f32;
            dl.add.push((Mesh::Quad, inst(billboard(cam.pos, *p, 0.09 * k + 0.02), [1.0, 0.95, 0.8, 0.35 * k], [kind::SOFT, 0.0, 0.0, 0.0])));
        }
    }
}

/// The clubhouse: your player standing in the shed.
pub fn build_hub_player(dl: &mut DrawList, model: &Model, p: &Person, lk: &Look) {
    person(dl, model, p, lk, true);
}

#[cfg(test)]
mod pose_tests {
    use super::*;
    use crate::game::{AMode, Person};
    fn rig_after(p: &mut Person, secs: f32, speed: f32) -> Rig {
        let m = Model::load(include_bytes!("../assets/person.glb")).unwrap();
        p.vis.treadmill = speed;
        for _ in 0..(secs * 60.0) as usize {
            if p.anim.throw_t >= 0.0 {
                p.anim.throw_t += 1.0 / 60.0;
            }
            crate::anim::animate(p, 1.0 / 60.0, None, 0.3);
        }
        let mut dl = DrawList::default();
        person(&mut dl, &m, p, &ref_look(), true)
    }
    #[test]
    fn poses_touch_the_world() {
        // standing: both feet on the floor
        let mut p = Person::new(Vec3::ZERO, 0.0);
        let r = rig_after(&mut p, 1.0, 0.0);
        println!("idle feet {:?}", r.feet);
        for f in r.feet {
            assert!(f.y.abs() < 0.05, "idle foot off the floor {f:?}");
        }
        // drinking: the hand comes to the mouth
        let mut p = Person::new(Vec3::ZERO, 0.0);
        p.anim.sip = true;
        let r = rig_after(&mut p, 1.5, 0.0);
        println!("sip hand {:?} mouth {:?} dist {}", r.hand_r, r.mouth, r.hand_r.distance(r.mouth));
        // squat: feet stay down
        let mut p = Person::new(Vec3::ZERO, 0.0);
        p.anim.mode = AMode::Bend;
        let r = rig_after(&mut p, 1.0, 0.0);
        println!("bend feet {:?} hand {:?}", r.feet, r.hand_r);
        // sitting on the referee's crate
        let mut p = Person::new(Vec3::ZERO, 0.0);
        p.anim.mode = AMode::Sit;
        let r = rig_after(&mut p, 2.0, 0.0);
        println!("sit feet {:?}", r.feet);
        assert!(r.feet.iter().all(|f| f.y < 0.09), "seated feet stay on the ground: {:?}", r.feet);
        // the seat of his shorts rests exactly on the top of the crate
        let m = Model::load(include_bytes!("../assets/person.glb")).unwrap();
        let mut dl = DrawList::default();
        person(&mut dl, &m, &p, &ref_look(), true);
        let bones = &dl.skinned[0].bones;
        use glam::Vec4;
        let mut low = f32::INFINITY;
        for v in &m.verts {
            let at = Vec3::from_array(v.pos).extend(1.0);
            let mut w = Vec4::ZERO;
            for k in 0..4 {
                w += bones[v.joints[k] as usize] * at * v.weights[k];
            }
            // only what is over the crate
            if w.x.abs() < 0.15 && (w.z - crate::anim::SEAT_Z).abs() < 0.15 {
                low = low.min(w.y);
            }
        }
        println!("lowest point over the crate: {low}");
        assert!((low - 0.4).abs() < 0.001, "sits at {low}, the crate's top is at 0.4");
        // bending down for the can: the hand gets near the can in front of the feet
        let mut p = Person::new(Vec3::ZERO, 0.0);
        p.anim.carry = true;
        rig_after(&mut p, crate::anim::CAN_GRAB * 0.95, 0.0);
        let r = rig_after(&mut p, 0.0, 0.0);
        let d = r.hand_r.distance(vec3(0.0, 0.1, 0.42));
        println!("reach hand {:?}, {d} from the can", r.hand_r);
        assert!(d < 0.28, "hand {:?} is {d} from the can", r.hand_r);
        // walking: one foot is always on the floor
        let mut p = Person::new(Vec3::ZERO, 0.0);
        for k in 0..6 {
            let r = rig_after(&mut p, if k == 0 { 1.0 } else { 0.12 }, 1.4);
            println!("walk feet {:?} hip {:?}", r.feet, p.vis.pose.hip);
        }
        let mut p = Person::new(Vec3::ZERO, 0.0);
        for k in 0..6 {
            let r = rig_after(&mut p, if k == 0 { 1.0 } else { 0.06 }, 4.8);
            println!("run feet {:?} hip {:?}", r.feet, p.vis.pose.hip);
        }
        for tt in [0.24f32, 0.38, 0.56] {
            let mut p = Person::new(Vec3::ZERO, 0.0);
            p.anim.throw_t = 0.0;
            let r = rig_after(&mut p, tt, 0.0);
            println!("throw t={tt} hand {:?}", r.hand_r);
        }
    }
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use crate::game::Sfx;
    #[test]
    fn frame_cost_around_a_hit() {
        let m = Model::load(include_bytes!("../assets/person.glb")).unwrap();
        let mut g = Game::new(7);
        g.start_match(game::Mode::Demo, 1);
        let looks = [[player_look([0.9, 0.7, 0.1], 0, 3), player_look([0.9, 0.7, 0.1], 1, 3), player_look([0.9, 0.7, 0.1], 2, 3)], [player_look([0.2, 0.6, 0.9], 0, 3), player_look([0.2, 0.6, 0.9], 1, 3), player_look([0.2, 0.6, 0.9], 2, 3)]];
        let cam = Cam { pos: vec3(0.0, 3.0, 8.0), look: Vec3::ZERO, fov: 1.0, aspect: 0.5 };
        let mut worst: Vec<(f64, usize, String)> = Vec::new();
        let mut hits = Vec::new();
        let (mut mouth, mut tip) = (Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.0, 1.0, 0.0));
        for f in 0..60 * 120 {
            let t0 = std::time::Instant::now();
            g.tick(1.0 / 60.0, mouth, tip);
            let t1 = t0.elapsed().as_secs_f64() * 1000.0;
            for ti in 0..2 {
                for i in 0..3 {
                    crate::anim::animate(&mut g.teams[ti].players[i], 1.0 / 60.0, Some(g.ball), 0.3);
                }
            }
            crate::anim::animate(&mut g.referee, 1.0 / 60.0, None, 2.9);
            let t2 = t0.elapsed().as_secs_f64() * 1000.0;
            let mut dl = DrawList::default();
            let o = SceneOpts { looks: looks.clone(), drill: None, aim_ball: None, aim_spin: 0.0, curve_armed: false, ring_r: None, hide_thrower: false, trail: true };
            let out = build(&mut dl, &m, &g, &cam, &o);
            mouth = out.ref_mouth;
            let _ = &mut tip;
            let t3 = t0.elapsed().as_secs_f64() * 1000.0;
            if g.sfx.iter().any(|s| matches!(s, Sfx::Hit)) {
                hits.push(f);
            }
            g.sfx.clear();
            worst.push((t3, f, format!("tick {t1:.2} anim {:.2} build {:.2} n={} state={:?}", t2 - t1, t3 - t2, dl.opaque.len() + dl.blend.len(), g.state)));
        }
        worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        println!("hits at frames {hits:?}");
        for w in worst.iter().take(12) {
            println!("{:.2} ms frame {} {}", w.0, w.1, w.2);
        }
        let avg: f64 = worst.iter().map(|w| w.0).sum::<f64>() / worst.len() as f64;
        println!("avg {avg:.3} ms");
    }
}
