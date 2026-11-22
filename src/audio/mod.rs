//! Sound. Everything is synthesised (no audio files): `bank` lists the sounds and `sfx`/`amb`
//! make them, once, into sample buffers. They're played at a place in the world (panned and
//! quieter with distance from the camera, plus the room's reverb) by our own mixer on the desktop
//! and Android (`mixer`, `device`) or by Web Audio in the browser (`web`). Each place has its
//! beds (loops: wind in the leaves, a café's chatter, the DJ) and its now-and-then sounds
//! (blackbirds, bike bells, gulls, glasses). The menus' buttons queue theirs with `ui`.

mod amb;
pub mod bank;
pub mod dsp;
mod sfx;
#[cfg(not(target_arch = "wasm32"))]
mod device;
#[cfg(not(target_arch = "wasm32"))]
mod mixer;
#[cfg(target_arch = "wasm32")]
mod web;

pub use bank::Cue;
use bank::{Job, ALL, MAX_VARS};
use crate::arenas::ArenaKey;
use crate::game::{Game, Mode, Sfx, State};
use dsp::{Clip, Rng};
use glam::{vec3, Vec3};
use std::cell::RefCell;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use device::Out;
#[cfg(target_arch = "wasm32")]
use web::Out;

/// Beds playing at once (one fading out while another fades in counts twice).
pub const LOOPS: usize = 6;

/// How to play one sound.
#[derive(Clone, Copy, Debug)]
pub struct Play {
    pub gain: f32,
    /// -1 left … 1 right
    pub pan: f32,
    /// 1 as rendered; 2 an octave up
    pub rate: f32,
    /// how much goes to the room's reverb
    pub wet: f32,
    /// seconds from now
    pub delay: f32,
}
impl Default for Play {
    fn default() -> Self {
        Play { gain: 1.0, pan: 0.0, rate: 1.0, wet: 0.0, delay: 0.0 }
    }
}

thread_local! {
    static UI: RefCell<Vec<Cue>> = const { RefCell::new(Vec::new()) };
}
/// A menu sound, from anywhere in the UI code; played at the end of the frame.
pub fn ui(c: Cue) {
    UI.with(|q| q.borrow_mut().push(c));
}

fn id(c: Cue, var: u32) -> usize {
    c as usize * MAX_VARS + var as usize
}

/// What a place sounds like.
struct Place {
    /// 0 grass, 1 hard, 2 wood
    floor: u8,
    /// reverb time and damping, and how much of the match goes to it
    room: (f32, f32, f32),
    beds: &'static [(Cue, f32)],
    /// (sound, times a minute, gain, how far off, how high)
    spots: &'static [(Cue, f32, f32, f32, f32)],
    /// people all round who cheer along
    crowd: bool,
}
fn place(a: ArenaKey) -> Place {
    use Cue::*;
    match a {
        ArenaKey::Vondel => Place {
            floor: 0,
            room: (0.5, 0.6, 0.06),
            beds: &[(Leaves, 0.32), (City, 0.12)],
            spots: &[(Sparrow, 5.0, 0.3, 14.0, 6.0), (Blackbird, 2.0, 0.35, 20.0, 8.0), (Tit, 1.5, 0.25, 16.0, 7.0), (Pigeon, 1.0, 0.3, 22.0, 9.0), (BikeBell, 0.8, 0.22, 35.0, 1.0)],
            crowd: false,
        },
        ArenaKey::Evening => Place {
            floor: 0,
            room: (0.5, 0.6, 0.06),
            beds: &[(Leaves, 0.22), (City, 0.14), (Crickets, 0.09)],
            spots: &[(Blackbird, 3.0, 0.35, 22.0, 8.0), (Pigeon, 0.5, 0.25, 25.0, 9.0), (BikeBell, 0.8, 0.2, 35.0, 1.0), (Sparrow, 1.0, 0.2, 18.0, 6.0)],
            crowd: false,
        },
        ArenaKey::Roof => Place { floor: 1, room: (0.4, 0.6, 0.05), beds: &[(Wind, 0.35), (City, 0.25)], spots: &[(Gull, 1.5, 0.3, 30.0, 12.0), (Train, 0.3, 0.35, 0.0, 0.0)], crowd: false },
        ArenaKey::Cafe => Place { floor: 2, room: (0.7, 0.5, 0.2), beds: &[(Babble, 0.42), (Room, 0.15)], spots: &[(Clink, 4.0, 0.25, 6.0, 1.0), (Cutlery, 3.0, 0.2, 7.0, 1.0), (Whoo, 0.5, 0.2, 7.0, 1.6)], crowd: true },
        ArenaKey::Club => Place { floor: 2, room: (0.9, 0.4, 0.16), beds: &[(Music, 0.3), (Babble, 0.18)], spots: &[(Whoo, 2.0, 0.3, 9.0, 1.6), (Clink, 1.0, 0.18, 8.0, 1.0)], crowd: true },
        ArenaKey::Krakau => Place { floor: 1, room: (0.8, 0.5, 0.1), beds: &[(Traffic, 0.26), (Crickets, 0.07)], spots: &[(CarPass, 1.2, 0.3, 0.0, 0.0), (BikeBell, 0.3, 0.18, 30.0, 1.0)], crowd: false },
        ArenaKey::Gym => Place { floor: 2, room: (1.8, 0.3, 0.28), beds: &[(Vent, 0.22)], spots: &[(Creak, 0.5, 0.15, 12.0, 4.0)], crowd: false },
        ArenaKey::Hub => Place { floor: 2, room: (0.5, 0.5, 0.16), beds: &[(Fridge, 0.16)], spots: &[(Creak, 1.0, 0.15, 5.0, 2.0)], crowd: false },
    }
}

