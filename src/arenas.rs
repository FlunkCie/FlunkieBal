//! The places you can play (Vondelpark, Science Park dak, Café de Krul, Duplex), plus the
//! training hall and the clubhouse shed. Each has its own light, sky and walls.
//! Indoor rooms are "dollhouses": a wall is only drawn while the camera is on its inner side,
//! so a camera outside the room looks straight in.

use crate::game::{self, Rng};
use crate::gfx::{inst, kind, DrawList, Mesh};
use crate::scene::{billboard, bottle, hex, open_crate, rx, ry, rz, s, su, t};
use glam::{vec3, Mat4, Vec3};
use std::f32::consts::PI;

pub const WOOD: f32 = 8.0;
pub const TILES: f32 = 9.0;
pub const WINDOWS: f32 = 11.0;
pub const BRICK: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArenaKey {
    Vondel,
    Evening,
    Roof,
    Cafe,
    Club,
    Krakau,
    Gym,
    Hub,
}
pub const PITCHES: [ArenaKey; 6] = [ArenaKey::Vondel, ArenaKey::Evening, ArenaKey::Roof, ArenaKey::Cafe, ArenaKey::Club, ArenaKey::Krakau];

impl ArenaKey {
    pub fn id(self) -> &'static str {
        match self {
            ArenaKey::Vondel => "vondel",
            ArenaKey::Evening => "evening",
            ArenaKey::Krakau => "krakau",
            ArenaKey::Roof => "roof",
            ArenaKey::Cafe => "cafe",
            ArenaKey::Club => "club",
            ArenaKey::Gym => "gym",
            ArenaKey::Hub => "hub",
        }
    }
    pub fn from_id(s: &str) -> ArenaKey {
        match s {
            "evening" => ArenaKey::Evening,
            "krakau" => ArenaKey::Krakau,
            "roof" => ArenaKey::Roof,
            "cafe" => ArenaKey::Cafe,
            "club" => ArenaKey::Club,
            "gym" => ArenaKey::Gym,
            "hub" => ArenaKey::Hub,
            _ => ArenaKey::Vondel,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ArenaKey::Vondel => "Vondelpark",
            ArenaKey::Evening => "Vondelpark evening",
            ArenaKey::Krakau => "Krakau Hostel",
            ArenaKey::Roof => "Science Park dak",
            ArenaKey::Cafe => "Café de Krul",
            ArenaKey::Club => "Duplex",
            ArenaKey::Gym => "Training hall",
            ArenaKey::Hub => "The Clubhouse",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            ArenaKey::Vondel => "Trees and a hazy afternoon",
            ArenaKey::Evening => "The park at golden hour",
            ArenaKey::Krakau => "The parking lot, at dusk",
            ArenaKey::Roof => "A campus rooftop at sunset",
            ArenaKey::Cafe => "A packed brown café",
            ArenaKey::Club => "Prague, on the roof",
            _ => "",
        }
    }
    /// Ball bounds (walls or railing), half extents.
    /// Which built set of scenery this place uses: the evening is the same park.
    pub fn env_id(self) -> &'static str {
        match self {
            ArenaKey::Evening => ArenaKey::Vondel.id(),
            ArenaKey::Gym | ArenaKey::Hub => "hub",
            k => k.id(),
        }
    }
    pub fn bounds(self) -> Option<(f32, f32)> {
        flunkiebal_core::proto::arena_bounds(self.id())
    }
}

/// Light and air for an arena.
#[derive(Clone, Copy)]
pub struct Look {
    pub sun_dir: Vec3,
    pub sun_col: [f32; 3],
    pub sky_top: [f32; 3],
    pub sky_hor: [f32; 3],
    pub ground: [f32; 3],
    pub fog: [f32; 4],
    pub exposure: f32,
    pub clear: [f32; 3],
    pub grade: [f32; 4],
    /// How strongly the sky/ground (or, indoors, the bounce off walls and ceiling) lights things.
    pub ambient: f32,
    /// Lamps: position, range, colour × intensity. Slot ranges of 0 are unused.
    pub lamps: [(Vec3, f32, [f32; 3]); 6],
    /// Rooms (min, max corner) whose corners get darker, the way light dies in a real room.
    pub rooms: [Option<(Vec3, Vec3)>; 2],
    /// Shadows are cast by lamp 0 (a bare bulb) instead of the sun.
    pub spot: bool,
}
/// The park's own lamp posts (see `scene::build_env`).
const EVENING_LAMPS: [Vec3; 3] = [vec3(-10.69, 3.45, -11.0), vec3(-7.57, 3.45, 7.0), vec3(-5.29, 3.45, 31.0)];
/// Half width, half length and height of the two tight rooms.
const CAFE: (f32, f32, f32) = (6.9, 8.9, 3.0);
const CLUB: (f32, f32, f32) = (8.4, 13.0, 6.6);
/// How high the DJ stands, and where the stage's front edge is: a few metres past the line, the crowd in between.
const STAGE: (f32, f32) = (1.7, -10.6);
const NO_LAMP: (Vec3, f32, [f32; 3]) = (Vec3::ZERO, 0.0, [0.0; 3]);
const GRADE: [f32; 4] = [1.08, 1.1, 0.0, 0.0];

pub fn look(k: ArenaKey) -> Look {
    let base = Look { sun_dir: vec3(0.0, 1.0, 0.0), sun_col: [0.0; 3], sky_top: [0.0; 3], sky_hor: [0.0; 3], ground: [0.0; 3], fog: [0.0; 4], exposure: 1.0, clear: [0.0; 3], grade: GRADE, ambient: 1.0, lamps: [NO_LAMP; 6], rooms: [None; 2], spot: false };
    let room = |hx: f32, hz: f32, h: f32, o: Vec3| Some((o + vec3(-hx, 0.0, -hz), o + vec3(hx, h, hz)));
    match k {
        ArenaKey::Vondel => Look { sun_dir: vec3(-0.62, 0.42, 0.66), sun_col: [3.0, 2.55, 2.0], sky_top: [0.28, 0.45, 0.8], sky_hor: [0.76, 0.8, 0.82], ground: [0.16, 0.2, 0.08], fog: [0.74, 0.79, 0.82, 0.018], exposure: 0.8, clear: [0.74, 0.79, 0.82], ambient: 0.8, ..base },
        ArenaKey::Evening => Look {
            // the sun low over the far end, long shadows, a warm haze, and the park's lamps coming on
            sun_dir: vec3(0.72, 0.17, -0.68),
            sun_col: [3.6, 1.8, 0.72],
            sky_top: [0.27, 0.3, 0.58],
            sky_hor: [0.95, 0.52, 0.3],
            ground: [0.25, 0.19, 0.1],
            fog: [0.8, 0.46, 0.28, 0.016],
            exposure: 0.85,
            clear: [0.8, 0.46, 0.28],
            ambient: 0.6,
            grade: [1.08, 1.12, 0.02, 0.0],
            lamps: [(EVENING_LAMPS[0], 11.0, [9.0, 5.8, 2.6]), (EVENING_LAMPS[1], 11.0, [9.0, 5.8, 2.6]), (EVENING_LAMPS[2], 11.0, [9.0, 5.8, 2.6]), NO_LAMP, NO_LAMP, NO_LAMP],
            ..base
        },
        ArenaKey::Krakau => {
            // the blue hour, and sodium lamps doing the work
            let lamp = |x: f32, z: f32| (vec3(x, 5.2, z), 20.0, [46.0, 25.0, 8.0]);
            Look {
                sun_dir: vec3(-0.4, 0.5, 0.6),
                sun_col: [0.16, 0.2, 0.36],
                sky_top: [0.05, 0.08, 0.24],
                sky_hor: [0.5, 0.3, 0.32],
                ground: [0.05, 0.05, 0.07],
                fog: [0.13, 0.11, 0.17, 0.012],
                exposure: 1.1,
                clear: [0.06, 0.06, 0.12],
                ambient: 0.4,
                grade: [1.1, 1.1, 0.0, 0.006],
                lamps: [lamp(-7.6, -9.0), lamp(7.6, -9.0), lamp(-7.6, 9.0), lamp(7.6, 9.0), (vec3(0.0, 3.2, -13.8), 9.0, [9.0, 7.0, 4.5]), NO_LAMP],
                ..base
            }
        }
        ArenaKey::Roof => Look { sun_dir: vec3(0.86, 0.16, -0.49), sun_col: [3.0, 1.65, 0.85], sky_top: [0.24, 0.27, 0.58], sky_hor: [1.0, 0.58, 0.36], ground: [0.3, 0.24, 0.22], fog: [0.85, 0.52, 0.4, 0.0025], exposure: 0.85, clear: [0.5, 0.4, 0.5], ambient: 0.85, ..base },
        ArenaKey::Cafe => {
            // low fringed lamps full of amber, and very little of anything else
            let pend = |x: f32, z: f32| (vec3(x, 2.3, z), 7.5, [6.0, 3.4, 1.4]);
            Look {
                sun_dir: vec3(0.22, 1.0, 0.3),
                sun_col: [0.1, 0.07, 0.04],
                sky_top: [0.38, 0.27, 0.17],
                sky_hor: [0.22, 0.14, 0.08],
                ground: [0.24, 0.14, 0.07],
                fog: [0.1, 0.055, 0.025, 0.01],
                exposure: 1.2,
                clear: [0.07, 0.04, 0.02],
                ambient: 0.13,
                grade: [1.12, 1.08, 0.02, 0.012],
                lamps: [(vec3(0.0, 2.55, 0.0), 9.0, [8.0, 4.5, 1.8]), pend(-3.4, -4.6), pend(3.4, -4.6), pend(-3.4, 4.6), pend(3.4, 4.6), (vec3(5.6, 2.3, 0.0), 6.0, [6.0, 3.2, 1.2])],
                spot: true,
                rooms: [room(CAFE.0, CAFE.1, CAFE.2, Vec3::ZERO), None],
                ..base
            }
        }
        ArenaKey::Club => Look {
            // night over Prague through the glass; orange from the truss, magenta from the stage
            sun_dir: vec3(0.15, 1.0, 0.25),
            sun_col: [0.1, 0.09, 0.2],
            sky_top: [0.02, 0.03, 0.09],
            sky_hor: [0.2, 0.12, 0.26],
            ground: [0.09, 0.05, 0.1],
            fog: [0.05, 0.03, 0.08, 0.006],
            exposure: 1.15,
            clear: [0.02, 0.02, 0.05],
            ambient: 0.1,
            grade: [1.16, 1.12, 0.0, 0.008],
            spot: true,
            lamps: [(vec3(0.0, 6.0, 0.0), 18.0, [30.0, 18.0, 10.0]), (vec3(-5.0, 5.0, -9.5), 12.0, [26.0, 4.0, 30.0]), (vec3(5.0, 5.0, -9.5), 12.0, [30.0, 10.0, 4.0]), (vec3(0.0, 3.0, 11.6), 10.0, [24.0, 13.0, 4.0]), (vec3(-7.2, 5.0, 2.0), 11.0, [4.0, 12.0, 28.0]), (vec3(7.2, 5.0, 2.0), 11.0, [26.0, 5.0, 18.0])],
            rooms: [room(CLUB.0, CLUB.1, CLUB.2, Vec3::ZERO), None],
            ..base
        },
        ArenaKey::Gym => {
            let lamp = |x: f32, z: f32| (vec3(x, 4.45, z), 16.0, [10.0, 9.2, 7.8]);
            Look {
                sun_dir: vec3(0.1, 1.0, 0.16),
                sun_col: [0.85, 0.78, 0.66],
                sky_top: [0.55, 0.46, 0.36],
                sky_hor: [0.3, 0.22, 0.15],
                ground: [0.36, 0.24, 0.13],
                fog: [0.1, 0.07, 0.05, 0.01],
                exposure: 1.05,
                clear: [0.08, 0.055, 0.04],
                ambient: 0.38,
                grade: [1.08, 1.1, 0.0, 0.003],
                lamps: [NO_LAMP, NO_LAMP, lamp(-4.2, -6.5), lamp(4.2, -6.5), lamp(-4.2, 6.5), lamp(4.2, 6.5)],
                rooms: [None, room(9.4, 13.4, 5.0, Vec3::ZERO)],
                ..base
            }
        }
        ArenaKey::Hub => Look {
            sun_dir: vec3(0.2, 1.0, -0.3),
            sun_col: [0.1, 0.08, 0.06],
            sky_top: [0.42, 0.3, 0.2],
            sky_hor: [0.22, 0.14, 0.08],
            ground: [0.3, 0.17, 0.09],
            fog: [0.08, 0.05, 0.035, 0.02],
            exposure: 1.2,
            clear: [0.07, 0.045, 0.03],
            ambient: 0.2,
            grade: [1.1, 1.1, 0.0, 0.01],
            lamps: [(HUB_O + vec3(0.15, 2.22, 0.55), 8.0, [7.5, 5.0, 2.6]), (HUB_O + vec3(DOOR_X, 1.4, -4.1), 4.5, [1.6, 1.4, 1.1]), NO_LAMP, NO_LAMP, NO_LAMP, NO_LAMP],
            rooms: [room(3.6, 4.6, 3.0, HUB_O), None],
            spot: true,
            ..base
        },
    }
}

