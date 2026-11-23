//! Character animation. Instead of picking a pose per frame, every person keeps a little state:
//! a smoothed facing, the speed they are really moving at, a gait phase driven by distance travelled
//! (so feet don't skate), and blend weights that fade poses in and out over realistic times.
//! Feet are placed on the ground and the legs are solved with two-bone IK in `scene::person`.

use crate::game::{AMode, Person};
use crate::icons;
use glam::{vec3, Vec3};
use std::f32::consts::{PI, TAU};

pub const STAND_HIP: f32 = 0.794; // standing hip height of the model, metres
pub const ANKLE: f32 = 0.02;
pub const HIP_X: f32 = 0.119;

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// pelvis position (person-local, metres): sway, height, forward
    pub hip: Vec3,
    pub pel_yaw: f32,
    pub pel_roll: f32,
    pub lean: f32,
    pub twist: f32,
    pub side: f32,
    pub head_pitch: f32,
    pub head_yaw: f32,
    /// arms: [left, right] × (forward, outward, elbow)
    pub arm: [[f32; 3]; 2],
    /// ankle targets on (or above) the ground, person-local: [left, right]
    pub foot: [Vec3; 2],
}
impl Default for Pose {
    fn default() -> Self {
        Pose {
            hip: vec3(0.0, STAND_HIP - 0.01, 0.0),
            pel_yaw: 0.0,
            pel_roll: 0.0,
            lean: 0.02,
            twist: 0.0,
            side: 0.0,
            head_pitch: 0.0,
            head_yaw: 0.0,
            arm: [[0.05, 0.1, 0.15]; 2],
            foot: [vec3(HIP_X - 0.01, ANKLE, 0.04), vec3(-(HIP_X - 0.01), ANKLE, 0.04)],
        }
    }
}
fn mix(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}
impl Pose {
    pub fn lerp(&self, o: &Pose, k: f32) -> Pose {
        if k <= 0.0 {
            return *self;
        }
        if k >= 1.0 {
            return *o;
        }
        let mut arm = self.arm;
        for s in 0..2 {
            for c in 0..3 {
                arm[s][c] = mix(self.arm[s][c], o.arm[s][c], k);
            }
        }
        Pose {
            hip: self.hip.lerp(o.hip, k),
            pel_yaw: mix(self.pel_yaw, o.pel_yaw, k),
            pel_roll: mix(self.pel_roll, o.pel_roll, k),
            lean: mix(self.lean, o.lean, k),
            twist: mix(self.twist, o.twist, k),
            side: mix(self.side, o.side, k),
            head_pitch: mix(self.head_pitch, o.head_pitch, k),
            head_yaw: mix(self.head_yaw, o.head_yaw, k),
            arm,
            foot: [self.foot[0].lerp(o.foot[0], k), self.foot[1].lerp(o.foot[1], k)],
        }
    }
    fn arm_r(&mut self, a: [f32; 3], k: f32) {
        for c in 0..3 {
            self.arm[1][c] = mix(self.arm[1][c], a[c], k);
        }
    }
}

/// Animation state kept per person (render only).
#[derive(Clone, Debug)]
pub struct Vis {
    pub init: bool,
    pub yaw: f32,
    yaw_v: f32,
    last: Vec3,
    pub speed: f32,
    accel: f32,
    gait: f32,
    w_bend: f32,
    pub w_sit: f32,
    w_cheer: f32,
    w_hold: f32,
    w_point: f32,
    w_reach: f32,
    w_drag: f32,
    w_smoke: f32,
    pub w_sip: f32,
    /// 0: the can is on the ground. Up to CAN_GRAB they bend down for it, after that it is in the hand.
    pub carry_t: f32,
    sip_t: f32,
    look: [f32; 2],
    t: f32,
    seed: f32,
    pub pose: Pose,
    /// test hook: animate as if moving at this speed while standing still
    pub treadmill: f32,
}
impl Vis {
    /// How far the can is up to the lips; it has to be picked up first.
    pub fn sip_k(&self) -> f32 {
        smooth(self.w_sip) * smooth((self.carry_t - CAN_GRAB) / 0.3)
    }
}
impl Default for Vis {
    fn default() -> Self {
        Vis { init: false, yaw: 0.0, yaw_v: 0.0, last: Vec3::ZERO, speed: 0.0, accel: 0.0, gait: 0.0, w_bend: 0.0, w_sit: 0.0, w_cheer: 0.0, w_hold: 0.0, w_point: 0.0, w_reach: 0.0, w_drag: 0.0, w_smoke: 0.0, w_sip: 0.0, carry_t: 0.0, sip_t: 0.0, look: [0.0; 2], t: 0.0, seed: 0.0, pose: Pose::default(), treadmill: 0.0 }
    }
}