/// Renders the bank: on a thread of its own natively, a few milliseconds a frame in the browser.
struct Render {
    #[cfg(not(target_arch = "wasm32"))]
    tx: Option<std::sync::mpsc::Sender<(Cue, u32)>>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: Option<std::sync::mpsc::Receiver<(Cue, u32, Clip)>>,
    #[cfg(target_arch = "wasm32")]
    jobs: std::collections::VecDeque<Job>,
}
impl Render {
    fn new() -> Render {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, jobs) = std::sync::mpsc::channel::<(Cue, u32)>();
            let (done, rx) = std::sync::mpsc::channel();
            let ok = std::thread::Builder::new()
                .name("flunki-sounds".into())
                .spawn(move || {
                    while let Ok((c, v)) = jobs.recv() {
                        let mut j = Job::new(c, v);
                        let clip = loop {
                            if let Some(c) = j.step() {
                                break c;
                            }
                        };
                        if done.send((c, v, clip)).is_err() {
                            break;
                        }
                    }
                })
                .is_ok();
            if ok {
                Render { tx: Some(tx), rx: Some(rx) }
            } else {
                Render { tx: None, rx: None }
            }
        }
        #[cfg(target_arch = "wasm32")]
        Render { jobs: Default::default() }
    }
    /// Ask for it; `false` when there's no one to ask (render it here and now).
    fn ask(&mut self, c: Cue, v: u32) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.tx.as_ref().map_or(false, |t| t.send((c, v)).is_ok());
        #[cfg(target_arch = "wasm32")]
        {
            self.jobs.push_back(Job::new(c, v));
            true
        }
    }
    /// Whatever has been finished.
    fn take(&mut self, out: &mut Vec<(Cue, u32, Clip)>) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rx) = self.rx.as_ref() {
            out.extend(rx.try_iter());
        }
        #[cfg(target_arch = "wasm32")]
        {
            // a few milliseconds of the frame, at most
            let t0 = crate::app::now_ms();
            while let Some(j) = self.jobs.front_mut() {
                if let Some(c) = j.step() {
                    out.push((j.cue, j.var, c));
                    self.jobs.pop_front();
                }
                if crate::app::now_ms() - t0 > 3.0 {
                    break;
                }
            }
        }
    }
}

pub struct Audio {
    out: Out,
    render: Render,
    /// per sound and take: 0 not asked for, 1 on its way, 2 there
    have: Vec<u8>,
    last_var: Vec<u32>,
    pub volume: f32,
    rng: Rng,
    ear: Vec3,
    right: Vec3,
    arena: Option<ArenaKey>,
    floor: u8,
    wet: f32,
    crowd: bool,
    beds: [(Option<Cue>, f32); LOOPS],
    clap_t: f32,
    run_t: f32,
    clap_k: usize,
    ui_t: f32,
}