/// Mix two arenas' light (walking from the clubhouse into the training hall). Lamps and rooms are
/// real things, so both arenas keep theirs; only the air and the exposure change.
pub fn blend(a: Look, b: Look, k: f32) -> Look {
    let m = |x: f32, y: f32| x + (y - x) * k;
    let m3 = |x: [f32; 3], y: [f32; 3]| [m(x[0], y[0]), m(x[1], y[1]), m(x[2], y[2])];
    let mut lamps = a.lamps;
    for (i, l) in lamps.iter_mut().enumerate() {
        if l.1 <= 0.0 {
            *l = b.lamps[i];
        }
    }
    Look {
        sun_dir: a.sun_dir.lerp(b.sun_dir, k),
        sun_col: m3(a.sun_col, b.sun_col),
        sky_top: m3(a.sky_top, b.sky_top),
        sky_hor: m3(a.sky_hor, b.sky_hor),
        ground: m3(a.ground, b.ground),
        fog: [m(a.fog[0], b.fog[0]), m(a.fog[1], b.fog[1]), m(a.fog[2], b.fog[2]), m(a.fog[3], b.fog[3])],
        exposure: m(a.exposure, b.exposure),
        clear: m3(a.clear, b.clear),
        grade: a.grade,
        ambient: m(a.ambient, b.ambient),
        lamps,
        rooms: [a.rooms[0].or(b.rooms[0]), a.rooms[1].or(b.rooms[1])],
        spot: if k < 0.5 { a.spot } else { b.spot },
    }
}

/// A wall and everything hung on it; drawn only while the camera is inside.
pub struct Wall {
    pub n: Vec3,
    pub d: f32,
    pub list: DrawList,
}
pub struct Env {
    pub list: DrawList,
    pub walls: Vec<Wall>,
    pub lights: Vec<(Vec3, [f32; 3], f32)>, // glow sprites: position, colour, size
}
impl Env {
    fn new() -> Env {
        Env { list: DrawList::default(), walls: Vec::new(), lights: Vec::new() }
    }
    fn wall(&mut self, n: Vec3, at: Vec3) -> &mut DrawList {
        self.walls.push(Wall { n, d: -n.dot(at), list: DrawList::default() });
        &mut self.walls.last_mut().unwrap().list
    }
    /// Append what the camera at `cam` can see.
    pub fn draw(&self, dl: &mut DrawList, cam: Vec3) {
        dl.opaque.extend_from_slice(&self.list.opaque);
        dl.blend.extend_from_slice(&self.list.blend);
        dl.add.extend_from_slice(&self.list.add);
        for w in &self.walls {
            if w.n.dot(cam) + w.d > 0.0 {
                dl.opaque.extend_from_slice(&w.list.opaque);
                dl.blend.extend_from_slice(&w.list.blend);
                dl.add.extend_from_slice(&w.list.add);
            }
        }
        for (p, c, sz) in &self.lights {
            dl.add.push((Mesh::Quad, inst(billboard(cam, *p, *sz), [c[0], c[1], c[2], 0.6], [kind::SOFT, 0.0, 0.0, 0.0])));
        }
    }
}

fn surf(dl: &mut DrawList, mesh: Mesh, m: Mat4, rgb: [f32; 3], k: f32, extra: f32, cast: bool) {
    dl.push(mesh, m, [rgb[0], rgb[1], rgb[2], 1.0], [k, 0.0, if cast { 1.0 } else { 0.0 }, extra]);
}
fn glowbox(dl: &mut DrawList, m: Mat4, rgb: [f32; 3], e: f32) {
    dl.push(Mesh::Cube, m, [rgb[0], rgb[1], rgb[2], 1.0], [kind::LIT, e, 0.0, 0.0]);
}
/// A room: floor, four inward walls with skirting, and a ceiling seen only from below.
fn room(e: &mut Env, hx: f32, hz: f32, h: f32, floor: (f32, [f32; 3], f32), wall: (f32, [f32; 3]), ceil: [f32; 3]) {
    room_h(e, hx, hz, h, floor, wall, ceil, [None; 4]);
}
/// A room whose walls (+x, −x, +z, −z) may have an opening: (centre along the wall, width, height).
#[allow(clippy::too_many_arguments)]
fn room_h(e: &mut Env, hx: f32, hz: f32, h: f32, floor: (f32, [f32; 3], f32), wall: (f32, [f32; 3]), ceil: [f32; 3], holes: [Option<(f32, f32, f32)>; 4]) {
    surf(&mut e.list, Mesh::Plane, s(hx * 2.0, 1.0, hz * 2.0), floor.1, floor.0, floor.2, false);
    let sides = [(vec3(-1.0, 0.0, 0.0), vec3(hx, 0.0, 0.0), hz * 2.0, PI * 0.5), (vec3(1.0, 0.0, 0.0), vec3(-hx, 0.0, 0.0), hz * 2.0, PI * 0.5), (vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, hz), hx * 2.0, 0.0), (vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz), hx * 2.0, 0.0)];
    for (k, (n, at, len, rot)) in sides.into_iter().enumerate() {
        let dl = e.wall(n, at);
        // the wall as pieces round the opening (if any): left, right, and the lintel above it
        let pieces: Vec<(f32, f32, f32, f32)> = match holes[k] {
            None => vec![(-len / 2.0, len / 2.0, 0.0, h)],
            Some((c, w, hh)) => vec![(-len / 2.0, c - w / 2.0, 0.0, h), (c + w / 2.0, len / 2.0, 0.0, h), (c - w / 2.0, c + w / 2.0, hh, h)],
        };
        let along = ry(rot).transform_vector3(Vec3::X);
        for (a0, a1, y0, y1) in pieces {
            if a1 - a0 < 0.01 || y1 - y0 < 0.01 {
                continue;
            }
            let mid = at + along * ((a0 + a1) * 0.5);
            surf(dl, Mesh::Cube, t(mid.x, (y0 + y1) * 0.5, mid.z) * ry(rot) * s(a1 - a0, y1 - y0, 0.12), wall.1, wall.0, 0.0, false);
            if y0 == 0.0 {
                surf(dl, Mesh::Cube, t(mid.x + n.x * 0.075, 0.07, mid.z + n.z * 0.075) * ry(rot) * s(a1 - a0, 0.14, 0.03), hex(0x2a1c12), kind::LIT, 0.0, false);
            }
        }
    }
    let dl = e.wall(vec3(0.0, -1.0, 0.0), vec3(0.0, h, 0.0));
    surf(dl, Mesh::Cube, t(0.0, h + 0.06, 0.0) * s(hx * 2.0, 0.12, hz * 2.0), ceil, kind::LIT, 0.0, false);
}