/// Move `x` toward `to` at `up` per second when rising and `down` per second when falling.
fn approach(x: &mut f32, to: f32, up: f32, down: f32, dt: f32) {
    if to > *x {
        *x = (*x + up * dt).min(to);
    } else {
        *x = (*x - down * dt).max(to);
    }
}
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn wrap(mut d: f32) -> f32 {
    while d > PI {
        d -= TAU;
    }
    while d < -PI {
        d += TAU;
    }
    d
}
/// Hermite through keyframes (t, value), flat at the ends.
fn keys(t: f32, k: &[(f32, f32)]) -> f32 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            return mix(a.1, b.1, smooth((t - a.0) / (b.0 - a.0)));
        }
    }
    k[k.len() - 1].1
}

/// What players can do just for show: (name, icon, seconds).
pub const EMOTES: [(&str, &str, f32); 8] = [("Smoke", icons::CIGARETTE, 6.0), ("Buzzball", icons::BUZZBALL, 4.5), ("Dance", icons::DANCE, 5.2), ("Wave", icons::WAVE, 2.5), ("Flex", icons::FLEX, 3.2), ("Facepalm", icons::FACEPALM, 2.8), ("Bow", icons::BOW, 2.6), ("Clap", icons::CLAP, 3.0)];
pub const EMOTE_SMOKE: u8 = 0;
pub const EMOTE_BUZZ: u8 = 1;
/// How far a prop is up to the mouth during an emote that has one.
pub fn emote_lift(i: u8, t: f32) -> f32 {
    let dur = EMOTES[i as usize % EMOTES.len()].2;
    let env = smooth(t / 0.35) * (1.0 - smooth((t - (dur - 0.45)) / 0.45));
    let per = if i == EMOTE_SMOKE { 2.6 } else { 2.0 };
    let u = t % per / per;
    env * smooth(u / 0.18) * (1.0 - smooth((u - 0.55) / 0.2))
}

/// How far through picking up (`Vis::carry_t`) the hand reaches the can on the ground.
pub const CAN_GRAB: f32 = 0.42;

/// Release happens this far into the throw animation.
pub const THROW_RELEASE: f32 = 0.38;
pub const THROW_LEN: f32 = 1.05;