impl Audio {
    pub fn new() -> Self {
        let mut a = Audio {
            out: Out::new(),
            render: Render::new(),
            have: vec![0; ALL.len() * MAX_VARS],
            last_var: vec![u32::MAX; ALL.len()],
            volume: 0.8,
            rng: Rng::new(crate::app::now_ms() as u32),
            ear: vec3(0.0, 6.0, 14.0),
            right: Vec3::X,
            arena: None,
            floor: 0,
            wet: 0.1,
            crowd: false,
            beds: [(None, 0.0); LOOPS],
            clap_t: 0.0,
            run_t: 0.0,
            clap_k: 0,
            ui_t: 0.0,
        };
        // the menus first, then the match, then the places
        let mut order: Vec<Cue> = ALL.iter().copied().filter(|c| c.is_ui()).collect();
        order.extend(ALL.iter().copied().filter(|c| !c.is_ui() && !c.is_loop()));
        order.extend(ALL.iter().copied().filter(|c| c.is_loop()));
        for c in order {
            a.ask(c);
        }
        a
    }
    fn ask(&mut self, c: Cue) {
        for v in 0..c.vars() {
            let i = id(c, v);
            if self.have[i] == 0 {
                self.have[i] = 1;
                if !self.render.ask(c, v) {
                    self.have[i] = 0;
                }
            }
        }
    }
    /// Call from a user gesture so browsers let the sound start.
    pub fn unlock(&mut self) {
        self.out.unlock();
    }
    /// A take of `c` that's ready (rendered here and now if it's short and hasn't come yet).
    fn take_of(&mut self, c: Cue) -> Option<usize> {
        let n = c.vars();
        let mut v = self.rng.i(n as usize) as u32;
        if n > 1 && v == self.last_var[c as usize] {
            v = (v + 1 + self.rng.i(n as usize - 1) as u32) % n;
        }
        let i = id(c, v);
        if self.have[i] != 2 {
            if c.steps() > 1 || c.is_loop() {
                self.ask(c);
                // any other take that's there will do
                let any = (0..n).find(|&w| self.have[id(c, w)] == 2)?;
                return Some(id(c, any));
            }
            let mut j = Job::new(c, v);
            let clip = j.step()?;
            self.out.add(i, &Arc::new(clip));
            self.have[i] = 2;
        }
        self.last_var[c as usize] = v;
        Some(i)
    }
    /// Play `c` as is.
    pub fn play(&mut self, c: Cue, p: Play) {
        if self.volume <= 0.001 {
            return;
        }
        if let Some(i) = self.take_of(c) {
            self.out.play(i, p);
        }
    }
    /// Play `c` at a place in the world; `near`: up to this far it's at full volume.
    pub fn play_at(&mut self, c: Cue, at: Vec3, gain: f32, near: f32) {
        let d = at - self.ear;
        let dist = d.length();
        let fall = (near / dist.max(near)).powf(0.85);
        let pan = if dist > 0.01 { (d / dist).dot(self.right) * 0.8 } else { 0.0 };
        // further off, more of it is the room
        let wet = self.wet * (0.7 + 0.8 * (dist / 25.0).min(1.0));
        let rate = 1.0 + self.rng.n() * 0.035;
        let g = gain * fall * self.rng.r(0.88, 1.0);
        self.play(c, Play { gain: g, pan, rate, wet, delay: 0.0 });
    }