// ---------- Science Park dak ----------
fn build_roof() -> Env {
    let mut e = Env::new();
    let mut r = Rng::new(77);
    e.list.push(Mesh::Sphere, su(220.0), [1.0; 4], [kind::SKY, 0.0, 0.0, 0.0]);
    surf(&mut e.list, Mesh::Plane, s(24.0, 1.0, 30.0), hex(0x99938a), TILES, 1.2, false);
    surf(&mut e.list, Mesh::Plane, t(0.0, -0.02, 0.0) * s(60.0, 1.0, 70.0), hex(0x6d6862), kind::LIT, 0.0, false);
    let conc = hex(0x8f8a82);
    let steel = hex(0xb8bcc0);
    for (x, z, w, d) in [(0.0f32, 15.0f32, 24.0f32, 0.3f32), (0.0, -15.0, 24.0, 0.3), (12.0, 0.0, 0.3, 30.0), (-12.0, 0.0, 0.3, 30.0)] {
        e.list.solid(Mesh::Cube, t(x, 0.225, z) * s(w, 0.45, d), conc);
        e.list.blend.push((Mesh::Cube, inst(t(x, 0.95, z) * s(w.max(0.02), 1.0, d.max(0.02)), [0.8, 0.9, 0.95, 0.18], [kind::UNLIT, 0.0, 0.0, 1.0])));
        e.list.solid(Mesh::Cube, t(x, 1.47, z) * s(w.max(0.06), 0.05, d.max(0.06)), steel);
    }
    let mut i = -12.0;
    while i <= 12.0 {
        for z in [15.0f32, -15.0] {
            e.list.solid(Mesh::Cube, t(i, 0.95, z) * s(0.04, 1.0, 0.04), steel);
        }
        i += 2.0;
    }
    // rooftop plant and a lift housing
    for (x, z) in [(-9.0f32, -12.0f32), (9.2, 11.6), (-9.4, 10.8)] {
        e.list.solid(Mesh::Cube, t(x, 0.65, z) * s(2.2, 1.3, 1.4), hex(0xa7aaa9));
        for o in [-0.5f32, 0.5] {
            e.list.push(Mesh::Disc, t(x + o, 1.31, z) * su(0.42), [0.08, 0.09, 0.1, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        }
    }
    e.list.solid(Mesh::Cube, t(9.6, 1.5, -12.4) * s(3.2, 3.0, 2.6), hex(0x7a7f86));
    // solar panels
    for x in [-10.6f32, 10.6] {
        let mut z = -9.0;
        while z <= 9.5 {
            e.list.push(Mesh::Cube, t(x, 0.42, z) * rx(-0.42) * s(1.6, 0.04, 0.95), [0.05, 0.09, 0.2, 1.0], [kind::LIT, 0.05, 1.0, 0.0]);
            z += 1.05;
        }
    }
    // planters
    for (k, (x, z)) in [(-11.0f32, -4.0f32), (-11.0, 4.0), (11.0, -4.0), (11.0, 4.0)].iter().enumerate() {
        e.list.solid(Mesh::Cube, t(*x, 0.3, *z) * s(0.9, 0.6, 1.6), hex(0x6b4a2e));
        e.list.solid(Mesh::Sphere, t(*x, 0.85, *z) * s(0.45, 0.42, 0.75), if k % 2 == 0 { hex(0x4f7d30) } else { hex(0x3f6b2a) });
    }
    // string lights over the side lanes
    for x in [-7.2f32, 7.2] {
        for z in [-14.2f32, 14.2] {
            e.list.solid(Mesh::Cube, t(x, 1.7, z) * s(0.06, 3.4, 0.06), steel);
        }
        for i in 0..=20 {
            let k = i as f32 / 20.0;
            let z = -14.2 + 28.4 * k;
            let y = 3.35 - (k * PI).sin() * 0.7;
            e.list.push(Mesh::Sphere, t(x, y, z) * su(0.045), [1.0, 0.82, 0.48, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
            e.lights.push((vec3(x, y, z), [1.0, 0.65, 0.25], 0.32));
        }
    }
    // people have made it their own: benches, crates, a barbecue, more green, a water tank and aerials
    for (x, z, rot) in [(-8.2f32, -13.6f32, 0.0f32), (-3.0, -13.8, 0.0), (3.4, 13.8, PI), (8.4, 13.6, PI), (-10.9, 11.8, PI * 0.5), (10.9, -11.2, -PI * 0.5)] {
        let m = t(x, 0.0, z) * ry(rot);
        e.list.solid(Mesh::Cube, m * t(0.0, 0.45, 0.0) * s(1.8, 0.07, 0.5), hex(0x8a6a44));
        e.list.solid(Mesh::Cube, m * t(0.0, 0.8, -0.22) * s(1.8, 0.4, 0.06), hex(0x8a6a44));
        for sd in [-0.8f32, 0.8] {
            e.list.solid(Mesh::Cube, m * t(sd, 0.22, 0.0) * s(0.08, 0.44, 0.45), hex(0x3a3d40));
        }
    }
    for (x, z, n) in [(-9.6f32, 13.4f32, 3usize), (10.6, 13.0, 2), (-10.6, -13.2, 2), (5.6, -13.8, 3), (-5.6, 13.6, 1)] {
        stack(&mut e.list, &mut r, x, z, n, 0.3 * x);
    }
    for (x, z) in [(-11.1f32, -8.5f32), (-11.1, 8.5), (11.1, -8.5), (11.1, 8.5), (-2.0, -14.2), (2.0, 14.2), (-7.0, 14.2), (7.0, -14.2), (0.0, -14.2), (0.0, 14.2)] {
        plant(&mut e.list, x, z, 0.8 + (x * 3.1 + z).sin().abs() * 0.5);
    }
    e.list.solid(Mesh::Cyl, t(-9.2, 2.1, -12.4) * s(1.1, 1.5, 1.1), hex(0x8d6e4e));
    for (dx, dz) in [(-0.8f32, -0.8f32), (0.8, -0.8), (-0.8, 0.8), (0.8, 0.8)] {
        e.list.solid(Mesh::Cyl, t(-9.2 + dx, 0.9, -12.4 + dz) * s(0.05, 1.8, 0.05), steel);
    }
    for (x, z, h) in [(9.0f32, -10.4f32, 5.0f32), (10.4, -10.8, 3.6), (-9.8, 13.8, 4.2)] {
        e.list.solid(Mesh::Cyl, t(x, h * 0.5, z) * s(0.03, h, 0.03), steel);
        e.list.solid(Mesh::Cube, t(x, h * 0.85, z) * s(0.9, 0.02, 0.02), steel);
    }
    // the city around: labs close by, towers further out, windows lighting up
    for k in 0..46 {
        let a = k as f32 / 46.0 * PI * 2.0 + r.f() * 0.1;
        let d = 38.0 + r.f() * 55.0;
        let (w, dd, h) = (6.0 + r.f() * 14.0, 6.0 + r.f() * 10.0, 6.0 + r.f() * 38.0);
        let tone = 0.12 + r.f() * 0.12;
        e.list.push(Mesh::Cube, t(a.sin() * d, h * 0.5 - 25.0, a.cos() * d) * ry(a) * s(w, h, dd), [tone, tone * 0.95, tone * 1.05, 1.0], [WINDOWS, 0.0, 0.0, 0.0]);
    }
    e
}

/// A potted plant.
fn plant(dl: &mut DrawList, x: f32, z: f32, sc: f32) {
    dl.solid(Mesh::Cyl, t(x, 0.22 * sc, z) * s(0.26 * sc, 0.44 * sc, 0.26 * sc), hex(0xa85f35));
    dl.solid(Mesh::Sphere, t(x, 0.75 * sc, z) * s(0.28 * sc, 0.4 * sc, 0.28 * sc), [0.06, 0.12, 0.04]);
    for (k, (ox, oy, oz)) in [(0.0f32, 0.95f32, 0.0f32), (0.16, 0.7, 0.1), (-0.14, 0.75, -0.1), (0.05, 0.62, -0.17)].iter().enumerate() {
        dl.push(Mesh::Leaves, t(x + ox * sc, oy * sc, z + oz * sc) * ry(k as f32 * 1.9) * su(0.38 * sc), [0.2 + 0.05 * k as f32, 0.42, 0.15, 1.0], [kind::LEAF, 0.0, 1.0, 0.0]);
    }
}

/// A parked car, nose along +x, its base at the origin.
fn car(dl: &mut DrawList, m: Mat4, col: [f32; 3], van: bool) {
    let dark = [0.03, 0.04, 0.05];
    let (len, wid) = (if van { 4.3 } else { 3.9 }, 1.7);
    dl.solid(Mesh::Cube, m * t(0.0, 0.5, 0.0) * s(len, 0.62, wid), col);
    if van {
        dl.solid(Mesh::Cube, m * t(-0.35, 1.28, 0.0) * s(len - 0.9, 0.98, wid - 0.06), col);
        dl.solid(Mesh::Cube, m * t(len * 0.5 - 1.02, 1.32, 0.0) * s(0.5, 0.56, wid - 0.1), dark);
    } else {
        dl.solid(Mesh::Cube, m * t(-0.2, 1.08, 0.0) * s(2.0, 0.54, wid - 0.16), col);
        dl.solid(Mesh::Cube, m * t(-0.2, 1.1, 0.0) * s(2.04, 0.36, wid - 0.12), dark);
        dl.solid(Mesh::Cube, m * t(-0.2, 1.1, 0.0) * s(1.72, 0.37, wid - 0.1), dark);
    }
    for (sx, sz) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        dl.solid(Mesh::Cyl, m * t(sx * len * 0.3, 0.3, sz * (wid * 0.5 - 0.02)) * rx(PI * 0.5) * s(0.3, 0.2, 0.3), hex(0x101012));
    }
    for sz in [-0.58f32, 0.58] {
        glowbox(dl, m * t(len * 0.5 + 0.005, 0.62, sz) * s(0.02, 0.14, 0.3), [0.9, 0.88, 0.7], 0.3);
        glowbox(dl, m * t(-len * 0.5 - 0.005, 0.62, sz) * s(0.02, 0.12, 0.3), [0.7, 0.05, 0.05], 0.5);
    }
    dl.solid(Mesh::Cube, m * t(len * 0.5 + 0.03, 0.34, 0.0) * s(0.06, 0.14, wid), hex(0x1a1a1c));
    dl.solid(Mesh::Cube, m * t(-len * 0.5 - 0.03, 0.34, 0.0) * s(0.06, 0.14, wid), hex(0x1a1a1c));
}

// ---------- Krakau Hostel: the parking lot behind it ----------
fn build_krakau() -> Env {
    let mut e = Env::new();
    let mut r = Rng::new(1996);
    e.list.push(Mesh::Sphere, su(220.0), [1.0; 4], [kind::SKY, 0.0, 0.0, 0.0]);
    surf(&mut e.list, Mesh::Plane, s(90.0, 1.0, 90.0), hex(0x3a3a3e), kind::ASPHALT, 0.0, false);
    let paint = hex(0xc9c6b8);
    // the bays down both sides, a car or van in each
    let cols = [0xb7bcc0, 0x1d2a44, 0x7a1f1f, 0x2d2f33, 0xd8d6cf, 0x34553a, 0x8a8f94, 0xc49a3a];
    for sd in [1.0f32, -1.0] {
        let mut z = -13.0f32;
        while z <= 13.01 {
            e.list.solid(Mesh::Cube, t(sd * 10.4, 0.004, z - 1.3) * s(4.6, 0.006, 0.09), paint);
            let c = hex(cols[(r.f() * 8.0) as usize % 8]);
            let van = r.f() < 0.18;
            let nudge = (r.f() - 0.5) * 0.25;
            // noses towards the pitch; the bumpers are where the ball bounces back
            car(&mut e.list, t(sd * 10.35, 0.0, z + nudge) * ry(if sd > 0.0 { PI } else { 0.0 }), c, van);
            z += 2.6;
        }
        e.list.solid(Mesh::Cube, t(sd * 10.4, 0.004, 14.3) * s(4.6, 0.006, 0.09), paint);
        e.list.solid(Mesh::Cube, t(sd * 13.6, 1.3, 0.0) * s(0.5, 2.6, 30.0), hex(0x6f5548));
    }
    // arrows, a manhole, oil stains and patches in the tarmac
    for (x, z) in [(-5.5f32, -11.0f32), (5.5, 10.0), (6.6, -3.0), (-6.6, 4.0), (1.4, 11.5), (-2.8, -9.8), (3.0, -11.8)] {
        e.list.push(Mesh::Disc, t(x, 0.004, z) * ry(x) * s(0.5 + (z * 0.7).sin().abs() * 0.5, 1.0, 0.4), [0.03, 0.03, 0.035, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    }
    e.list.solid(Mesh::Cyl, t(5.8, 0.006, 3.4) * s(0.36, 0.012, 0.36), hex(0x1c1c1e));
    e.list.solid(Mesh::Cube, t(-3.4, 0.003, 11.0) * ry(0.2) * s(3.0, 0.004, 1.3), hex(0x2a2a2e));
    // the hostel: four floors of plaster, a lit door, a sign and an awning, a smokers' bench
    surf(&mut e.list, Mesh::Cube, t(0.0, 8.0, -18.2) * s(44.0, 16.0, 7.0), hex(0x6d6a60), WINDOWS, 0.0, true);
    surf(&mut e.list, Mesh::Cube, t(0.0, 1.5, -14.6) * s(44.0, 3.0, 0.2), hex(0xb7ad98), kind::PLASTER, 0.0, true);
    glowbox(&mut e.list, t(0.0, 1.15, -14.47) * s(1.5, 2.3, 0.04), [1.0, 0.82, 0.55], 1.3);
    e.list.solid(Mesh::Cube, t(0.0, 2.55, -14.0) * s(3.0, 0.08, 1.2), hex(0x7a1f1f));
    for x in [-5.4f32, -2.6, 2.6, 5.4] {
        glowbox(&mut e.list, t(x, 1.55, -14.47) * s(1.4, 1.2, 0.04), [0.72, 0.58, 0.38], 0.55);
        e.list.solid(Mesh::Cube, t(x, 1.55, -14.45) * s(0.05, 1.2, 0.03), hex(0x20201e));
    }
    let sign = t(-1.98, 3.5, -14.42);
    e.list.solid(Mesh::Cube, t(0.0, 3.85, -14.5) * s(4.8, 1.05, 0.1), hex(0x18191c));
    crate::chalk::write(&mut e.list, sign * t(0.0, 0.0, 0.03), (0.0, 0.0), 0.7, "HOSTEL", [1.0, 0.75, 0.3]);
    e.lights.push((vec3(0.0, 3.85, -14.2), [1.0, 0.65, 0.3], 3.2));
    e.lights.push((vec3(0.0, 2.4, -13.7), [1.0, 0.75, 0.45], 1.1));
    let wood = hex(0x6a5236);
    for x in [-3.4f32, 3.4] {
        e.list.solid(Mesh::Cube, t(x, 0.45, -14.0) * s(1.7, 0.07, 0.45), wood);
        for sd in [-0.75f32, 0.75] {
            e.list.solid(Mesh::Cube, t(x + sd, 0.22, -14.0) * s(0.07, 0.44, 0.4), hex(0x26262a));
        }
    }
    // what collects round the back: crates, bins, bottles, bikes, pallets, cones
    for (x, z, n) in [(-7.0f32, -13.8f32, 3usize), (-6.0, -13.9, 2), (6.4, -13.8, 2), (7.1, -13.9, 3), (6.9, 13.7, 2), (-6.8, 13.7, 3), (4.6, 13.9, 1)] {
        stack(&mut e.list, &mut r, x, z, n, 0.4 * x);
    }
    for (x, col) in [(-1.8f32, 0x2f6a3a), (1.8, 0x3a3f46), (8.1, 0x2f6a3a)] {
        e.list.solid(Mesh::Cube, t(x, 0.55, -13.95) * s(0.8, 1.1, 0.75), hex(col));
        e.list.solid(Mesh::Cube, t(x, 1.13, -13.95) * rx(0.1) * s(0.85, 0.06, 0.8), hex(0x202226));
    }
    for k in 0..14 {
        let (x, z) = (-7.5 + r.f() * 15.0, if k % 2 == 0 { -13.3 + r.f() * 0.8 } else { 13.1 + r.f() * 1.0 });
        if r.f() < 0.45 {
            bottle(&mut e.list, t(x, 0.033, z) * ry(r.f() * 6.0) * rz(PI * 0.5) * t(0.0, -0.12, 0.0), [0.75, 0.72, 0.6]);
        } else {
            bottle(&mut e.list, t(x, 0.0, z), [0.75, 0.72, 0.6]);
        }
    }
    for (x, z) in [(-7.9f32, 12.2f32), (7.9, 12.2), (-7.9, -12.4)] {
        e.list.solid(Mesh::Cone, t(x, 0.22, z) * s(0.13, 0.44, 0.13), hex(0xd9581c));
        e.list.solid(Mesh::Cube, t(x, 0.01, z) * s(0.3, 0.02, 0.3), hex(0x1c1c1e));
    }
    e.list.solid(Mesh::Cube, t(3.0, 0.2, 13.9) * ry(0.2) * s(1.2, 0.14, 0.8), hex(0x8a6c44));
    e.list.solid(Mesh::Cube, t(3.0, 0.34, 13.9) * ry(0.5) * s(1.2, 0.14, 0.8), hex(0x7c6040));
    // the street lamps: the pole, the arm, the glow
    for (x, z) in [(-7.6f32, -9.0f32), (7.6, -9.0), (-7.6, 9.0), (7.6, 9.0)] {
        let sd = x.signum();
        e.list.solid(Mesh::Cyl, t(sd * 8.15, 2.7, z) * s(0.07, 5.4, 0.07), hex(0x2a2c30));
        e.list.solid(Mesh::Cube, t(sd * 7.85, 5.35, z) * s(0.65, 0.05, 0.05), hex(0x2a2c30));
        glowbox(&mut e.list, t(x, 5.28, z) * s(0.34, 0.06, 0.16), [1.0, 0.68, 0.3], 3.0);
        e.lights.push((vec3(x, 5.2, z), [1.0, 0.6, 0.22], 1.7));
    }
    // the fence behind the near line, and the town beyond it
    e.list.blend.push((Mesh::Cube, inst(t(0.0, 1.05, 14.7) * s(28.0, 2.1, 0.02), [0.45, 0.47, 0.5, 0.2], [kind::UNLIT, 0.0, 0.0, 1.0])));
    let mut i = -14.0;
    while i <= 14.0 {
        e.list.solid(Mesh::Cyl, t(i, 1.1, 14.7) * s(0.03, 2.2, 0.03), hex(0x54585c));
        i += 2.0;
    }
    e.list.solid(Mesh::Cube, t(0.0, 2.15, 14.7) * s(28.0, 0.04, 0.04), hex(0x54585c));
    let greens = [hex(0x2c4419), hex(0x35501e), hex(0x283f1c)];
    for k in 0..16 {
        let x = -24.0 + k as f32 * 3.2 + r.f();
        let g = greens[k % 3];
        if k % 2 == 0 {
            let (z, h) = (17.0 + r.f() * 3.0, 3.4 + r.f() * 2.0);
            crate::scene::tree(&mut e.list, &mut r, vec3(x, 0.0, z), h, 0.17, g);
        } else {
            let sc = 0.7 + r.f() * 0.4;
            crate::scene::bush(&mut e.list, &mut r, vec3(x, 0.0, 15.6), sc, g);
        }
    }
    for k in 0..22 {
        let x = -44.0 + k as f32 * 4.2;
        let (w, h, z) = (9.0 + r.f() * 6.0, 9.0 + r.f() * 12.0, 30.0 + r.f() * 14.0);
        let tone = 0.14 + r.f() * 0.12;
        e.list.push(Mesh::Cube, t(x, h * 0.5, z) * s(w, h, 9.0), [tone * 1.1, tone, tone * 0.95, 1.0], [WINDOWS, 0.0, 0.0, 0.0]);
    }
    for sd in [1.0f32, -1.0] {
        for k in 0..6 {
            let z = -24.0 + k as f32 * 9.5;
            let h = 8.0 + r.f() * 9.0;
            let tone = 0.15 + r.f() * 0.1;
            e.list.push(Mesh::Cube, t(sd * (20.0 + r.f() * 4.0), h * 0.5, z) * s(11.0, h, 9.0), [tone, tone, tone * 1.05, 1.0], [WINDOWS, 0.0, 0.0, 0.0]);
        }
    }
    // a church tower on the skyline
    e.list.solid(Mesh::Cube, t(-18.0, 13.0, 46.0) * s(5.0, 26.0, 5.0), hex(0x6d4a3a));
    e.list.solid(Mesh::Cone, t(-18.0, 30.0, 46.0) * s(2.9, 8.0, 2.9), hex(0x2c4a44));
    e
}

// ---------- Cooldown Café ----------
fn build_cafe() -> Env {
    let mut e = Env::new();
    let mut r = Rng::new(1887);
    let (hx, hz, h) = CAFE;
    // wallpaper a hundred years of smoke has turned the colour of tea, and a floor sanded by boots
    room(&mut e, hx, hz, h, (WOOD, hex(0x4e2e18), 0.0), (kind::PLASTER, hex(0x8a6230)), hex(0x24160c));
    let (dark, mid, brass, cream) = (hex(0x28160a), hex(0x4a2a14), hex(0xb08a3a), hex(0xe8d8b0));
    // a Persian runner down the pitch, the way the tables here wear them
    persian(&mut e.list, t(0.0, 0.004, 0.0), 2.6, 7.4);
    let sides = [(vec3(-1.0, 0.0, 0.0), vec3(hx, 0.0, 0.0), hz, -PI * 0.5), (vec3(1.0, 0.0, 0.0), vec3(-hx, 0.0, 0.0), hz, PI * 0.5), (vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, hz), hx, PI), (vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz), hx, 0.0)];
    for (k, (n, at, half, rot)) in sides.iter().enumerate() {
        // +z in `m` looks into the room, +x runs along the wall
        let m = t(at.x, 0.0, at.z) * ry(*rot);
        let d = e.wall(*n, *at);
        // panelling to shoulder height, a shelf on top, a row of Delft tiles above it
        d.solid(Mesh::Cube, m * t(0.0, 0.68, 0.09) * s(half * 2.0, 1.36, 0.06), mid);
        d.solid(Mesh::Cube, m * t(0.0, 1.38, 0.13) * s(half * 2.0, 0.05, 0.2), dark);
        d.solid(Mesh::Cube, m * t(0.0, 2.92, 0.09) * s(half * 2.0, 0.16, 0.08), dark);
        let mut x = -half + 0.5;
        while x < *half {
            d.solid(Mesh::Cube, m * t(x, 0.68, 0.125) * s(0.8, 1.1, 0.012), dark);
            d.solid(Mesh::Cube, m * t(x, 0.68, 0.13) * s(0.68, 0.98, 0.012), hex(0x5a3419));
            x += 1.0;
        }
        let mut x = -half + 0.14;
        let mut q = k;
        while x < *half {
            let c = if q % 2 == 0 { cream } else { hex(0x3b5a8a) };
            d.solid(Mesh::Cube, m * t(x, 1.52, 0.1) * s(0.13, 0.13, 0.01), c);
            q += 1;
            x += 0.15;
        }
        // full up to the ceiling with what a café collects: frames, shelves, plates
        let mut x = -half + 0.5;
        let mut q = k * 3;
        while x < half - 0.3 {
            let f = q as f32;
            let (w, hh, y) = (0.42 + (f * 1.3).sin().abs() * 0.34, 0.36 + (f * 2.1).sin().abs() * 0.28, 2.15 + (f * 0.9).sin() * 0.12);
            if q % 5 == 3 {
                d.solid(Mesh::Cyl, m * t(x, y, 0.1) * rx(PI * 0.5) * s(0.18, 0.02, 0.18), cream);
                d.solid(Mesh::Cyl, m * t(x, y, 0.11) * rx(PI * 0.5) * s(0.12, 0.012, 0.12), hex(0x3b5a8a));
            } else {
                d.solid(Mesh::Cube, m * t(x, y, 0.09) * s(w, hh, 0.04), hex(0x1c1008));
                let c = [hex(0x6b5a38), hex(0x3f4e3a), hex(0x7a4a2a), hex(0x8a7a58), hex(0x48383a)][q % 5];
                d.solid(Mesh::Cube, m * t(x, y, 0.11) * s(w - 0.08, hh - 0.08, 0.01), c);
            }
            x += w + 0.14;
            q += 1;
        }
    }
    // the bar along +x: dark wood, brass rail, taps, and a wall of jenever and glasses behind
    {
        let d = e.wall(vec3(-1.0, 0.0, 0.0), vec3(hx, 0.0, 0.0));
        d.solid(Mesh::Cube, t(hx - 0.14, 2.1, 0.0) * s(0.08, 1.5, 7.2), hex(0x1c0f06));
        for row in 0..3 {
            let y = 1.62 + row as f32 * 0.42;
            d.solid(Mesh::Cube, t(hx - 0.27, y, 0.0) * s(0.22, 0.03, 7.0), mid);
            let mut z = -3.4;
            let mut q = row;
            while z < 3.45 {
                let c = [hex(0x5a7a3a), hex(0xb07a2a), hex(0x7a2a1a), hex(0xd8c890), hex(0x2a3a5a), hex(0x8a5a2a)][q % 6];
                d.push(Mesh::Cyl, t(hx - 0.27, y + 0.13, z) * s(0.035, 0.24, 0.035), [c[0], c[1], c[2], 1.0], [kind::LIT, 0.25, 1.0, 0.0]);
                d.solid(Mesh::Cyl, t(hx - 0.27, y + 0.28, z) * s(0.014, 0.07, 0.014), c);
                z += 0.2;
                q += 1;
            }
        }
    }
    e.list.solid(Mesh::Cube, t(hx - 0.92, 0.56, 0.0) * s(0.6, 1.12, 7.4), hex(0x301a0c));
    e.list.solid(Mesh::Cube, t(hx - 0.96, 1.14, 0.0) * s(0.78, 0.05, 7.6), hex(0x170b05));
    e.list.solid(Mesh::Cyl, t(hx - 1.34, 0.22, 0.0) * rx(PI * 0.5) * s(0.03, 7.3, 0.03), brass);
    let mut z = -3.3;
    while z < 3.31 {
        e.list.solid(Mesh::Cube, t(hx - 1.23, 0.56, z) * s(0.012, 0.95, 0.5), hex(0x3f2412));
        z += 0.66;
    }
    for z in [-1.0f32, 0.0, 1.0] {
        e.list.solid(Mesh::Cyl, t(hx - 1.02, 1.36, z) * s(0.03, 0.42, 0.03), brass);
        e.list.solid(Mesh::Cube, t(hx - 1.1, 1.52, z) * s(0.16, 0.04, 0.04), brass);
        e.list.solid(Mesh::Sphere, t(hx - 1.02, 1.62, z) * s(0.05, 0.09, 0.05), cream);
    }
    let mut z = -3.2f32;
    while z < 3.2 {
        bottle(&mut e.list, t(hx - 0.98 + (z * 3.0).sin() * 0.14, 1.165, z + 0.2), [0.86, 0.8, 0.6]);
        z += 0.55;
    }
    // the windows on −x: leaded amber glass, plants on the sill, the street's glow behind
    {
        let d = e.wall(vec3(1.0, 0.0, 0.0), vec3(-hx, 0.0, 0.0));
        for z in [-5.6f32, -2.0, 2.0, 5.6] {
            d.solid(Mesh::Cube, t(-hx + 0.13, 2.05, z) * s(0.03, 1.45, 2.3), dark);
            for iy in 0..3 {
                for iz in 0..5 {
                    let warm = if (iy + iz) % 2 == 0 { [1.0, 0.62, 0.22] } else { [0.95, 0.42, 0.14] };
                    glowbox(d, t(-hx + 0.15, 1.6 + iy as f32 * 0.45, z - 0.9 + iz as f32 * 0.45) * s(0.01, 0.41, 0.41), warm, 0.55);
                }
            }
        }
    }
    // the café's name in white curly letters, at both ends, over a lit doorway and a mirror
    for (z, rot, x0) in [(-hz + 0.14, 0.0f32, -2.5f32), (hz - 0.14, PI, 2.5)] {
        let n = if z < 0.0 { vec3(0.0, 0.0, 1.0) } else { vec3(0.0, 0.0, -1.0) };
        let d = e.wall(n, vec3(0.0, 0.0, if z < 0.0 { -hz } else { hz }));
        d.solid(Mesh::Cube, t(0.0, 2.48, z) * s(5.4, 0.62, 0.03), hex(0x140a05));
        crate::chalk::write(d, t(x0, 2.28, z) * ry(rot) * t(0.0, 0.0, 0.03), (0.0, 0.0), 0.4, "CAFE DE KRUL", [1.0, 0.96, 0.82]);
    }
    {
        let d = e.wall(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz));
        // the dartboard
        d.solid(Mesh::Cyl, t(-4.4, 1.85, -hz + 0.16) * rx(PI * 0.5) * s(0.3, 0.05, 0.3), hex(0x141414));
        for (rr, c) in [(0.24f32, 0x8a1c1c), (0.17, 0xd8c890), (0.1, 0x1c5a2c), (0.03, 0xc01c1c)] {
            d.solid(Mesh::Cyl, t(-4.4, 1.85, -hz + 0.19) * rx(PI * 0.5) * s(rr, 0.012, rr), hex(c));
        }
        // the jukebox, the only thing here that's not brown
        d.solid(Mesh::Cube, t(4.6, 0.72, -hz + 0.42) * s(0.95, 1.44, 0.55), hex(0x4a1c1c));
        glowbox(d, t(4.6, 1.02, -hz + 0.71) * s(0.72, 0.66, 0.02), [1.0, 0.45, 0.16], 2.0);
        glowbox(d, t(4.6, 0.46, -hz + 0.71) * s(0.72, 0.12, 0.02), [0.3, 0.8, 1.0], 1.2);
    }
    // little tables with their carpets, candles, stools and glasses, wherever the game leaves room
    for (x, z) in [(-5.9f32, -4.4f32), (-5.9, -1.5), (-5.9, 1.5), (-5.9, 4.4), (4.4, 7.9), (-2.0, 8.05), (2.0, -8.05), (-2.6, -8.05)] {
        e.list.solid(Mesh::Cyl, t(x, 0.37, z) * s(0.05, 0.74, 0.05), dark);
        e.list.solid(Mesh::Cyl, t(x, 0.74, z) * s(0.42, 0.02, 0.42), hex(0x6a1f1a));
        e.list.solid(Mesh::Cyl, t(x, 0.65, z) * s(0.44, 0.18, 0.44), hex(0x7a2a20));
        e.list.solid(Mesh::Cyl, t(x, 0.8, z) * s(0.03, 0.1, 0.03), cream);
        e.lights.push((vec3(x, 0.9, z), [1.0, 0.62, 0.25], 0.42));
        bottle(&mut e.list, t(x + 0.18, 0.75, z - 0.1), [0.86, 0.8, 0.6]);
        bottle(&mut e.list, t(x - 0.16, 0.75, z + 0.14), [0.86, 0.8, 0.6]);
        for a in [0.6f32, 2.6, 4.4] {
            let (cx, cz) = (x + a.cos() * 0.62, z + a.sin() * 0.62);
            if cx.abs() < hx - 0.25 && cz.abs() < hz - 0.25 {
                e.list.solid(Mesh::Cyl, t(cx, 0.24, cz) * s(0.17, 0.48, 0.17), hex(0x3a2010));
            }
        }
    }
    for (x, z, n) in [(-6.2f32, -8.1f32, 3usize), (-6.2, 8.1, 3), (5.9, -8.2, 2), (6.2, 8.2, 3)] {
        stack(&mut e.list, &mut r, x, z, n, 0.3 * x);
    }
    for (x, z) in [(5.5f32, 5.1f32), (5.5, -5.1)] {
        e.list.solid(Mesh::Cyl, t(x, 0.45, z) * s(0.32, 0.9, 0.32), hex(0x5a361c));
        for y in [0.2f32, 0.7] {
            e.list.solid(Mesh::Cyl, t(x, y, z) * s(0.33, 0.04, 0.33), hex(0x1c1c1e));
        }
    }
    for (x, z) in [(-6.2f32, 6.4f32), (-6.2, -6.4)] {
        plant(&mut e.list, x, z, 1.15);
    }
    // the ceiling: dark beams, fringed lamps, hanging glasses; and it's a party, so orange bunting,
    // fairy lights and a small mirror ball. All of it goes with the ceiling when looked at from above.
    {
        let d = e.wall(vec3(0.0, -1.0, 0.0), vec3(0.0, h, 0.0));
        let mut z = -hz + 0.8;
        while z < hz {
            d.solid(Mesh::Cube, t(0.0, h - 0.1, z) * s(hx * 2.0, 0.2, 0.22), hex(0x170c05));
            z += 1.8;
        }
        for (x, z) in [(-3.4f32, -4.6f32), (3.4, -4.6), (-3.4, 4.6), (3.4, 4.6), (0.0, 0.0), (5.6, 0.0)] {
            d.solid(Mesh::Cyl, t(x, h - 0.32, z) * s(0.012, 0.64, 0.012), dark);
            d.solid(Mesh::Cone, t(x, 2.5, z) * s(0.3, 0.22, 0.3), hex(0x6a2010));
            d.solid(Mesh::Cyl, t(x, 2.37, z) * s(0.31, 0.07, 0.31), hex(0x8a5e20));
            d.push(Mesh::Sphere, t(x, 2.38, z) * su(0.06), [1.0, 0.8, 0.5, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
        }
        for q in 0..30 {
            let k = q as f32 / 30.0;
            d.solid(Mesh::Cyl, t(hx - 1.0, h - 0.34, -3.4 + 6.8 * k) * s(0.03, 0.12, 0.03), cream);
        }
        for (x0, z0, x1, z1) in [(-hx, -6.2f32, hx, -6.2f32), (-hx, -2.1, hx, -2.1), (-hx, 2.1, hx, 2.1), (-hx, 6.2, hx, 6.2), (-5.0, -hz, -5.0, hz), (5.0, -hz, 5.0, hz)] {
            for q in 0..24 {
                let k = q as f32 / 24.0;
                let (x, z, y) = (x0 + (x1 - x0) * k, z0 + (z1 - z0) * k, h - 0.34 - (k * PI).sin() * 0.3);
                let c = [[1.0, 0.42, 0.04], [1.0, 1.0, 0.95], [0.1, 0.25, 0.65], [0.8, 0.1, 0.1]][q % 4];
                d.solid(Mesh::Cone, t(x, y - 0.06, z) * rx(PI) * ry(if z0 == z1 { 0.0 } else { PI * 0.5 }) * s(0.055, 0.1, 0.012), c);
                if q % 2 == 0 {
                    d.push(Mesh::Sphere, t(x, y + 0.02, z) * su(0.025), [1.0, 0.82, 0.5, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
                }
            }
        }
        d.push(Mesh::Sphere, t(0.0, h - 0.5, 0.0) * su(0.15), [0.5, 0.5, 0.55, 1.0], [kind::LIT, 0.1, 0.0, 0.0]);
    }
    // regulars along the bar and the back wall
    for (x, z, c) in [(4.75f32, -3.1f32, 0x7a3a2a), (4.8, -1.8, 0x2a4a6a), (4.75, -0.4, 0xd8701a), (4.8, 1.1, 0x4a5a3a), (4.75, 2.5, 0xd8701a), (4.8, 3.6, 0x6a2a4a), (-3.6, 8.2, 0xd8701a), (0.0, 8.3, 0x3a3a4a), (3.2, 8.25, 0x8a6a2a), (-0.4, -8.3, 0xd8701a), (3.9, -8.25, 0x2a5a4a), (-6.1, 0.0, 0x7a2a2a), (-6.2, 3.0, 0xd8701a), (-6.2, -3.0, 0x2a3a6a)] {
        punter(&mut e.list, x, z, (-x).atan2(-z), 0.0, 0.0, hex(c), r.f());
    }
    e
}

/// A patterned carpet, from layers of stripes: red ground, navy border, gold lines, a row of medallions.
fn persian(dl: &mut DrawList, m: Mat4, hx: f32, hz: f32) {
    let lay = |dl: &mut DrawList, y: f32, a: f32, b: f32, c: u32| {
        let col = hex(c);
        dl.push(Mesh::Cube, m * t(0.0, y, 0.0) * s(a * 2.0, 0.003, b * 2.0), [col[0], col[1], col[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    };
    lay(dl, 0.0, hx, hz, 0x1a1020);
    lay(dl, 0.002, hx - 0.12, hz - 0.12, 0x6a5228);
    lay(dl, 0.004, hx - 0.17, hz - 0.17, 0x141c34);
    lay(dl, 0.006, hx - 0.42, hz - 0.42, 0x6a5228);
    lay(dl, 0.008, hx - 0.47, hz - 0.47, 0x46100c);
    let mut z = -hz + 1.0;
    while z < hz - 0.6 {
        for (rr, c, y) in [(0.46f32, 0x141c34, 0.011f32), (0.38, 0x6a5228, 0.013), (0.3, 0x56180e, 0.015), (0.1, 0x8a7a50, 0.017)] {
            let col = hex(c);
            dl.push(Mesh::Disc, m * t(0.0, y, z) * s(rr * 1.25, 1.0, rr), [col[0], col[1], col[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        }
        z += 1.75;
    }
    let mut z = -hz + 0.34;
    let mut q = 0;
    while z < hz - 0.2 {
        for x in [-hx + 0.29, hx - 0.29] {
            let c = hex(if q % 2 == 0 { 0x8a7a50 } else { 0x56180e });
            dl.push(Mesh::Cube, m * t(x, 0.008, z) * ry(PI * 0.25) * s(0.12, 0.003, 0.12), [c[0], c[1], c[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        }
        q += 1;
        z += 0.26;
    }
}

/// Somebody in the crowd: not a player, just a shape with a colour. `hype` raises the arms, `y` is the floor they're on.
#[allow(clippy::too_many_arguments)]
fn punter(dl: &mut DrawList, x: f32, z: f32, yaw: f32, y: f32, hype: f32, shirt: [f32; 3], seed: f32) {
    dl.blend.push((Mesh::Quad, inst(t(x, y + 0.012, z) * rx(-PI * 0.5) * su(0.8), [0.0, 0.0, 0.0, 0.55], [kind::SOFT, 0.0, 0.0, 0.0])));
    let m = t(x, y, z) * ry(yaw);
    let skin = [hex(0xd8a080), hex(0xb07a56), hex(0x7a5236), hex(0xe2b89a)][(seed * 4.0) as usize % 4];
    let hair = [hex(0x1c1410), hex(0x4a2e18), hex(0x8a6a3a), hex(0x2a2a2a)][(seed * 11.0) as usize % 4];
    let legs = [hex(0x1c2438), hex(0x222226), hex(0x3a3028)][(seed * 7.0) as usize % 3];
    for sd in [-0.09f32, 0.09] {
        dl.solid(Mesh::Cyl, m * t(sd, 0.42, 0.0) * s(0.075, 0.84, 0.08), legs);
    }
    dl.solid(Mesh::Cyl, m * t(0.0, 1.14, 0.0) * s(0.19, 0.62, 0.13), shirt);
    dl.solid(Mesh::Sphere, m * t(0.0, 1.58, 0.0) * su(0.115), skin);
    dl.solid(Mesh::Sphere, m * t(0.0, 1.63, -0.015) * s(0.12, 0.09, 0.12), hair);
    for sd in [-1.0f32, 1.0] {
        let up = 2.75 * hype + 0.12;
        let arm = m * t(sd * 0.23, 1.38, 0.0) * rz(sd * 0.22) * rx(-up) * t(0.0, -0.3, 0.0);
        dl.solid(Mesh::Cyl, arm * s(0.05, 0.6, 0.05), if hype > 0.5 { skin } else { shirt });
    }
}

// ---------- Duplex: the glass cube on the roof, over Wenceslas Square ----------
fn build_club() -> Env {
    let mut e = Env::new();
    let mut r = Rng::new(420);
    let (hx, hz, h) = CLUB;
    e.list.push(Mesh::Sphere, su(220.0), [1.0; 4], [kind::SKY, 0.0, 0.0, 0.0]);
    // an ordinary club floor, worn dark wood
    surf(&mut e.list, Mesh::Plane, s(hx * 2.0, 1.0, hz * 2.0), hex(0x5a4436), WOOD, 0.0, false);
    let steel = hex(0x101116);
    // walls of glass in black steel, with the city showing through
    for (n, at, half, rot) in [(vec3(-1.0, 0.0, 0.0), vec3(hx, 0.0, 0.0), hz, PI * 0.5), (vec3(1.0, 0.0, 0.0), vec3(-hx, 0.0, 0.0), hz, PI * 0.5), (vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, hz), hx, 0.0), (vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz), hx, 0.0)] {
        let m = t(at.x, 0.0, at.z) * ry(rot);
        let d = e.wall(n, at);
        d.blend.push((Mesh::Cube, inst(m * t(0.0, h * 0.5, 0.0) * s(half * 2.0, h, 0.03), [0.4, 0.55, 0.8, 0.1], [kind::UNLIT, 0.0, 0.0, 1.0])));
        let mut x = -half;
        while x <= half + 0.01 {
            d.solid(Mesh::Cube, m * t(x, h * 0.5, 0.0) * s(0.09, h, 0.14), steel);
            x += 1.84;
        }
        for y in [0.05f32, 1.75, 3.55, h - 0.05] {
            d.solid(Mesh::Cube, m * t(0.0, y, 0.0) * s(half * 2.0, 0.09, 0.14), steel);
        }
        glowbox(d, m * t(0.0, 0.14, 0.06) * s(half * 2.0, 0.03, 0.02), [1.0, 0.32, 0.06], 2.2);
    }
    // the barriers that keep the crowd off the pitch, and send the ball back
    let rail = hex(0x9a9ea6);
    let (bx, bz, side) = (4.63, 7.73, 6.25);
    for (x, z, w, d) in [(bx, 0.0f32, 0.05f32, side * 2.0), (-bx, 0.0, 0.05, side * 2.0), (0.0, bz, 13.0, 0.05), (0.0, -bz, 13.0, 0.05), (5.55, side, 1.9, 0.05), (-5.55, side, 1.9, 0.05), (5.55, -side, 1.9, 0.05), (-5.55, -side, 1.9, 0.05), (6.5, bz - 0.74, 0.05, 1.48), (-6.5, bz - 0.74, 0.05, 1.48), (6.5, -bz + 0.74, 0.05, 1.48), (-6.5, -bz + 0.74, 0.05, 1.48)] {
        e.list.solid(Mesh::Cube, t(x, 1.02, z) * s(w, 0.05, d), rail);
        e.list.solid(Mesh::Cube, t(x, 0.34, z) * s(w, 0.04, d), rail);
        let len = w.max(d);
        let n = (len as i32).max(1);
        for k in 0..=n {
            let f = k as f32 / n as f32 - 0.5;
            let (px, pz) = (x + if d > w { 0.0 } else { w * f }, z + if d > w { d * f } else { 0.0 });
            e.list.solid(Mesh::Cube, t(px, 0.52, pz) * s(0.03, 1.04, 0.03), rail);
        }
    }
    // the stage beyond the far barrier: the DJ up high, speaker towers, the LED wall (in `dynamic`) behind
    let (sh, sz) = STAGE;
    {
        let d = e.wall(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz));
        d.solid(Mesh::Cube, t(0.0, h * 0.5, -hz + 0.2) * s(hx * 2.0 - 0.2, h - 0.1, 0.12), hex(0x050508));
    }
    let depth = hz + sz;
    e.list.solid(Mesh::Cube, t(0.0, sh * 0.5, sz - depth * 0.5) * s(11.0, sh, depth), hex(0x0e0e12));
    for q in 0..3 {
        glowbox(&mut e.list, t(0.0, 0.35 + q as f32 * 0.52, sz + 0.01) * s(11.0, 0.03, 0.02), [1.0, 0.3, 0.1], 2.0);
    }
    e.list.solid(Mesh::Cube, t(0.0, sh + 0.52, sz - 1.1) * s(3.2, 1.04, 0.7), hex(0x15151c));
    glowbox(&mut e.list, t(0.0, sh + 0.55, sz - 0.74) * s(2.8, 0.05, 0.02), [0.3, 0.9, 1.0], 2.4);
    for sx in [-0.8f32, 0.8] {
        e.list.solid(Mesh::Cyl, t(sx, sh + 1.06, sz - 1.1) * s(0.22, 0.03, 0.22), hex(0x2a2a30));
    }
    for sx in [-1.0f32, 1.0] {
        for q in 0..3 {
            let y = sh + 0.6 + q as f32 * 1.15;
            e.list.solid(Mesh::Cube, t(sx * 4.6, y, sz - 0.7) * s(1.05, 1.1, 0.9), hex(0x08080b));
            for yy in [y - 0.25, y + 0.25] {
                e.list.push(Mesh::Sphere, t(sx * 4.6, yy, sz - 0.24) * s(0.24, 0.24, 0.04), [0.1, 0.1, 0.12, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
            }
        }
    }
    // the bar across +z, back-lit, its shelves full
    e.list.solid(Mesh::Cube, t(0.0, 0.55, hz - 0.62) * s(10.0, 1.1, 0.55), hex(0x14141a));
    e.list.solid(Mesh::Cube, t(0.0, 1.12, hz - 0.65) * s(10.2, 0.05, 0.75), hex(0xc8ccd4));
    glowbox(&mut e.list, t(0.0, 0.2, hz - 0.92) * s(10.0, 0.03, 0.02), [1.0, 0.5, 0.1], 2.6);
    let mut x = -4.8;
    while x < 4.81 {
        bottle(&mut e.list, t(x, 1.145, hz - 0.65), [0.86, 0.8, 0.6]);
        x += 0.8;
    }
    {
        let d = e.wall(vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, hz));
        glowbox(d, t(0.0, 2.15, hz - 0.1) * s(10.4, 1.9, 0.03), [1.0, 0.45, 0.12], 0.55);
        for row in 0..3 {
            let y = 1.55 + row as f32 * 0.55;
            d.solid(Mesh::Cube, t(0.0, y, hz - 0.17) * s(10.4, 0.03, 0.14), hex(0x101014));
            let mut x = -5.0;
            while x < 5.01 {
                let c = [hex(0x6a2a10), hex(0x304a20), hex(0x1a2a4a), hex(0x8a6a20), hex(0x501428)][((x * 3.0 + row as f32) as i32).unsigned_abs() as usize % 5];
                d.solid(Mesh::Cyl, t(x, y + 0.16, hz - 0.17) * s(0.05, 0.3, 0.05), c);
                x += 0.2;
            }
        }
        d.solid(Mesh::Cube, t(0.0, 4.0, hz - 0.1) * s(5.2, 1.0, 0.05), hex(0x07070a));
        crate::chalk::write(d, t(2.2, 3.66, hz - 0.14) * ry(PI), (0.0, 0.0), 0.7, "DUPLEX", [1.0, 0.25, 0.75]);
    }
    {
        let d = e.wall(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz));
        d.solid(Mesh::Cube, t(-0.95, 5.95, -hz + 0.3) * s(6.4, 1.3, 0.04), hex(0x07070a));
        crate::chalk::write(d, t(-4.0, 5.5, -hz + 0.34), (0.0, 0.0), 0.9, "DUPLEX", [1.0, 0.9, 0.8]);
    }
    // the roof: a truss all round and across, packed with lights, strings of bulbs, two mirror balls
    {
        let d = e.wall(vec3(0.0, -1.0, 0.0), vec3(0.0, h, 0.0));
        d.solid(Mesh::Cube, t(0.0, h + 0.06, 0.0) * s(hx * 2.0, 0.12, hz * 2.0), hex(0x07070c));
        let tr = hex(0x24252c);
        for x in [-7.4f32, -2.5, 2.5, 7.4] {
            d.solid(Mesh::Cube, t(x, 5.9, 0.0) * s(0.24, 0.24, hz * 2.0 - 0.3), tr);
        }
        for z in [-11.4f32, -3.8, 3.8, 11.4] {
            d.solid(Mesh::Cube, t(0.0, 5.9, z) * s(hx * 2.0 - 0.3, 0.24, 0.24), tr);
        }
        for (x, z) in [(0.0f32, 0.0f32), (4.2, -7.0)] {
            d.push(Mesh::Sphere, t(x, 4.6, z) * su(0.28), [0.5, 0.5, 0.55, 1.0], [kind::LIT, 0.1, 0.0, 0.0]);
            d.solid(Mesh::Cyl, t(x, 5.3, z) * s(0.01, 1.2, 0.01), hex(0x888888));
        }
    }
    // Prague: seven floors down, the square as a river of light, the Museum at its head
    surf(&mut e.list, Mesh::Plane, t(0.0, -24.0, 0.0) * s(300.0, 1.0, 300.0), hex(0x101018), kind::LIT, 0.0, false);
    for k in 0..30 {
        for sd in [-1.0f32, 1.0] {
            let z = -96.0 + k as f32 * 3.6;
            e.list.push(Mesh::Sphere, t(sd * 5.5, -18.0, z) * su(0.35), [1.0, 0.8, 0.45, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
        }
    }
    for k in 0..70 {
        let a = k as f32 / 70.0 * PI * 2.0 + r.f() * 0.08;
        let dist = 26.0 + r.f() * 78.0;
        let (x, z) = (a.sin() * dist, a.cos() * dist);
        if x.abs() < 13.0 && z < -14.0 {
            continue;
        }
        let (w, dd, hb) = (8.0 + r.f() * 11.0, 8.0 + r.f() * 11.0, 10.0 + r.f() * 16.0);
        let tone = 0.06 + r.f() * 0.08;
        e.list.push(Mesh::Cube, t(x, hb * 0.5 - 24.0, z) * ry(a) * s(w, hb, dd), [tone, tone * 0.95, tone * 1.1, 1.0], [WINDOWS, 0.0, 0.0, 0.0]);
        e.list.solid(Mesh::Cone, t(x, hb - 24.0 + 2.0, z) * ry(a) * s(w * 0.62, 4.0, dd * 0.62), [0.13, 0.06, 0.05]);
    }
    // the National Museum, floodlit, and a few spires
    let stone = [0.75, 0.56, 0.3];
    glowbox(&mut e.list, t(0.0, -14.0, -104.0) * s(60.0, 20.0, 12.0), stone, 0.55);
    glowbox(&mut e.list, t(0.0, -1.0, -104.0) * s(12.0, 8.0, 12.0), stone, 0.6);
    e.list.push(Mesh::Hemi, t(0.0, 3.0, -104.0) * s(6.5, 7.5, 6.5), [0.36, 0.5, 0.42, 1.0], [kind::LIT, 0.5, 0.0, 0.0]);
    for (x, z, hb) in [(38.0f32, -70.0f32, 40.0f32), (46.0, -64.0, 34.0), (-56.0, 40.0, 42.0), (-48.0, 46.0, 42.0), (60.0, 34.0, 30.0)] {
        glowbox(&mut e.list, t(x, hb * 0.5 - 24.0, z) * s(4.0, hb, 4.0), [0.5, 0.38, 0.24], 0.35);
        e.list.push(Mesh::Cone, t(x, hb - 24.0 + 6.0, z) * s(2.6, 12.0, 2.6), [0.1, 0.1, 0.12, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    }
    e
}

// ---------- the training hall ----------
fn build_gym() -> Env {
    let mut e = Env::new();
    let (hx, hz, h) = (9.4, 13.4, 5.0);
    // the training hall is the barn behind the clubhouse: same planks, and its front wall opens into the shed
    room_h(&mut e, hx, hz, h, (WOOD, hex(0xc08a52), 1.0), (WOOD, hex(0x8a6a48)), hex(0x5a4430), [None, None, Some((0.0, 7.2, HUB_H)), None]);
    // court lines
    for (x, z, w, d) in [(0.0f32, 0.0f32, 12.0f32, 0.06f32), (-6.0, 0.0, 0.06, 20.0), (6.0, 0.0, 0.06, 20.0), (0.0, 10.0, 12.0, 0.06), (0.0, -10.0, 12.0, 0.06)] {
        e.list.push(Mesh::Cube, t(x, 0.004, z) * s(w, 0.006, d), [0.75, 0.15, 0.12, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    }
    e.list.push(Mesh::RingThin, t(0.0, 0.005, 0.0) * su(2.5), [0.75, 0.15, 0.12, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    {
        let dl = e.wall(vec3(1.0, 0.0, 0.0), vec3(-hx, 0.0, 0.0));
        // wall bars
        for k in 0..8 {
            let z = -6.0 + k as f32 * 0.9;
            dl.solid(Mesh::Cube, t(-hx + 0.15, 1.3, z) * s(0.06, 2.6, 0.06), hex(0x9a6a3a));
            if k < 7 {
                for j in 0..12 {
                    dl.solid(Mesh::Cyl, t(-hx + 0.15, 0.2 + j as f32 * 0.2, z + 0.45) * rx(PI * 0.5) * s(0.02, 0.9, 0.02), hex(0xb8844a));
                }
            }
        }
    }
    for (n, at) in [(vec3(-1.0, 0.0, 0.0), vec3(hx, 0.0, 0.0)), (vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, hz)), (vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -hz))] {
        let dl = e.wall(n, at);
        let along = vec3(n.z, 0.0, -n.x);
        for k in -3..=3 {
            let p = at + n * 0.07 + along * (k as f32 * 2.4);
            dl.push(Mesh::Cube, Mat4::from_translation(vec3(p.x, 3.9, p.z)) * ry(along.x.atan2(along.z)) * s(0.02, 1.0, 1.6), [0.5, 0.62, 0.8, 1.0], [kind::LIT, 0.5, 0.0, 0.0]);
            dl.solid(Mesh::Cube, Mat4::from_translation(vec3(p.x, 3.9, p.z) + n * 0.02) * ry(along.x.atan2(along.z)) * s(0.03, 0.05, 1.6), hex(0x2a1c12));
        }
    }
    // mats and benches
    for (x, z) in [(-7.6f32, 8.0f32), (-7.6, 10.2), (7.6, -9.0)] {
        e.list.solid(Mesh::Cube, t(x, 0.06, z) * s(1.8, 0.12, 2.0), hex(0x2a5a8a));
    }
    for z in [-4.0f32, 4.0] {
        e.list.solid(Mesh::Cube, t(8.3, 0.42, z) * s(0.35, 0.06, 3.0), hex(0x9a6a3a));
        e.list.solid(Mesh::Cube, t(8.3, 0.2, z) * s(0.3, 0.4, 0.08), hex(0x555555));
    }
    // industrial pendant lamps over the court (they are the hall's four lamps in its Look)
    for (x, z) in [(-4.2f32, -6.5f32), (4.2, -6.5), (-4.2, 6.5), (4.2, 6.5)] {
        e.list.solid(Mesh::Cyl, t(x, 4.78, z) * s(0.012, 0.44, 0.012), hex(0x222222));
        e.list.push(Mesh::Cone, t(x, 4.56, z) * s(0.42, 0.26, 0.42), [0.16, 0.2, 0.18, 1.0], [kind::LIT, 0.0, 1.0, 0.0]);
        e.list.push(Mesh::Disc, t(x, 4.43, z) * rx(PI) * su(0.36), [1.0, 0.93, 0.8, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
        e.lights.push((vec3(x, 4.38, z), [1.0, 0.9, 0.72], 1.0));
    }
    e
}

// ---------- the clubhouse shed (laid out like the JavaScript version) ----------
pub const BUZZ: [u32; 6] = [0xff4f9a, 0xff8a2a, 0x7ad13a, 0x8a5ad0, 0x22c4d8, 0xffd23f];
const CRATE_COLS: [u32; 6] = [0x1d5b2f, 0x1d5b2f, 0x8a1c1c, 0xc9a227, 0x1f3f7a, 0x2f6b2a];
pub fn buzz(dl: &mut DrawList, m: Mat4, k: usize) {
    // a round, brightly coloured cocktail can with a band and a lid
    let col = hex(BUZZ[k % 6]);
    let band = hex(BUZZ[(k + 3) % 6]);
    dl.push(Mesh::Sphere, m * t(0.0, 0.052, 0.0) * su(0.052), [col[0], col[1], col[2], 1.0], [kind::LIT, 0.08, 1.0, 0.0]);
    dl.push(Mesh::Cyl, m * t(0.0, 0.052, 0.0) * s(0.0535, 0.018, 0.0535), [band[0], band[1], band[2], 1.0], [kind::LIT, 0.08, 0.0, 0.0]);
    dl.solid(Mesh::Cyl, m * t(0.0, 0.112, 0.0) * s(0.021, 0.018, 0.021), [0.75, 0.75, 0.76]);
}
fn pack(dl: &mut DrawList, m: Mat4, k: usize) {
    // a pack of cigarettes, no brand
    let (body, top) = [(0x3d3a2a, 0xc8c2a8), (0xf2f0ea, 0xb8261e), (0xf2f0ea, 0xc9a227), (0x1c2a4a, 0xe0e0e0)][k % 4];
    dl.solid(Mesh::Cube, m * t(0.0, 0.044, 0.0) * s(0.056, 0.088, 0.024), hex(body));
    dl.solid(Mesh::Cube, m * t(0.0, 0.078, 0.0) * s(0.0565, 0.022, 0.0245), hex(top));
    dl.solid(Mesh::Cube, m * t(0.0, 0.02, 0.0125) * s(0.046, 0.026, 0.001), [0.9, 0.9, 0.9]);
}
fn crate_c(dl: &mut DrawList, x: f32, y: f32, z: f32, ry_: f32, col: u32) {
    let m = t(x, y, z) * ry(ry_);
    // plastic crates look darker than their colour name: the JavaScript version tinted a grey texture
    let c = hex(col);
    open_crate(dl, m, [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6]);
}
fn stack(dl: &mut DrawList, r: &mut Rng, x: f32, z: f32, n: usize, ry_: f32) {
    for i in 0..n {
        let col = CRATE_COLS[(r.f() * 6.0) as usize % 6];
        crate_c(dl, x + (r.f() - 0.5) * 0.04, i as f32 * 0.3, z + (r.f() - 0.5) * 0.04, ry_ + (r.f() - 0.5) * 0.12, col);
    }
}
fn butt(dl: &mut DrawList, r: &mut Rng, x: f32, y: f32, z: f32) {
    let c = if r.f() < 0.5 { hex(0xd08a3a) } else { hex(0xeeeeee) };
    dl.solid(Mesh::Cyl, t(x, y, z) * ry(r.f() * 6.0) * rx(PI * 0.5) * s(0.0045, 0.03, 0.0045), c);
}
fn build_hub() -> Env {
    let mut e = Env::new();
    let mut r = Rng::new(29);
    let (w, l, h) = (3.6f32, 4.6f32, 3.0f32);
    room_h(&mut e, w, l, h, (WOOD, hex(0x60422a), 0.0), (WOOD, hex(0x76563a)), hex(0x3a2a1c), [None, None, None, Some((DOOR_X, DOOR_W, DOOR_H))]);
    let dl = &mut e.list;
    // rafters
    let beam = hex(0x4a3420);
    let mut z = -l + 0.6;
    while z < l {
        dl.solid(Mesh::Cube, t(0.0, h - 0.12, z) * s(2.0 * w, 0.16, 0.12), beam);
        z += 1.5;
    }
    dl.solid(Mesh::Cube, t(0.0, h - 0.3, 0.0) * s(0.14, 0.18, 2.0 * l), beam);
    // the back wall: a window with daylight, a dartboard, the chalkboard with your records
    {
        let bw = e.wall(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -l));
        // the barn door into the training hall, propped open
        for x in [DOOR_X - DOOR_W / 2.0 - 0.05, DOOR_X + DOOR_W / 2.0 + 0.05] {
            bw.solid(Mesh::Cube, t(x, DOOR_H / 2.0, -l + 0.08) * s(0.1, DOOR_H, 0.16), hex(0x4a3420));
        }
        bw.solid(Mesh::Cube, t(DOOR_X, DOOR_H + 0.05, -l + 0.08) * s(DOOR_W + 0.3, 0.1, 0.16), hex(0x4a3420));
        bw.push(Mesh::Cube, t(DOOR_X + DOOR_W / 2.0 + 0.55, DOOR_H / 2.0, -l + 0.6) * ry(-1.2) * s(DOOR_W * 0.95, DOOR_H - 0.05, 0.05), [0.33, 0.23, 0.15, 1.0], [WOOD, 0.0, 1.0, 0.0]);
        for (k, rr) in [0.28f32, 0.24, 0.2, 0.16, 0.12, 0.08, 0.04].iter().enumerate() {
            let c = if k % 2 == 0 { hex(0x151515) } else { hex(0xe8e0c8) };
            bw.push(Mesh::Disc, t(2.55, 1.75, -l + 0.075 + k as f32 * 0.004) * rx(PI * 0.5) * su(*rr), [c[0], c[1], c[2], 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        }
        bw.push(Mesh::Disc, t(2.55, 1.75, -l + 0.105) * rx(PI * 0.5) * su(0.025), [0.72, 0.15, 0.12, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        let (bc, bwid, bh) = BOARD;
        bw.solid(Mesh::Cube, t(bc.x, bc.y, -l + 0.075) * s(bwid + 0.1, bh + 0.1, 0.03), hex(0x5a3a20));
        bw.push(Mesh::Cube, t(bc.x, bc.y, -l + 0.095) * s(bwid, bh, 0.012), [0.07, 0.09, 0.085, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
        bw.solid(Mesh::Cube, t(bc.x, bc.y - bh / 2.0 - 0.05, -l + 0.11) * s(bwid + 0.1, 0.05, 0.06), hex(0x5a3a20));
    }
    // the left wall: a door, the fridge, a FLUNKIEBAL pennant
    {
        let lw = e.wall(vec3(1.0, 0.0, 0.0), vec3(-w, 0.0, 0.0));
        lw.push(Mesh::Cube, t(-w + 0.07, 1.05, 1.9) * s(0.02, 2.1, 1.0), [0.33, 0.22, 0.14, 1.0], [WOOD, 0.0, 0.0, 0.0]);
        lw.solid(Mesh::Cube, t(-w + 0.1, 1.0, 2.25) * s(0.04, 0.04, 0.14), hex(0x777777));
        // the pennant: a long gold triangle
        lw.solid(Mesh::Cone, t(-w + 0.08, 2.25, -0.6) * rx(PI * 0.5) * rz(0.0) * s(0.3, 1.9, 0.01), hex(0xf2b632));
    }
    dl_fridge(&mut e.list, w);
    let dl = &mut e.list;
    // beer crates in every colour against the walls
    stack(dl, &mut r, -3.1, -4.1, 5, 0.1);
    stack(dl, &mut r, -2.66, -4.1, 4, -0.05);
    stack(dl, &mut r, -3.1, -3.65, 3, 0.05);
    stack(dl, &mut r, -2.2, -4.12, 2, 0.2);
    stack(dl, &mut r, 3.15, 3.0, 4, 1.57);
    stack(dl, &mut r, 3.15, 3.42, 3, 1.6);
    stack(dl, &mut r, 3.15, 2.58, 5, 1.55);
    stack(dl, &mut r, 2.75, 4.1, 2, 0.3);
    stack(dl, &mut r, -3.15, 0.9, 3, 1.57);
    stack(dl, &mut r, -3.15, 0.45, 2, 1.5);
    crate_c(dl, 0.95, 0.0, 1.5, 0.6, 0x8a1c1c);
    // shelves on the right wall: cocktail cans and cigarette packs
    let shelf = hex(0x5a3c22);
    for y in [0.95f32, 1.45, 1.95] {
        dl.solid(Mesh::Cube, t(w - 0.2, y, -1.3) * s(0.36, 0.04, 2.6), shelf);
    }
    for z in [-2.55f32, -0.05] {
        dl.solid(Mesh::Cube, t(w - 0.2, 1.2, z) * s(0.36, 1.5, 0.04), shelf);
    }
    for i in 0..11 {
        buzz(dl, t(w - 0.22, 0.97, -2.4 + i as f32 * 0.11), i);
        buzz(dl, t(w - 0.22, 1.47, -2.4 + i as f32 * 0.11), i + 3);
    }
    for i in 0..10 {
        pack(dl, t(w - 0.22, 1.97, -2.45 + i as f32 * 0.075) * ry(PI * 0.5), i % 4);
    }
    for i in 0..8 {
        pack(dl, t(w - 0.22, 1.97, -1.5 + i as f32 * 0.075) * ry(PI * 0.5), (i + 1) % 4);
    }
    // the workbench: a pyramid of cans, packs, an ashtray, trophies
    let top = hex(0x6b4a2e);
    dl.solid(Mesh::Cube, t(-1.7, 0.9, -2.6) * s(1.5, 0.06, 0.7), top);
    for (dx, dz) in [(-0.68f32, -0.3f32), (0.68, -0.3), (-0.68, 0.3), (0.68, 0.3)] {
        dl.solid(Mesh::Cube, t(-1.7 + dx, 0.44, -2.6 + dz) * s(0.06, 0.88, 0.06), top);
    }
    let ty = 0.93;
    for (i, (dx, dz)) in [(0.0f32, 0.0f32), (0.11, 0.0), (-0.11, 0.0), (0.055, 0.095), (-0.055, 0.095), (0.0, -0.095)].iter().enumerate() {
        buzz(dl, t(-2.1 + dx, ty, -2.55 + dz), i);
    }
    buzz(dl, t(-2.1, ty + 0.09, -2.53), 4);
    pack(dl, t(-1.55, ty, -2.45) * ry(0.4), 1);
    pack(dl, t(-1.48, ty, -2.62) * ry(-0.3), 2);
    pack(dl, t(-1.3, ty, -2.5) * ry(1.2), 0);
    dl.solid(Mesh::Cyl, t(-1.2, ty + 0.018, -2.75) * s(0.09, 0.035, 0.09), hex(0x888c90));
    for _ in 0..6 {
        let (x, z) = (-1.2 + (r.f() - 0.5) * 0.1, -2.75 + (r.f() - 0.5) * 0.1);
        butt(dl, &mut r, x, ty + 0.04, z);
    }
    let gold = [0.85, 0.6, 0.12, 1.0];
    for (dx, sc) in [(-0.4f32, 0.9f32), (0.0, 1.1), (0.4, 0.8)] {
        let m = t(-1.7 + dx, 0.93, -2.92) * su(sc);
        dl.push(Mesh::Cyl, m * t(0.0, 0.01, 0.0) * s(0.06, 0.02, 0.06), gold, [kind::LIT, 0.25, 1.0, 0.0]);
        dl.push(Mesh::Cyl, m * t(0.0, 0.07, 0.0) * s(0.016, 0.1, 0.016), gold, [kind::LIT, 0.25, 1.0, 0.0]);
        dl.push(Mesh::Cone, m * t(0.0, 0.17, 0.0) * rx(PI) * s(0.065, 0.1, 0.065), gold, [kind::LIT, 0.25, 1.0, 0.0]);
    }
    // a rug, and a bare bulb on a cord
    dl.push(Mesh::Cube, t(0.0, 0.003, 0.0) * s(1.8, 0.006, 1.3), [0.32, 0.05, 0.05, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    dl.push(Mesh::Cube, t(0.0, 0.0075, 0.0) * s(1.62, 0.003, 1.12), [0.06, 0.1, 0.28, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    dl.push(Mesh::Cube, t(0.0, 0.0105, 0.0) * s(1.5, 0.003, 1.0), [0.32, 0.05, 0.05, 1.0], [kind::LIT, 0.0, 0.0, 0.0]);
    for i in 0..6 {
        for j in 0..4 {
            let c = if (i + j) % 2 == 0 { [0.6, 0.25, 0.08, 1.0] } else { [0.06, 0.1, 0.28, 1.0] };
            dl.push(Mesh::Cube, t(-0.55 + i as f32 * 0.22, 0.0135, -0.33 + j as f32 * 0.22) * ry(PI * 0.25) * s(0.09, 0.003, 0.09), c, [kind::LIT, 0.0, 0.0, 0.0]);
        }
    }
    dl.solid(Mesh::Cyl, t(0.15, h - 0.35, 0.55) * s(0.004, 0.7, 0.004), hex(0x111111));
    dl.push(Mesh::Sphere, t(0.15, h - 0.74, 0.55) * su(0.05), [1.0, 0.88, 0.66, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
    e.lights.push((vec3(0.15, h - 0.74, 0.55), [1.0, 0.75, 0.44], 0.3));
    let dl = &mut e.list;
    // the mess right around you: walls of crates behind and beside you, traffic cones on top
    stack(dl, &mut r, -1.35, -1.25, 6, 0.15);
    stack(dl, &mut r, -0.92, -1.4, 5, -0.1);
    stack(dl, &mut r, -1.4, -0.82, 4, 0.3);
    stack(dl, &mut r, -1.6, -1.7, 6, 0.0);
    stack(dl, &mut r, 1.55, -1.5, 5, -0.2);
    stack(dl, &mut r, 1.95, -1.25, 6, 0.1);
    stack(dl, &mut r, 1.5, -1.05, 3, 0.25);
    for (x, y, z, tilt) in [(-1.35f32, 1.8f32, -1.25f32, 0.12f32), (1.95, 1.8, -1.25, -0.08)] {
        let m = t(x, y, z) * rz(tilt);
        dl.solid(Mesh::Cube, m * t(0.0, 0.015, 0.0) * s(0.26, 0.03, 0.26), hex(0xff6a1a));
        dl.solid(Mesh::Cone, m * t(0.0, 0.24, 0.0) * s(0.1, 0.42, 0.1), hex(0xff6a1a));
        dl.solid(Mesh::Cyl, m * t(0.0, 0.23, 0.0) * s(0.066, 0.07, 0.066), hex(0xf2f2f2));
    }
    // a crate table on your left with a cocktail-can pyramid, an overflowing ashtray, packs, a lighter
    crate_c(dl, -0.95, 0.0, 0.15, 0.2, 0x1d5b2f);
    crate_c(dl, -0.95, 0.3, 0.15, 0.25, 0x8a1c1c);
    // the crates are open: a board on top makes it a table
    dl.solid(Mesh::Cube, t(-0.95, 0.606, 0.15) * ry(0.25) * s(0.46, 0.012, 0.36), hex(0x7a5a3a));
    let tt = 0.612;
    for (k, (dx, dz)) in [(0.0f32, 0.0f32), (0.11, 0.0), (-0.11, 0.0), (0.055, 0.09), (-0.055, 0.09), (0.0, -0.09)].iter().enumerate() {
        buzz(dl, t(-1.0 + dx, tt, 0.1 + dz), k);
    }
    buzz(dl, t(-1.0, tt + 0.095, 0.1), 6);
    dl.solid(Mesh::Cyl, t(-0.82, tt + 0.015, 0.25) * s(0.075, 0.03, 0.075), hex(0x7c8084));
    for _ in 0..14 {
        let (x, y, z) = (-0.82 + (r.f() - 0.5) * 0.12, tt + 0.035 + r.f() * 0.02, 0.25 + (r.f() - 0.5) * 0.12);
        butt(dl, &mut r, x, y, z);
    }
    pack(dl, t(-0.85, tt, 0.02) * ry(0.6), 1);
    pack(dl, t(-1.12, tt, 0.28) * ry(-0.4), 2);
    dl.solid(Mesh::Cube, t(-0.75, tt + 0.03, 0.08) * s(0.024, 0.06, 0.012), hex(0x2a8adf));
    // an old couch behind you on the right, a blanket half off it
    let cm = t(0.95, 0.0, -0.95) * ry(-0.35);
    for (x, y, z, sw, sh, sd, c) in [(0.0f32, 0.2f32, 0.0f32, 1.5f32, 0.4f32, 0.75f32, 0x5a4a3a), (0.0, 0.55, -0.3, 1.5, 0.5, 0.18, 0x5a4a3a), (-0.7, 0.42, 0.0, 0.16, 0.3, 0.75, 0x5a4a3a), (0.7, 0.42, 0.0, 0.16, 0.3, 0.75, 0x5a4a3a), (-0.32, 0.45, 0.04, 0.6, 0.1, 0.6, 0x6b5a46), (0.32, 0.43, 0.06, 0.6, 0.1, 0.6, 0x6b5a46)] {
        dl.solid(Mesh::Cube, cm * t(x, y, z) * s(sw, sh, sd), hex(c));
    }
    dl.solid(Mesh::Cube, cm * t(0.35, 0.5, 0.15) * Mat4::from_euler(glam::EulerRot::XYZ, 0.25, 0.3, -0.35) * s(0.7, 0.02, 0.6), hex(0x2a4a7a));
    // beer bottles everywhere, most tipped over; cocktail cans, packs and butts
    let can_cols = [0x2f7a3a, 0xc8ccd0, 0xb8261e, 0x1f3f7a, 0xd8a640];
    for _ in 0..26 {
        let (x, z) = (-1.3 + r.f() * 3.0, -0.6 + r.f() * 2.2);
        if (x * x + z * z).sqrt() < 0.35 {
            continue;
        }
        let col = hex(can_cols[(r.f() * 5.0) as usize % 5]);
        let up = r.f() < 0.25;
        let m = if up { t(x, 0.0, z) } else { t(x, 0.033, z) * ry(r.f() * 6.0) * rz(PI * 0.5) * t(0.0, -0.12, 0.0) };
        bottle(dl, m, col);
    }
    for _ in 0..5 {
        let col = hex(can_cols[(r.f() * 5.0) as usize % 5]);
        let (x, z) = (0.5 + r.f() * 0.8, -0.85 + r.f() * 0.2);
        bottle(dl, t(x, 0.42, z), col);
    }
    for i in 0..12 {
        let (x, z) = (-1.2 + r.f() * 2.8, -0.4 + r.f() * 1.9);
        if (x * x + z * z).sqrt() > 0.4 {
            let m = if r.f() < 0.5 { t(x, 0.052, z) * rz(PI * 0.5) * t(0.0, -0.052, 0.0) } else { t(x, 0.0, z) };
            buzz(dl, m, i);
        }
    }
    for i in 0..8 {
        let (x, z) = (-1.1 + r.f() * 2.5, -0.3 + r.f() * 1.7);
        if (x * x + z * z).sqrt() > 0.35 {
            pack(dl, t(x, 0.012, z) * ry(r.f() * 6.0) * rx(PI * 0.5) * t(0.0, -0.044, 0.0), i % 4);
        }
    }
    for _ in 0..30 {
        let (x, z) = (-1.2 + r.f() * 2.8, -0.5 + r.f() * 2.1);
        if (x * x + z * z).sqrt() > 0.3 {
            butt(dl, &mut r, x, 0.005, z);
        }
    }
    // a greasy pizza box and a crate full of empties in front of you
    dl.solid(Mesh::Cube, t(0.75, 0.02, 0.95) * ry(0.4) * s(0.42, 0.04, 0.42), hex(0xc9a46a));
    dl.solid(Mesh::Cube, t(0.68, 0.16, 0.72) * Mat4::from_euler(glam::EulerRot::YXZ, 0.4, -1.1, 0.0) * s(0.42, 0.01, 0.42), hex(0xb8935a));
    crate_c(dl, -0.55, 0.0, 1.15, -0.3, 0x1d5b2f);
    // fairy lights sagging across the crate walls
    let lc = [0xff4f6a, 0xffd23f, 0x4fd06a, 0x4fa8ff, 0xff8a2a];
    for i in 0..=18 {
        let u = i as f32 / 18.0;
        let (x, y, z) = (-1.5 + u * 3.5, 2.2 - (u * PI).sin() * 0.35, -1.55 + (u * PI * 2.0).sin() * 0.05);
        let c = hex(lc[i % 5]);
        e.list.push(Mesh::Sphere, t(x, y, z) * su(0.02), [c[0] * 2.0, c[1] * 2.0, c[2] * 2.0, 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
        if i % 3 == 0 {
            e.lights.push((vec3(x, y, z), c, 0.18));
        }
    }
    e
}
fn dl_fridge(dl: &mut DrawList, _w: f32) {
    dl.solid(Mesh::Cube, t(-3.15, 0.85, -1.6) * s(0.7, 1.7, 0.65), hex(0xe8e6e0));
    dl.solid(Mesh::Cube, t(-2.79, 1.1, -1.35) * s(0.03, 0.4, 0.04), hex(0x999999));
}
/// The chalkboard on the back wall (shed coordinates): centre, width, height.
const BOARD: (Vec3, f32, f32) = (Vec3::new(-1.75, 1.72, -4.6), 1.4, 0.88);
/// The clubhouse shed sits in front of the training hall, its back wall shared with the hall's front.
pub const HUB_O: Vec3 = Vec3::new(0.0, 0.0, 13.4 + 4.6);
pub const HUB_H: f32 = 3.0;
pub const DOOR_X: f32 = 0.35;
pub const DOOR_W: f32 = 1.6;
pub const DOOR_H: f32 = 2.3;

/// The training records, chalked on the board.
pub fn chalk_records(dl: &mut DrawList, pb: &[Option<f32>; 3]) {
    let (c, w, h) = BOARD;
    let frame = Mat4::from_translation(HUB_O + vec3(c.x - w / 2.0, c.y - h / 2.0, c.z + 0.105));
    let chalk = [0.92, 0.9, 0.84];
    let x0 = 0.1;
    crate::chalk::write(dl, frame, (x0, h - 0.2), 0.11, "Records", chalk);
    dl.push(Mesh::Cube, frame * t(w / 2.0, h - 0.25, 0.0) * s(w - 0.2, 0.006, 0.002), [0.7, 0.68, 0.62, 1.0], [kind::UNLIT, 0.0, 0.0, 1.0]);
    for (i, k) in [game::Drill::Throw, game::Drill::Drink, game::Drill::Run].iter().enumerate() {
        let y = h - 0.42 - i as f32 * 0.16;
        crate::chalk::write(dl, frame, (x0, y), 0.075, k.name(), [0.85, 0.82, 0.74]);
        let v = k.fmt(pb[k.idx()]).replace(' ', "");
        let v = if v == "–" { "-".to_string() } else { v };
        let xr = crate::chalk::right_aligned_x(w - 0.1, 0.075, &v);
        crate::chalk::write(dl, frame, (xr, y), 0.075, &v, [1.0, 0.9, 0.62]);
    }
}

/// Shift a whole environment.
fn moved(mut e: Env, by: Vec3) -> Env {
    let m = Mat4::from_translation(by);
    let shift = |dl: &mut DrawList| {
        for list in [&mut dl.opaque, &mut dl.blend, &mut dl.add] {
            for (_, i) in list.iter_mut() {
                i.m = (m * Mat4::from_cols_array_2d(&i.m)).to_cols_array_2d();
            }
        }
    };
    shift(&mut e.list);
    for w in e.walls.iter_mut() {
        shift(&mut w.list);
        w.d -= w.n.dot(by);
    }
    for l in e.lights.iter_mut() {
        l.0 += by;
    }
    e
}
/// The clubhouse and the training hall behind it: one building.
fn build_clubhouse() -> Env {
    let mut e = build_gym();
    let hub = moved(build_hub(), HUB_O);
    e.list.opaque.extend(hub.list.opaque);
    e.list.blend.extend(hub.list.blend);
    e.list.add.extend(hub.list.add);
    // the shed's back wall is shared with the hall: it is never cut away, or you would see the
    // back of the chalkboard from the hall
    for w in hub.walls {
        if w.n.z > 0.9 {
            e.list.opaque.extend(w.list.opaque);
            e.list.blend.extend(w.list.blend);
            e.list.add.extend(w.list.add);
        } else {
            e.walls.push(w);
        }
    }
    e.lights.extend(hub.lights);
    e
}

pub fn build(k: ArenaKey) -> Env {
    match k {
        ArenaKey::Vondel | ArenaKey::Evening => Env { list: crate::scene::build_env(), walls: Vec::new(), lights: Vec::new() },
        ArenaKey::Krakau => build_krakau(),
        ArenaKey::Roof => build_roof(),
        ArenaKey::Cafe => build_cafe(),
        ArenaKey::Club => build_club(),
        ArenaKey::Gym | ArenaKey::Hub => build_clubhouse(),
    }
}

/// Moving parts: the club's light beams.
/// The ones singing along in Café de Krul's corners.
const SINGERS: [(f32, f32, u32); 8] = [(-5.1, 8.2, 0xd8701a), (-5.75, 7.75, 0x7a3a2a), (5.2, 7.2, 0xd8701a), (4.2, 8.45, 0x2a4a6a), (-5.2, -8.2, 0xd8701a), (-5.75, -7.75, 0x4a5a3a), (5.2, -7.2, 0xd8701a), (1.0, -8.45, 0x6a2a4a)];

pub fn dynamic(dl: &mut DrawList, k: ArenaKey, t_: f32, cam: Vec3) {
    if k == ArenaKey::Evening {
        for p in EVENING_LAMPS {
            dl.add.push((Mesh::Quad, inst(billboard(cam, p, 1.1), [1.0, 0.68, 0.34, 0.75], [kind::SOFT, 0.0, 0.0, 0.0])));
        }
    }
    let beat = t_ * 2.1 * PI; // 126 beats a minute
    if k == ArenaKey::Club {
        let inside_top = cam.y < CLUB.2;
        // the LED wall behind the DJ
        if cam.z > -CLUB.1 {
            for iy in 0..7 {
                for ix in 0..18 {
                    let (u, v) = (ix as f32 - 8.5, iy as f32 - 3.0);
                    let wave = ((u * 0.45 + t_ * 2.3).sin() + (v * 0.8 - t_ * 1.7).sin() + ((u * u + v * v).sqrt() * 0.8 - beat * 0.5).sin()) / 3.0 * 0.5 + 0.5;
                    let c = if wave > 0.62 { [1.0, 0.42, 0.06] } else if wave > 0.38 { [0.95, 0.06, 0.45] } else { [0.08, 0.03, 0.3] };
                    glowbox(dl, t(u * 0.76, 3.1 + v * 0.62, -CLUB.1 + 0.29) * s(0.72, 0.58, 0.02), c, 0.05 + wave * wave * wave * 1.0);
                }
            }
        }
        // the DJ, up on the stage, working the crowd
        let (sh, sz) = STAGE;
        punter(dl, (t_ * 0.8).sin() * 0.3, sz - 1.65, 0.0, sh + ((beat * 0.5).sin() * 0.5 + 0.5) * 0.05, ((beat * 0.125).sin() * 0.5 + 0.5).clamp(0.2, 1.0), hex(0x101014), 0.3);
        if inside_top {
            // party lights everywhere: heads along every truss, each with a thin moving beam
            let pal = [[1.0, 0.45, 0.08], [1.0, 0.1, 0.55], [0.2, 0.5, 1.0], [0.3, 1.0, 0.55], [0.75, 0.2, 1.0]];
            let mut i = 0usize;
            for x in [-7.4f32, -2.5, 2.5, 7.4] {
                for iz in 0..7 {
                    let z = -11.4 + iz as f32 * 3.8;
                    let a = t_ * (0.5 + (i % 5) as f32 * 0.1) + i as f32 * 1.7;
                    let c = pal[(i + (t_ * 0.25) as usize) % 5];
                    let tilt = 0.3 + (a * 1.2).sin() * 0.28;
                    dl.solid(Mesh::Cube, t(x, 5.68, z) * s(0.2, 0.2, 0.24), hex(0x0a0a0e));
                    glowbox(dl, t(x, 5.57, z) * s(0.13, 0.02, 0.13), c, 2.6);
                    // the cone's tip is at +y: flipped so it starts at the lamp and widens on its way down
                    let m = t(x, 5.55, z) * ry(a) * rx(PI - tilt) * t(0.0, 2.7, 0.0) * rx(PI) * s(0.75, 5.4, 0.75);
                    dl.add.push((Mesh::Cone, inst(m, [c[0], c[1], c[2], 0.05], [kind::BEAM, 0.0, 0.0, 0.0])));
                    i += 1;
                }
            }
            // strings of bulbs chasing round the room
            for (x0, z0, x1, z1) in [(-8.1f32, -12.7f32, 8.1f32, -12.7f32), (8.1, -12.7, 8.1, 12.7), (8.1, 12.7, -8.1, 12.7), (-8.1, 12.7, -8.1, -12.7), (-8.1, -3.5, 8.1, 3.5), (-8.1, 3.5, 8.1, -3.5)] {
                for q in 0..30 {
                    let f = q as f32 / 30.0;
                    let on = ((q as f32 * 0.8 - t_ * 5.0).sin() * 0.5 + 0.5).powi(2);
                    let c = pal[q % 5];
                    let p = vec3(x0 + (x1 - x0) * f, 4.9 - (f * PI * 4.0).sin().abs() * 0.5, z0 + (z1 - z0) * f);
                    dl.push(Mesh::Sphere, t(p.x, p.y, p.z) * su(0.04), [c[0], c[1], c[2], 1.0], [kind::UNLIT, 0.0, 0.0, 0.0]);
                    dl.add.push((Mesh::Quad, inst(billboard(cam, p, 0.12 + on * 0.2), [c[0], c[1], c[2], 0.08 + on * 0.34], [kind::SOFT, 0.0, 0.0, 0.0])));
                }
            }
            // sparks off the mirror balls
            for i in 0..26 {
                let a = t_ * 0.5 + i as f32 * 0.25;
                let dir = vec3(a.cos(), -0.1 - (i as f32 * 1.7).sin().abs() * 0.5, a.sin()).normalize();
                let p = if i % 2 == 0 { vec3(0.0, 4.6, 0.0) } else { vec3(4.2, 4.6, -7.0) } + dir * 6.0;
                dl.add.push((Mesh::Quad, inst(billboard(cam, p, 0.07), [1.0, 0.95, 0.85, 0.7], [kind::SOFT, 0.0, 0.0, 0.0])));
            }
            // a strobe on the kick now and then
            if (beat * 0.5).sin() > 0.985 {
                for (x, z) in [(-7.4f32, -11.4f32), (7.4, -11.4), (-7.4, 11.4), (7.4, 11.4)] {
                    dl.add.push((Mesh::Quad, inst(billboard(cam, vec3(x, 5.5, z), 1.6), [1.0, 1.0, 1.0, 0.8], [kind::SOFT, 0.0, 0.0, 0.0])));
                }
            }
        }
        // the crowd, packed in all round behind the barriers, hands in the air
        const SHIRTS: [u32; 10] = [0xe2e2e2, 0x101014, 0xc0225a, 0x2a4a8a, 0xd8a82a, 0x1a8a7a, 0x8a2ac0, 0xd85a1a, 0x4a4a52, 0x7a2020];
        let mut i = 0u32;
        let mut z = -12.6f32;
        while z < 12.5 {
            let mut x = -8.05f32;
            while x < 8.1 {
                i = i.wrapping_add(1);
                let h1 = ((i as f32 * 12.9898).sin() * 43758.547).fract().abs();
                let h2 = ((i as f32 * 78.233).sin() * 12543.11).fract().abs();
                let (px, pz) = (x + (h1 - 0.5) * 0.5, z + (h2 - 0.5) * 0.5);
                x += 0.78;
                // behind the players, and at the sides just past the referee; the corners are where those who are out stand
                let on_pitch = (px.abs() < 5.0 && pz.abs() < 8.05) || (px.abs() < 6.85 && pz.abs() > 5.95 && pz.abs() < 8.05);
                let on_stage = pz < STAGE.1 + 0.25 && px.abs() < 5.9;
                let at_bar = pz > CLUB.1 - 1.35 && px.abs() < 5.5;
                if on_pitch || on_stage || at_bar {
                    continue;
                }
                let hop = ((beat + h1 * 6.0).sin() * 0.5 + 0.5).powi(2) * (0.05 + h2 * 0.1);
                let hype = (0.45 + 0.75 * (beat * 0.25 + h2 * 9.0).sin()).clamp(0.0, 1.0);
                // they face the DJ, more or less
                let yaw = (-px).atan2(STAGE.1 - 1.5 - pz) + (t_ * 1.3 + h1 * 5.0).sin() * 0.4;
                punter(dl, px, pz, yaw, hop, hype, hex(SHIRTS[(h1 * 10.0) as usize % 10]), h2);
            }
            z += 0.78;
        }
    }
    if k == ArenaKey::Cafe {
        // the mirror ball's spots creep over the walls, the candles flicker, the corner sings along
        if cam.y < CAFE.2 {
            for i in 0..12 {
                let a = t_ * 0.4 + i as f32 * 0.52;
                let d = vec3(a.cos(), -0.2 - (i as f32 * 2.3).sin().abs() * 0.3, a.sin()).normalize();
                let p = vec3(0.0, CAFE.2 - 0.5, 0.0) + d * 4.6;
                let c = [[1.0, 0.8, 0.5], [1.0, 0.45, 0.15], [1.0, 1.0, 0.9]][i % 3];
                dl.add.push((Mesh::Quad, inst(billboard(cam, p, 0.1), [c[0], c[1], c[2], 0.6], [kind::SOFT, 0.0, 0.0, 0.0])));
            }
        }
        for i in 0..SINGERS.len() {
            let (x, z, c) = SINGERS[i];
            let seed = (i as f32 * 0.61).fract();
            let sway = (beat * 0.25 + seed * 3.0).sin();
            let hop = ((beat * 0.5 + seed).sin() * 0.5 + 0.5) * 0.03;
            let hype = (0.3 + 0.7 * (beat * 0.125 + seed * 7.0).sin()).clamp(0.0, 1.0);
            punter(dl, x + sway * 0.06, z, (-x).atan2(-z) + sway * 0.2, hop, hype, hex(c), seed);
        }
    }
}