/// Advance one person's animation. `look_at` makes them follow something (the ball) with their head.
pub fn animate(p: &mut Person, dt: f32, look_at: Option<Vec3>, seed: f32) {
    // an emote stops as soon as the match wants them for something
    if p.anim.emote > 0 {
        let a = &mut p.anim;
        a.emote_t += dt;
        let dur = EMOTES[(a.emote - 1) as usize % EMOTES.len()].2;
        if a.emote_t > dur || a.mode != AMode::Idle || a.throw_t >= 0.0 || a.carry || p.vis.speed > 0.6 {
            a.emote = 0;
        }
    }
    let v = &mut p.vis;
    let flat = vec3(p.pos.x, 0.0, p.pos.z);
    if !v.init || flat.distance(v.last) > 1.2 {
        // first frame or a teleport (new match): no gait, no turning from the old spot
        v.init = true;
        v.last = flat;
        v.yaw = p.yaw;
        v.yaw_v = 0.0;
        v.speed = 0.0;
        v.seed = seed;
        v.w_sit = if p.anim.mode == AMode::Sit { 1.0 } else { 0.0 };
    }
    let dt = dt.max(1e-4);
    v.t += dt;
    // how fast they really move, and a stride phase tied to distance (feet stay planted)
    let moved = if v.treadmill > 0.0 { v.treadmill * dt } else { flat.distance(v.last) };
    v.last = flat;
    let inst = (moved / dt).min(9.0);
    let prev = v.speed;
    v.speed += (inst - v.speed) * (1.0 - (-10.0 * dt).exp());
    v.accel += (((v.speed - prev) / dt).clamp(-12.0, 12.0) - v.accel) * (1.0 - (-6.0 * dt).exp());
    let spd = v.speed;
    let step = (0.5 + 0.17 * spd).clamp(0.55, 1.35);
    v.gait = (v.gait + PI * moved / step) % TAU;
    // turning: a critically damped spring, quick but never instant
    let w = 11.0;
    let d = wrap(p.yaw - v.yaw);
    v.yaw_v += (w * w * d - 2.0 * w * v.yaw_v) * dt;
    v.yaw = wrap(v.yaw + v.yaw_v * dt);
    // pose weights fade in and out over human timescales
    let a = &p.anim;
    approach(&mut v.w_bend, (a.mode == AMode::Bend) as u8 as f32, 3.2, 2.6, dt);
    approach(&mut v.w_sit, (a.mode == AMode::Sit) as u8 as f32, 1.6, 1.9, dt);
    approach(&mut v.w_cheer, (a.mode == AMode::Cheer) as u8 as f32, 3.0, 2.0, dt);
    approach(&mut v.w_sip, a.sip as u8 as f32, 2.8, 2.2, dt);
    approach(&mut v.w_hold, a.hold as u8 as f32, 3.0, 4.0, dt);
    approach(&mut v.w_point, a.point as u8 as f32, 2.5, 2.0, dt);
    approach(&mut v.w_reach, a.reach as u8 as f32, 2.6, 2.6, dt);
    approach(&mut v.w_drag, a.drag() as u8 as f32, 1.6, 1.4, dt);
    approach(&mut v.w_smoke, a.smoking as u8 as f32, 1.5, 1.5, dt);
    approach(&mut v.carry_t, a.carry as u8 as f32, 1.0 / 0.95, 1.0 / 0.85, dt);
    v.sip_t = if a.sip { v.sip_t + dt } else { (v.sip_t - dt * 2.0).max(0.0) };
    // head: follow the ball, or (the referee) look where the game says he looks
    let (ly, lp) = match look_at {
        Some(t) => {
            let dx = t.x - p.pos.x;
            let dz = t.z - p.pos.z;
            let yaw = wrap(dx.atan2(dz) - v.yaw).clamp(-1.15, 1.15);
            let dist = (dx * dx + dz * dz).sqrt().max(0.3);
            let pitch = (-(t.y - 1.55).atan2(dist)).clamp(-0.5, 0.6);
            (yaw, pitch)
        }
        None => (a.look, 0.0),
    };
    v.look[0] += (ly - v.look[0]) * (1.0 - (-5.0 * dt).exp());
    v.look[1] += (lp - v.look[1]) * (1.0 - (-4.0 * dt).exp());
    let pose = build(p);
    p.vis.pose = pose;
}

// ---------- the poses ----------
fn idle(v: &Vis) -> Pose {
    let mut o = Pose::default();
    let t = v.t + v.seed * 7.0;
    // weight shifts slowly from one leg to the other; breathing lifts the chest
    let shift = (t * 0.45).sin() * 0.7 + (t * 0.17).sin() * 0.3;
    let br = (t * 1.5).sin();
    o.hip.x = 0.022 * shift;
    o.hip.y = STAND_HIP - 0.012 - 0.008 * shift.abs();
    o.pel_roll = -0.05 * shift;
    o.side = 0.035 * shift;
    o.lean = 0.02 + 0.008 * br;
    // the free leg rests a little forward and out
    let free = if shift > 0.0 { 1 } else { 0 };
    o.foot[free].z += 0.07 * shift.abs();
    o.foot[free].x += if free == 0 { 0.03 } else { -0.03 } * shift.abs();
    for s in 0..2 {
        let ph = if s == 0 { 0.0 } else { 1.3 };
        o.arm[s] = [0.06 + 0.02 * (t * 0.8 + ph).sin(), 0.09 + 0.015 * br, 0.16 + 0.05 * (t * 0.6 + ph).sin()];
    }
    o.head_pitch = 0.04 * (t * 0.31).sin();
    o
}