    fn team_at(g: &Game, t: usize) -> Vec3 {
        let p = &g.teams[t.min(1)].players;
        (p[0].pos + p[1].pos + p[2].pos) / 3.0 + vec3(0.0, 1.5, 0.0)
    }
    /// A sound the match made.
    pub fn game(&mut self, s: Sfx, g: &Game) {
        use Cue::*;
        let floor = self.floor;
        let ball = g.ball;
        let refp = g.referee.pos + vec3(0.0, 1.6, 0.0);
        let drill = g.train.is_some();
        match s {
            Sfx::Throw => {
                let at = g.thrown_by.map(|(t, i)| g.teams[t].players[i].pos + vec3(0.0, 1.4, 0.0)).unwrap_or(ball);
                let k = (g.ball_vel.length() / 9.0).clamp(0.3, 1.3);
                self.play_at(Throw, at, 0.45 + 0.35 * k, 5.0);
            }
            Sfx::Bounce(i) => {
                let c = [BounceGrass, BounceHard, BounceWood][floor as usize];
                self.play_at(c, ball, i.clamp(0.06, 1.0).powf(0.7) * 0.9, 5.0);
            }
            Sfx::Touch(i) => self.play_at(BallBottle, ball, i.clamp(0.3, 1.0), 6.0),
            Sfx::Bottle(i) if (i - 0.3).abs() < 1e-4 => self.play_at(BottleSet, refp - vec3(0.0, 1.5, 0.0), if floor == 0 { 0.5 } else { 0.8 }, 5.0),
            Sfx::Bottle(i) => self.play_at(if floor == 0 { BottleGrass } else { BottleHard }, vec3(0.0, 0.2, 0.0), i.clamp(0.35, 1.0), 6.0),
            Sfx::CanHit => self.play_at(CanKnock, ball, 0.9, 6.0),
            Sfx::Whistle => self.play_at(WhistleTwo, refp, 0.75, 10.0),
            Sfx::Stop => self.play_at(WhistleLong, refp, 0.75, 10.0),
            Sfx::Straf => {
                self.play_at(WhistleFoul, refp, 0.8, 10.0);
                if !drill {
                    let t = self.rng.i(2);
                    let at = Self::team_at(g, t);
                    self.delayed(Ooh, at, 0.5, 8.0, 0.35);
                }
            }
            Sfx::Glug | Sfx::Cough => {
                let t = g.hitter.or(g.my_team).unwrap_or(0);
                let c = if matches!(s, Sfx::Glug) { Gulp } else { Cough };
                self.play_at(c, Self::team_at(g, t), if c == Gulp { 0.6 } else { 0.7 }, 7.0);
            }
            Sfx::Step => {
                let at = g.run.as_ref().map(|r| g.teams[r.runners].players[r.ri].pos).unwrap_or(ball);
                self.play_at([StepGrass, StepHard, StepWood][floor as usize], at, 0.55, 6.0);
            }
            Sfx::Hit => {
                self.play(HitSting, Play { gain: 0.35, ..Default::default() });
                if !drill {
                    let t = g.hitter.unwrap_or(g.turn);
                    self.play_at(Cheer, Self::team_at(g, t), 0.8, 8.0);
                    if self.crowd {
                        self.play(Crowd, Play { gain: 0.25, wet: self.wet, ..Default::default() });
                    }
                }
            }
            Sfx::Miss => {
                if drill {
                    self.play(MissSting, Play { gain: 0.3, ..Default::default() });
                } else {
                    let t = g.thrown_by.map(|t| t.0).unwrap_or(g.turn);
                    self.play_at(Aww, Self::team_at(g, t), 0.6, 8.0);
                }
            }
            Sfx::Win => {
                let won = g.my_team.map_or(true, |t| g.win == Some(t)) || g.mode == Mode::Demo;
                self.play(if won { WinSting } else { LoseSting }, Play { gain: 0.5, ..Default::default() });
                let at = Self::team_at(g, g.win.unwrap_or(0));
                self.delayed(Crowd, at, if self.crowd { 0.9 } else { 0.6 }, 12.0, 0.2);
            }
            Sfx::Spin => self.play(Spin, Play { gain: 0.35, ..Default::default() }),
        }
    }
    fn delayed(&mut self, c: Cue, at: Vec3, gain: f32, near: f32, delay: f32) {
        let d = at - self.ear;
        let dist = d.length().max(0.01);
        let fall = (near / dist.max(near)).powf(0.85);
        let wet = self.wet * (0.7 + 0.8 * (dist / 25.0).min(1.0));
        self.play(c, Play { gain: gain * fall, pan: (d / dist).dot(self.right) * 0.8, rate: 1.0, wet, delay });
    }
    /// Someone does an emote (`i` in `anim::EMOTES`), `delay` seconds from now.
    pub fn emote(&mut self, i: usize, at: Vec3, delay: f32) {
        let (c, d, g) = match i {
            0 => (Cue::Lighter, 0.5, 0.5),
            1 => (Cue::CanOpen, 0.3, 0.5),
            5 => (Cue::Slap, 0.45, 0.6),
            7 => (Cue::Claps, 0.2, 0.45),
            _ => return,
        };
        self.delayed(c, at + vec3(0.0, 1.4, 0.0), g, 6.0, delay + d);
    }