/// Walking blends into running with speed; feet follow stance and swing arcs.
fn locomotion(v: &Vis, base: &Pose) -> Pose {
    let mut o = *base;
    let spd = v.speed;
    let r = smooth((spd - 2.0) / 1.6); // 0 walk … 1 run
    let step = (0.5 + 0.17 * spd).clamp(0.55, 1.35);
    let duty = mix(0.62, 0.36, r);
    let lift = mix(0.07, 0.24, r);
    let ul = v.gait / TAU; // left leg phase
    let foot_at = |u: f32| -> (f32, f32) {
        let u = u.rem_euclid(1.0);
        // runners land nearly under the hips and push off far behind; walkers reach out front
        let front = step * 0.5 * mix(1.0, 0.5, r);
        let back = -step * 0.5 * mix(1.0, 1.15, r);
        if u < duty {
            let s = u / duty;
            (mix(front, back, s), 0.0)
        } else {
            let s = (u - duty) / (1.0 - duty);
            // swing: heel comes up behind first (more so when running), then the leg reaches forward
            let z = mix(back, front, smooth(s));
            let y = lift * (PI * s).sin() + r * 0.12 * (PI * s * 1.6).sin().max(0.0) * (1.0 - s);
            (z, y)
        }
    };
    let (zl, yl) = foot_at(ul);
    let (zr, yr) = foot_at(ul + 0.5);
    o.foot[0] = vec3(HIP_X - 0.02, ANKLE + yl, 0.04 + zl);
    o.foot[1] = vec3(-(HIP_X - 0.02), ANKLE + yr, 0.04 + zr);
    let swing = (zl - zr) / step.max(0.1); // −1 … 1, left foot forward = +
    // the hips drop a little and bounce: lowest at double support when walking, highest in flight when running
    let walk_b = 0.016 * (4.0 * PI * ul).cos();
    let run_b = 0.04 * (4.0 * PI * ul + PI).cos();
    o.hip = vec3(0.012 * (2.0 * PI * ul).sin() * (1.0 - r), STAND_HIP - 0.02 - 0.03 * r + mix(walk_b, run_b, r), 0.0);
    o.pel_yaw = -0.11 * swing * (1.0 - 0.4 * r);
    o.pel_roll = 0.03 * (2.0 * PI * ul).sin() * (1.0 - r);
    o.twist = 0.16 * swing * (1.0 - 0.3 * r);
    o.side = 0.0;
    o.lean = 0.04 + 0.04 * spd + (v.accel * 0.03).clamp(-0.12, 0.18);
    let amp = 0.25 + 0.12 * spd;
    let elbow = mix(0.35, 1.45, r);
    o.arm[0] = [-amp * swing + 0.05, 0.1 + 0.04 * r, elbow + 0.15 * swing.max(0.0)];
    o.arm[1] = [amp * swing + 0.05, 0.1 + 0.04 * r, elbow + 0.15 * (-swing).max(0.0)];
    o.head_pitch = -0.5 * o.lean + 0.05;
    o
}

fn bend(base: &Pose) -> Pose {
    // squat and hinge to reach the ground; the feet stay where they are
    let mut o = *base;
    o.hip = vec3(0.0, 0.6, -0.16);
    o.lean = 1.0;
    o.pel_roll = 0.0;
    o.pel_yaw = 0.0;
    o.twist = 0.0;
    o.foot = [vec3(HIP_X + 0.02, ANKLE, 0.05), vec3(-(HIP_X + 0.02), ANKLE, 0.0)];
    o.arm = [[1.25, 0.12, 0.35], [1.4, 0.06, 0.2]];
    o.head_pitch = -0.45;
    o
}

/// Where the middle of a sitter's seat is, behind their feet's reference point (see the referee's crate).
pub const SEAT_Z: f32 = -0.05;
/// The hip joint's height for sitting on an upright crate (40 cm), measured on the model: see `poses_touch_the_world`.
const SEAT_HIP: f32 = 0.4898;

fn sit(v: &Vis) -> Pose {
    // on the crate, forearms on the knees, feet planted in front
    let mut o = Pose::default();
    let t = v.t + v.seed * 5.0;
    o.hip = vec3(0.0, SEAT_HIP, SEAT_Z); // never bobbing: the crate doesn't give
    o.lean = 0.14 + 0.01 * (t * 1.5).sin();
    o.foot = [vec3(HIP_X + 0.05, ANKLE, 0.38), vec3(-(HIP_X + 0.05), ANKLE, 0.34)];
    o.arm = [[0.75, 0.15, 1.15], [0.7, 0.15, 1.1]];
    o.head_pitch = -0.18;
    o
}

/// Down on one knee's worth of squat, right hand at the can in front of the feet.
fn reach_down(base: &Pose) -> Pose {
    let mut o = *base;
    o.hip = vec3(0.0, 0.4, -0.14);
    o.lean = 0.82;
    o.pel_roll = 0.0;
    o.pel_yaw = 0.0;
    o.twist = -0.12;
    o.foot = [vec3(HIP_X + 0.03, ANKLE, 0.09), vec3(-(HIP_X + 0.03), ANKLE, -0.02)];
    o.arm = [[0.5, 0.2, 0.5], [0.95, -0.02, 0.08]];
    o.head_pitch = -0.4;
    o
}

fn cheer(v: &Vis) -> Pose {
    // little jumps: crouch, push off, fly, land soft; arms pumping
    let mut o = Pose::default();
    let per = 0.85;
    let u = ((v.t + v.seed * 3.0) % per) / per;
    let (dip, air) = if u < 0.22 {
        (0.09 * smooth(u / 0.22), 0.0)
    } else if u < 0.6 {
        let s = (u - 0.22) / 0.38;
        (0.0, 0.17 * 4.0 * s * (1.0 - s))
    } else {
        let s = (u - 0.6) / 0.4;
        (0.11 * (PI * s).sin(), 0.0)
    };
    o.hip.y = STAND_HIP - 0.012 - dip + air;
    o.lean = 0.12 * dip / 0.1;
    for f in o.foot.iter_mut() {
        f.y = ANKLE + air * 0.85;
    }
    let pump = (u * TAU).sin();
    o.arm = [[2.75 + 0.2 * pump, 0.35, 0.3 - 0.2 * pump], [2.75 - 0.2 * pump, 0.35, 0.3 + 0.2 * pump]];
    o.head_pitch = -0.25;
    o
}

/// Underarm lob: wind-up, step in with the other foot, release, follow through.
fn throw(o: &mut Pose, t: f32) {
    let env = smooth(t / 0.08) * (1.0 - smooth((t - 0.8) / (THROW_LEN - 0.8)));
    if env <= 0.0 {
        return;
    }
    let r = THROW_RELEASE;
    let arm = keys(t, &[(0.0, 0.1), (0.24, -0.95), (r, 1.05), (0.56, 1.95), (0.8, 1.2), (THROW_LEN, 0.1)]);
    let elbow = keys(t, &[(0.0, 0.25), (0.24, 0.2), (r, 0.12), (0.56, 0.35), (THROW_LEN, 0.2)]);
    let twist = keys(t, &[(0.0, 0.0), (0.24, 0.32), (r, -0.12), (0.56, -0.3), (THROW_LEN, 0.0)]);
    let lean = keys(t, &[(0.0, 0.03), (0.24, 0.12), (r, 0.22), (0.56, 0.28), (THROW_LEN, 0.03)]);
    let hip_z = keys(t, &[(0.0, 0.0), (0.24, -0.06), (r, 0.07), (0.56, 0.1), (0.8, 0.06), (THROW_LEN, 0.0)]);
    let dip = keys(t, &[(0.0, 0.0), (0.24, 0.05), (r, 0.03), (0.56, 0.02), (THROW_LEN, 0.0)]);
    // the left foot steps forward during the wind-up and stays there through the release
    let step = keys(t, &[(0.0, 0.0), (0.12, 0.0), (0.3, 0.34), (0.8, 0.34), (THROW_LEN, 0.0)]);
    let step_lift = keys(t, &[(0.0, 0.0), (0.12, 0.0), (0.21, 0.07), (0.3, 0.0), (0.82, 0.0), (0.93, 0.05), (THROW_LEN, 0.0)]);
    o.arm_r([arm, 0.14, elbow], env);
    o.arm[0][0] = mix(o.arm[0][0], -0.35 * (arm / 2.0), env);
    o.twist = mix(o.twist, twist, env);
    o.lean = mix(o.lean, lean, env);
    o.hip.z = mix(o.hip.z, hip_z, env);
    o.hip.y = mix(o.hip.y, o.hip.y - dip, env);
    o.foot[0].z = mix(o.foot[0].z, 0.04 + step, env);
    o.foot[0].y = mix(o.foot[0].y, ANKLE + step_lift, env);
    o.head_pitch = mix(o.head_pitch, 0.1, env);
}