    /// Once a frame: where the camera is, which place is on screen and how loud it should be
    /// (`level`: 1 in a match, lower in the menus), and the match for the clapping.
    pub fn tick(&mut self, dt: f32, cam: (Vec3, Vec3), arena: ArenaKey, level: f32, g: &Game) {
        let mut done = Vec::new();
        self.render.take(&mut done);
        for (c, v, clip) in done {
            let i = id(c, v);
            self.out.add(i, &Arc::new(clip));
            self.have[i] = 2;
        }
        self.ear = cam.0;
        let fwd = (cam.1 - cam.0).normalize_or(Vec3::NEG_Z);
        self.right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        let pl = place(arena);
        if self.arena != Some(arena) {
            self.arena = Some(arena);
            self.out.room(pl.room.0, pl.room.1);
        }
        self.floor = pl.floor;
        self.wet = pl.room.2;
        self.crowd = pl.crowd;
        self.out.frame(self.volume);
        if !self.out.ready() {
            return;
        }
        // the beds: what this place wants, fading what it doesn't
        for s in self.beds.iter_mut() {
            if let Some(c) = s.0 {
                if !pl.beds.iter().any(|b| b.0 == c) {
                    s.1 += dt;
                }
            }
        }
        for slot in 0..LOOPS {
            let (c, fade) = self.beds[slot];
            match c {
                Some(_) if fade > 2.5 => {
                    self.beds[slot] = (None, 0.0);
                    self.out.bed(slot, None, Play::default());
                }
                Some(c) if fade > 0.0 => self.out.bed(slot, Some(id(c, 0)), Play { gain: 0.0, wet: self.wet, ..Default::default() }),
                _ => {}
            }
        }
        for &(c, gain) in pl.beds {
            if self.have[id(c, 0)] != 2 {
                self.ask(c);
                continue;
            }
            let slot = match self.beds.iter().position(|s| s.0 == Some(c)) {
                Some(s) => s,
                None => match self.beds.iter().position(|s| s.0.is_none()) {
                    Some(s) => s,
                    None => continue,
                },
            };
            self.beds[slot] = (Some(c), 0.0);
            self.out.bed(slot, Some(id(c, 0)), Play { gain: gain * level, wet: self.wet * 0.5, ..Default::default() });
        }
        // now and then, somewhere round about
        for &(c, per_min, gain, far, high) in pl.spots {
            if self.rng.u() < dt * per_min / 60.0 {
                if far == 0.0 {
                    // carries its own movement
                    self.play(c, Play { gain: gain * level, wet: self.wet, ..Default::default() });
                } else {
                    let a = self.rng.r(0.0, std::f32::consts::TAU);
                    let d = far * self.rng.r(0.7, 1.4);
                    let at = vec3(self.ear.x + a.cos() * d, high * self.rng.r(0.7, 1.3), self.ear.z + a.sin() * d);
                    self.play_at(c, at, gain * level, far);
                }
            }
        }
        // the runner's team claps them home, faster and faster
        if g.state == State::Run && g.mode != Mode::Demo && level > 0.5 {
            if let Some(r) = g.run.as_ref() {
                self.run_t += dt;
                self.clap_t -= dt;
                if self.clap_t <= 0.0 {
                    self.clap_t = (0.5 - self.run_t * 0.025).max(0.27);
                    let mates: Vec<usize> = (0..3).filter(|&i| i != r.ri && Some(i) != r.ri2).collect();
                    if let Some(&i) = mates.get(self.clap_k % mates.len().max(1)) {
                        let at = g.teams[r.runners].players[i].pos + vec3(0.0, 1.3, 0.0);
                        self.play_at(Cue::Clap, at, 0.35, 6.0);
                    }
                    if self.crowd {
                        let (pan, rate, delay) = (self.rng.n() * 0.8, self.rng.r(0.9, 1.1), self.rng.r(0.0, 0.03));
                        self.play(Cue::Clap, Play { gain: 0.15, pan, rate, wet: self.wet, delay });
                    }
                    self.clap_k += 1;
                }
            }
        } else {
            self.run_t = 0.0;
            self.clap_t = 0.0;
        }
    }
    /// After the UI is drawn: the menu sounds it asked for, or a plain tap when something was
    /// clicked that didn't ask for one.
    pub fn ui_frame(&mut self, dt: f32, clicked: bool) {
        self.ui_t -= dt;
        let mut q = UI.with(|q| std::mem::take(&mut *q.borrow_mut()));
        if q.is_empty() && clicked {
            q.push(Cue::Tap);
        }
        for c in q {
            if self.ui_t > 0.0 && matches!(c, Cue::Tap | Cue::Tick | Cue::TapBig) {
                continue;
            }
            self.ui_t = 0.04;
            let gain = match c {
                Cue::Chime | Cue::LevelUp => 0.5,
                Cue::SwishUp | Cue::SwishDown => 0.22,
                _ => 0.4,
            };
            let rate = 1.0 + self.rng.n() * 0.03;
            self.play(c, Play { gain, rate, ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bank::*;
    use super::dsp::*;

    fn render(c: Cue, v: u32) -> Clip {
        let mut j = Job::new(c, v);
        loop {
            if let Some(c) = j.step() {
                return c;
            }
        }
    }
    #[test]
    fn the_list_is_in_order() {
        for (i, c) in ALL.iter().enumerate() {
            assert_eq!(*c as usize, i, "{c:?}");
            assert!(c.vars() as usize <= MAX_VARS);
        }
    }
    #[test]
    fn every_sound_is_audible_and_clean() {
        for c in ALL {
            for v in 0..c.vars() {
                let t = std::time::Instant::now();
                let a = render(c, v);
                let ms = t.elapsed().as_secs_f64() * 1000.0 / c.steps() as f64;
                let peak = a.l.iter().chain(a.r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
                assert!(a.l.iter().chain(a.r.iter()).all(|x| x.is_finite()), "{c:?} not finite");
                assert!(peak > 0.3 && peak <= 1.0, "{c:?}/{v} peak {peak}");
                assert!(a.len() > 100 && a.len() < (RATE * 14.0) as usize, "{c:?} is {} samples", a.len());
                if a.is_stereo() {
                    assert_eq!(a.l.len(), a.r.len());
                }
                // a step of rendering must fit in a frame (release builds are ~10x faster)
                assert!(ms < 400.0, "{c:?} took {ms:.0} ms a step");
            }
        }
    }
    #[test]
    fn loops_join_up() {
        for c in ALL.iter().filter(|c| c.is_loop()) {
            let a = render(*c, 0);
            let n = a.len();
            // the jump from the end back to the start is no bigger than the steps inside
            let inside = a.l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            let seam = (a.l[0] - a.l[n - 1]).abs();
            assert!(seam <= inside * 1.5 + 1e-3, "{c:?} seam {seam} inside {inside}");
        }
    }
    #[test]
    fn a_whistle_has_its_pitch() {
        // a pea whistle sits near 3 kHz
        let a = render(Cue::WhistleLong, 0);
        let z = a.l[4410..8820].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((250..=340).contains(&z), "{z} crossings in 0.1 s");
    }
    /// `FLUNKI_WAV=dir cargo test --release --lib dump_wavs -- --ignored`: every sound as a .wav, to listen to.
    #[test]
    #[ignore]
    fn dump_wavs() {
        let dir = std::env::var("FLUNKI_WAV").unwrap_or_else(|_| "/tmp/flunki-wav".into());
        std::fs::create_dir_all(&dir).unwrap();
        for c in ALL {
            for v in 0..c.vars().min(2) {
                let a = render(c, v);
                let ch: u16 = if a.is_stereo() { 2 } else { 1 };
                let mut d = Vec::new();
                for k in 0..a.len() {
                    d.extend_from_slice(&((a.l[k] * 32767.0) as i16).to_le_bytes());
                    if ch == 2 {
                        d.extend_from_slice(&((a.r[k] * 32767.0) as i16).to_le_bytes());
                    }
                }
                let mut w = Vec::new();
                let rate = RATE as u32;
                w.extend_from_slice(b"RIFF");
                w.extend_from_slice(&(36 + d.len() as u32).to_le_bytes());
                w.extend_from_slice(b"WAVEfmt ");
                w.extend_from_slice(&16u32.to_le_bytes());
                w.extend_from_slice(&1u16.to_le_bytes());
                w.extend_from_slice(&ch.to_le_bytes());
                w.extend_from_slice(&rate.to_le_bytes());
                w.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
                w.extend_from_slice(&(ch * 2).to_le_bytes());
                w.extend_from_slice(&16u16.to_le_bytes());
                w.extend_from_slice(b"data");
                w.extend_from_slice(&(d.len() as u32).to_le_bytes());
                w.extend_from_slice(&d);
                std::fs::write(format!("{dir}/{c:?}-{v}.wav"), w).unwrap();
            }
        }
    }
}