fn emote_pose(o: &mut Pose, i: u8, t: f32, clock: f32) {
    let dur = EMOTES[i as usize % EMOTES.len()].2;
    let k = smooth(t / 0.3) * (1.0 - smooth((t - (dur - 0.35)) / 0.35));
    if k <= 0.0 {
        return;
    }
    let base = *o;
    let mut e = base;
    match i {
        EMOTE_SMOKE | EMOTE_BUZZ => {
            // something in the right hand, up to the lips now and then; the other hand on the hip
            let lift = emote_lift(i, t);
            e.arm[1] = [0.42, 0.14, 1.35];
            e.arm[0] = [0.15, 0.42, 1.0];
            e.arm[1] = [mix(0.42, 1.15, lift), mix(0.14, 0.6, lift), mix(1.35, 2.3, lift)];
            e.head_pitch = mix(-0.02, if i == EMOTE_BUZZ { -0.42 } else { -0.14 }, lift);
            e.lean = base.lean - 0.03 * lift;
            e.hip.x = base.hip.x + 0.015;
        }
        2 => {
            // the dance: hips to the beat, stepping from foot to foot, arms pumping in turn
            let beat = t * 5.2;
            let (s, c) = beat.sin_cos();
            e.hip = vec3(0.06 * s, base.hip.y - 0.035 - 0.02 * c.abs(), 0.01);
            e.pel_yaw = 0.2 * s;
            e.pel_roll = -0.08 * s;
            e.twist = -0.3 * s;
            e.side = 0.1 * s;
            e.head_yaw = 0.2 * c;
            e.head_pitch = -0.08 + 0.08 * c.abs();
            let up = |x: f32| x.max(0.0) * 0.07;
            e.foot[0] = vec3(HIP_X + 0.07, ANKLE + up(s), 0.04 + 0.05 * s.max(0.0));
            e.foot[1] = vec3(-(HIP_X + 0.07), ANKLE + up(-s), 0.04 + 0.05 * (-s).max(0.0));
            e.arm[0] = [1.2 + 0.7 * s, 0.25, 1.5 - 0.5 * s];
            e.arm[1] = [1.2 - 0.7 * s, 0.25, 1.5 + 0.5 * s];
        }
        3 => {
            // wave: the arm up, the forearm going to and fro
            e.arm[1] = [2.75, 0.5 + 0.3 * (t * 9.0).sin(), 0.35 + 0.25 * (t * 9.0).sin()];
            e.side = 0.05;
            e.head_pitch = -0.06;
        }
        4 => {
            // both biceps, a little pump
            let pump = 0.2 * (t * 4.0).sin().max(0.0);
            // found by search on the model: the fists beside the head
            e.arm[0] = [1.7, -1.0, 2.2 + pump];
            e.arm[1] = [1.7, -1.0, 2.2 + pump];
            e.lean = base.lean - 0.1;
            e.hip.y = base.hip.y - 0.01;
            e.head_pitch = -0.12;
            e.twist = 0.12 * (t * 1.4).sin();
        }
        5 => {
            // facepalm: the hand comes up to meet the dropping head, a slow shake
            e.arm[1] = [1.2, 0.55, 2.3];
            e.head_pitch = 0.32;
            e.lean = base.lean + 0.12;
            e.head_yaw = 0.14 * (t * 3.2).sin();
        }
        6 => {
            // a deep bow, one arm across the belly
            let d = smooth(t / 0.7) * (1.0 - smooth((t - 1.6) / 0.6));
            e.lean = base.lean + 0.95 * d;
            e.hip.z = base.hip.z - 0.1 * d;
            e.head_pitch = 0.2 * d;
            e.arm[1] = [0.35, 0.05, 1.5];
            e.arm[0] = [-0.2, 0.1, 0.2];
        }
        _ => {
            // clapping in front of the chest
            let clap = (t * 12.0).sin();
            // found by search on the model: elbows by the sides, the hands meeting in front of the chest
            let open = -0.5 + 0.13 * clap.max(-0.2);
            e.arm[0] = [-0.4, open, 2.0];
            e.arm[1] = [-0.4, open, 2.0];
            e.head_pitch = -0.04 + 0.03 * (clock * 2.0).sin();
        }
    }
    *o = o.lerp(&e, k);
}

fn build(p: &Person) -> Pose {
    let v = &p.vis;
    let a = &p.anim;
    let base = idle(v);
    let k_loco = smooth((v.speed - 0.08) / 0.5);
    let mut o = base.lerp(&locomotion(v, &base), k_loco);
    o = o.lerp(&bend(&o), smooth(v.w_bend));
    o = o.lerp(&sit(v), smooth(v.w_sit));
    o = o.lerp(&cheer(v), smooth(v.w_cheer));
    if v.w_reach > 0.0 {
        // seated, leaning forward with an arm down to the crate's opening
        let k = smooth(v.w_reach);
        o.lean = mix(o.lean, 0.8, k);
        o.head_pitch = mix(o.head_pitch, -0.45, k);
        o.arm_r([0.95, 0.0, 0.1], k);
    }
    // the right arm: holding the ball, a can to the lips, pointing, the cigarette
    let tmp_hold = smooth(v.w_hold);
    o.arm_r([0.75, 0.16, 1.05], tmp_hold);
    if v.w_smoke > 0.0 {
        let k = smooth(v.w_smoke);
        let rest = if v.w_sit > 0.5 { [0.6, 0.2, 1.35] } else { [0.45, 0.15, 1.5] };
        o.arm_r(rest, k);
        o.arm_r([1.15, 0.55, 2.3], smooth(v.w_drag));
        o.head_pitch = mix(o.head_pitch, -0.12, smooth(v.w_drag));
    }
    // the can: bend down for it, bring it up to the chest, and put it back the same way
    let c = v.carry_t;
    if c > 0.0 {
        let down = smooth(c / CAN_GRAB) * (1.0 - smooth((c - CAN_GRAB) / 0.42));
        o = o.lerp(&reach_down(&o), down);
        o.arm_r([0.42, 0.14, 1.3], smooth((c - CAN_GRAB - 0.08) / 0.4));
    }
    let sip = v.sip_k();
    if sip > 0.0 {
        o.arm_r([1.15, 0.6, 2.3], sip); // found by search on the model: the hand reaches the lips
        // the longer you drink, the further the head and the can go back; a swallow now and then
        let long = (v.sip_t / 2.5).min(1.0);
        let swallow = 0.018 * (v.sip_t * 5.5).sin().max(0.0);
        let tilt = -0.2 - 0.34 * long - swallow;
        o.head_pitch = mix(o.head_pitch, tilt, sip);
        o.lean = mix(o.lean, o.lean - 0.05 - 0.05 * long, sip);
        o.hip.z = mix(o.hip.z, o.hip.z + 0.012, sip);
        // the free arm hangs a little out, for balance
        o.arm[0] = [mix(o.arm[0][0], 0.12, sip * 0.6), mix(o.arm[0][1], 0.22, sip * 0.6), mix(o.arm[0][2], 0.4, sip * 0.6)];
    }
    o.arm_r([1.45, 0.1, 0.05], smooth(v.w_point));
    if a.emote > 0 {
        emote_pose(&mut o, a.emote - 1, a.emote_t, v.t);
    }
    if a.throw_t >= 0.0 {
        throw(&mut o, a.throw_t);
    }
    // the head turns to watch; a bit of it goes into the shoulders when it's a big turn
    o.head_yaw = v.look[0] * 0.8;
    o.twist += v.look[0] * 0.2 * (1.0 - v.w_sit);
    o.head_pitch += v.look[1];
    o
}
