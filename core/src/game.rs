//! The match itself: rules, physics, the referee and the computer players.
//! A straight port of the JavaScript game's simulation, kept free of rendering and UI.

use glam::{vec3, Vec3};

pub const R: f32 = 0.047; // bottle radius
pub const H: f32 = 0.32; // bottle height
pub const BR: f32 = 0.105; // ball radius
pub const LINE: f32 = 6.0; // throwing lines at z = ±LINE
pub const G: f32 = 9.81;
pub const STEP: f32 = 1.0 / 240.0;
pub const MAX_STRAF: u32 = 3;
pub const HC: f32 = 0.085; // bottle centre of mass height (1.5 L PET with ~0.5 L water)
pub const KD: f32 = 0.036; // volleyball air drag
pub const CURVE_A: f32 = 2.0;
pub const REF_POS: Vec3 = vec3(3.4, 0.0, 0.0);
pub const GAZE_HALF: f32 = 0.5;
pub const SNEAK_DUR: f32 = 3.6;
pub const SNEAK_AMT: f32 = 0.15;
pub const SNEAK_LEFT: f32 = 0.03;

fn kb() -> f32 {
    G / (1.33 * (R * R + HC * HC))
}
fn crit() -> f32 {
    (R / HC).atan()
}
pub fn ref_face() -> f32 {
    (-REF_POS.x).atan2(-REF_POS.z)
}
pub fn wrap_a(mut d: f32) -> f32 {
    use std::f32::consts::PI;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d < -PI {
        d += 2.0 * PI;
    }
    d
}
fn ease(k: f32) -> f32 {
    k * k * (3.0 - 2.0 * k)
}

#[derive(Clone, Copy)]
pub struct Diff {
    pub aim_sd: f32,
    pub dist_sd: f32,
    pub run: f32,
    pub drink: f32,
    pub straf: f32,
}
pub const DIFFS: [Diff; 3] = [
    Diff { aim_sd: 0.065, dist_sd: 1.0, run: 3.5, drink: 0.036, straf: 0.12 },
    Diff { aim_sd: 0.045, dist_sd: 0.7, run: 4.2, drink: 0.048, straf: 0.07 },
    Diff { aim_sd: 0.03, dist_sd: 0.5, run: 4.9, drink: 0.06, straf: 0.03 },
];
pub const DIFF_NAMES: [&str; 3] = ["Sloppy", "Regular", "Champions"];
pub const RING_L: [&str; 4] = ["", "Nice", "Great", "Excellent"];
pub const RING_M: [f32; 4] = [1.0, 1.1, 1.25, 1.5];
pub fn ring_idx(r: f32) -> u8 {
    if r < 0.45 {
        3
    } else if r < 0.7 {
        2
    } else {
        1
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Foul {
    Sneak,
    Spill,
    Stop,
    False,
    Auto,
    Can,
}
impl Foul {
    pub fn text(self) -> &'static str {
        match self {
            Foul::Sneak => "Illegal sip",
            Foul::Spill => "Spilled beer",
            Foul::Stop => "Drank after STOP",
            Foul::False => "False start",
            Foul::Auto => "Autoclicker",
            Foul::Can => "Bottle knocked over",
        }
    }
}

/// Small deterministic RNG (no platform entropy needed on the web).
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    pub fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
    pub fn gauss(&mut self) -> f32 {
        let mut u = 0.0;
        while u == 0.0 {
            u = self.f();
        }
        let v = self.f();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f32::consts::PI * v).cos()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AMode {
    Idle,
    Run,
    Bend,
    Cheer,
    Sit,
}

#[derive(Clone)]
pub struct Anim {
    pub mode: AMode,
    pub speed: f32,
    pub phase: f32,
    pub throw_t: f32,
    /// an emote playing: its number in `anim::EMOTES` plus one (0: none). Only ever on the screens.
    pub emote: u8,
    pub emote_t: f32,
    pub sip: bool,
    /// sitting, reaching down into the crate in front of the seat
    pub reach: bool,
    /// has their can in hand (picked up off the ground, not necessarily drinking this moment)
    pub carry: bool,
    pub hold: bool,
    pub point: bool,
    pub smoking: bool,
    pub smoke_t: f32,
    pub look: f32,
    pub cheer_t: f32,
}
impl Default for Anim {
    fn default() -> Self {
        Anim { mode: AMode::Idle, speed: 0.0, phase: 0.0, throw_t: -1.0, emote: 0, emote_t: 0.0, sip: false, reach: false, carry: false, hold: false, point: false, smoking: false, smoke_t: 0.0, look: 0.0, cheer_t: 0.0 }
    }
}
impl Anim {
    /// The referee takes a drag for the first second of each smoking cycle…
    pub fn drag(&self) -> bool {
        self.smoking && self.smoke_t % 4.2 < 1.0
    }
    /// …then blows the smoke out.
    pub fn exhale(&self) -> bool {
        self.smoking && (1.3..2.7).contains(&(self.smoke_t % 4.2))
    }
}

#[derive(Clone)]
pub struct Person {
    pub pos: Vec3,
    pub yaw: f32,
    pub anim: Anim,
    pub visible: bool,
    pub net_target: Option<Vec3>,
    pub net_from: Vec3,
    pub net_vel: Vec3,
    /// current walking/running speed (people speed up and slow down, they don't teleport to full speed)
    pub vel: f32,
    pub vis: crate::anim::Vis,
}
impl Person {
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Person { pos, yaw, anim: Anim::default(), visible: true, net_target: None, net_from: pos, net_vel: Vec3::ZERO, vel: 0.0, vis: crate::anim::Vis::default() }
    }
    /// Roughly where the right hand is; used for launching and carrying the ball.
    pub fn hand(&self) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        // where the right hand is at the moment of release in the underarm throw
        let off = vec3(-0.25, 0.97, 0.68);
        self.pos + vec3(off.x * c + off.z * s, off.y, -off.x * s + off.z * c)
    }
}

#[derive(Clone, Default)]
pub struct DrinkCtl {
    pub hold: bool,
    pub ht: f32,
    pub p: f32,
    pub cough: f32,
    pub mul: f32,
    pub stop_watch: f32,
    pub stop_show: f32,
    pub late: bool,
    glug: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Foot {
    L,
    R,
}

#[derive(Clone, Default)]
pub struct RunCtl {
    pub last: Option<Foot>,
    pub lock: f32,
    times: Vec<f64>,
    false_for: i64,
    false_t: f32,
    pub caught_for: i64,
    pub msg: String,
    pub msg_t: f32,
    pub down: [f32; 2],
}

#[derive(Clone)]
pub struct Team {
    pub name: &'static str,
    pub side: f32,
    pub color: [f32; 3],
    pub players: [Person; 3],
    pub homes: [Vec3; 3],
    pub beers: [f32; 3],
    pub mult: [f32; 3],
    pub out: [bool; 3],
    pub idx: usize,
    pub hits: u32,
    pub throws: u32,
    pub straf: u32,
    pub drinking: bool,
    pub late_ok: bool,
    pub early: f32,
    false_straf_for: i64,
    unseen_for: i64,
    pub dc: DrinkCtl,
    pub rc: RunCtl,
}
impl Team {
    fn new(name: &'static str, side: f32, color: [f32; 3]) -> Self {
        let homes = [-1.5f32, 0.0, 1.5].map(|x| vec3(x, 0.0, side * (LINE + 0.45)));
        let face = if side > 0.0 { std::f32::consts::PI } else { 0.0 };
        Team {
            name,
            side,
            color,
            players: homes.map(|h| Person::new(h, face)),
            homes,
            beers: [1.0; 3],
            mult: [1.0; 3],
            out: [false; 3],
            idx: 0,
            hits: 0,
            throws: 0,
            straf: 0,
            drinking: false,
            late_ok: false,
            early: 0.0,
            false_straf_for: -1,
            unseen_for: -1,
            dc: DrinkCtl::default(),
            rc: RunCtl::default(),
        }
    }
    pub fn face(&self) -> f32 {
        if self.side > 0.0 {
            std::f32::consts::PI
        } else {
            0.0
        }
    }
    /// The next player at or after `from` who still has beer.
    pub fn active_idx(&self, from: usize) -> usize {
        for k in 0..3 {
            let i = (from + k) % 3;
            if self.beers[i] > 0.0 {
                return i;
            }
        }
        from
    }
    pub fn runner_idx(&self) -> usize {
        self.active_idx(self.idx)
    }
    /// The second runner stands the bottle up; with one player left there is none.
    pub fn runner2_idx(&self) -> Option<usize> {
        let a = self.runner_idx();
        let b = self.active_idx((a + 1) % 3);
        (b != a).then_some(b)
    }
    pub fn can_pos(&self, i: usize, k: usize) -> [f32; 2] {
        let hm = self.homes[i];
        [hm.x - 0.07 + k as f32 * 0.085, hm.z - self.side * 0.42]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Cpu,
    Hot,
    Demo,
    Online,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Menu,
    Aim,
    CpuAim,
    Flight,
    Run,
    Stopped,
    Over,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctrl {
    Local,
    Cpu,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Ball,
    Pick,
    Bottle,
    Fix,
    Home,
    Done,
}

pub struct RunState {
    pub runners: usize,
    pub ri: usize,
    pub phase: Phase,
    pub boost: f32,
    pub mult: f32,
    t: f32,
    pt: f32,
    ft: f32,
    th0: f32,
    bx: f32,
    bz: f32,
    pub ri2: Option<usize>,
    pub phase2: Phase,
    ft2: f32,
    th2: f32,
    bx2: f32,
    bz2: f32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Sfx {
    Throw,
    Bounce(f32),
    Touch(f32),
    Bottle(f32),
    CanHit,
    Whistle,
    Stop,
    Glug,
    Cough,
    Step,
    Hit,
    Straf,
    Spin,
    Miss,
    Win,
}

#[derive(Clone, Debug)]
pub struct Banner {
    pub title: String,
    pub sub: String,
    pub dur: f32,
    pub straf: bool,
}

pub struct ThrowSpec {
    pub origin: Vec3,
    pub yaw: f32,
    pub dist: f32,
    pub curve: f32,
    pub ring: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drill {
    Throw,
    Drink,
    Run,
}
impl Drill {
    pub fn name(self) -> &'static str {
        match self {
            Drill::Throw => "Throwing",
            Drill::Drink => "Drinking",
            Drill::Run => "Running",
        }
    }
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn fmt(self, v: Option<f32>) -> String {
        match v {
            None => "–".into(),
            Some(v) if self == Drill::Throw => format!("{} / 10", v as u32),
            Some(v) => format!("{v:.1} s"),
        }
    }
}
pub const DRILL_THROWS: u32 = 10;
pub const DRILL_RUNS: usize = 3;

/// A solo drill on the pitch. Fouls cost 2 seconds instead of a Strafbier.
pub struct Train {
    pub kind: Drill,
    pub n: u32,
    pub hits: u32,
    pub rings: u32,
    pub t: f32,
    rt0: f32,
    pub pen: f32,
    clock: bool,
    pub runs: Vec<f32>,
    pub done: bool,
}

/// Steps of the guided practice round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tut {
    Intro,
    Throw,
    Fly,
    Retry,
    Drink,
    Wait,
    Run,
    Ref,
    Done,
}
impl Tut {
    pub fn hold(self) -> bool {
        matches!(self, Tut::Intro | Tut::Ref | Tut::Done)
    }
    pub fn step(self) -> u32 {
        match self {
            Tut::Intro => 1,
            Tut::Throw | Tut::Fly | Tut::Retry => 2,
            Tut::Drink => 3,
            Tut::Wait | Tut::Run => 4,
            Tut::Ref | Tut::Done => 5,
        }
    }
    pub fn text(self) -> (&'static str, &'static str) {
        match self {
            Tut::Intro => ("Welcome to the pitch", "Two teams of three, a half-full water bottle in the middle. Knock it over and your team drinks, while the other team races to put it back up."),
            Tut::Throw => ("Throw", "Put your thumb on the ball and flick it up toward the bottle. A faster flick goes further. Let go while the coloured ring is small for a bonus."),
            Tut::Fly => ("Watch the bottle…", "It only counts if it actually falls over."),
            Tut::Retry => ("So close", "A soft hit only makes it wobble. Flick again, a bit harder or straighter."),
            Tut::Drink => ("Drink!", "Hold the DRINK button. The ring around it is your breath: let go before it fills, or you choke and spill. When you hear STOP, let go straight away."),
            Tut::Wait => ("Their throw", "Keep your thumbs on L and R, but don't tap before the bottle falls. That's a false start."),
            Tut::Run => ("Run!", "One of your players fetches the ball, another stands the bottle back up. Tap L and R in turn to make them sprint, and get both home. The same foot twice makes you stumble."),
            Tut::Ref => ("Mind the referee", "The light on the lawn shows where he's looking. Right now that's at you. Spill, false start or drink after STOP while he watches, and you get a Strafbier: one extra bottle. Players who empty their bottle leave the pitch; the first team with no one left wins."),
            Tut::Done => ("You're ready", "Fouls didn't count in this practice round. In a real match they do."),
        }
    }
}

/// Things worth telling the players. Each screen words them from its own side (see `word`).
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Ev {
    Turn(usize, usize),
    Hit(usize, u8, bool),
    Miss(usize),
    Stop,
    Out(usize, usize),
    Over(usize),
    Straf(usize, Foul),
    Unseen(usize, Foul),
    SneakOk(usize),
}

/// What happened, for the stats page.
#[derive(Clone, Copy, Debug)]
pub enum StatEv {
    Throw { team: usize, idx: usize },
    Hit { team: usize, ring: u8, curve: bool },
    Stop,
    Straf { team: usize },
    Unseen { team: usize },
    Sneak { team: usize },
    Over { winner: usize },
}

#[derive(Clone, Copy)]
enum Act {
    CpuWind(usize),
    CpuLaunch(usize),
    StartTurn(usize),
    ShowOver(usize),
    CpuFalse(usize),
    CpuLate(usize),
    DemoRestart(usize),
    DrinkGo,
    TrainNext,
    TrainRunNext,
    TrainFinish,
}

struct Sneak {
    t: usize,
    idxs: Vec<usize>,
    start: f32,
    mine: bool,
    human: bool,
}

pub struct Smoke {
    pub pos: Vec3,
    pub vel: Vec3,
    pub life: f32,
    pub max: f32,
    pub s0: f32,
    pub s1: f32,
    pub o: f32,
}

pub struct Splash {
    pub pos: Vec3,
    pub vel: Vec3,
    pub life: f32,
}

pub struct Game {
    pub mode: Mode,
    pub diff: usize,
    pub state: State,
    pub teams: [Team; 2],
    pub referee: Person,
    pub turn: usize,
    pub hitter: Option<usize>,
    last_hitter: Option<usize>,
    prev_state: State,
    pub run: Option<RunState>,
    pub win: Option<usize>,
    // ball
    pub ball: Vec3,
    pub ball_vel: Vec3,
    pub ball_rot: glam::Quat,
    pub ball_curve: f32,
    pub ball_free: bool,
    pub holder: Option<(usize, usize)>, // (team, player)
    // bottle
    pub theta: f32,
    omega: f32,
    pub tip_dir: Vec3,
    pub bottle_up: bool,
    bottle_falling: bool,
    toppled: bool,
    // cans knocked over: (team, player, can) → direction
    pub knocked: Vec<((usize, usize, usize), [f32; 2])>,
    // referee
    ref_target: i8, // -1 centre, 0 team A, 1 team B, 2 smoke
    ref_until: f32,
    ref_walk: u8, // 0 seated, 4 take a bottle from the crate, 1 go, 2 put it down and point, 3 back
    ref_walk_t: usize,
    ref_walk_i: usize,
    ref_walk_tm: f32,
    /// the Strafbier on its way: whose it is. It isn't at their feet until he has put it there.
    pub ref_bottle: Option<(usize, usize)>,
    /// he has it in his hand
    pub ref_hold: bool,
    pub gaze_col: [f32; 3],
    pub gaze_op: f32,
    pub smoke: Vec<Smoke>,
    smoke_next: f32,
    smoke_wisp: f32,
    pub splash: Vec<Splash>,
    // bookkeeping
    pub game_time: f32,
    pub clock: f32,
    phys_acc: f32,
    flight_t: f32,
    flight_no: i64,
    ln_i: i64,
    throw_ring: u8,
    throw_curve: f32,
    timers: Vec<(f32, u64, Act)>,
    gen: u64,
    sneaks: Vec<Sneak>,
    pub my_sneak_t: f32,
    pub rng: Rng,
    pub sfx: Vec<Sfx>,
    pub banners: Vec<Banner>,
    pub trail: Vec<Vec3>,
    pub show_over: bool,
    pub bounds: Option<(f32, f32)>,
    pub train: Option<Train>,
    pub train_result: Option<(Drill, f32, String)>,
    pub tut: Option<Tut>,
    pub stat_ev: Vec<StatEv>,
    /// events to forward to the screens (online server)
    pub net_ev: Vec<Ev>,
    /// whose side this screen is on: the team, and which players it controls
    pub my_team: Option<usize>,
    pub my_players: [[bool; 3]; 2],
    /// online: people on each team (a team with nobody is played by the computer)
    pub humans: [u8; 2],
    /// online: this screen only mirrors the server's match
    pub mirror: bool,
    pub pred: crate::proto::Pred,
    /// online: extra time to let go after STOP, because the player hears it late (their connection)
    pub stop_grace: [f32; 2],
    /// online: when the tap being handled really happened (ms); arrival times say little about rhythm
    pub tap_ms: Option<f64>,
    pub net_theta: f32,
    pub net_look: f32,
    /// the match is paused for this person, going on after this many seconds anyway
    pub net_pause: Option<(u32, f32)>,
    pub net_aim_left: f32,
    pub ball_target: Option<(Vec3, Vec3)>,
    pub net_sneak: [f32; 2],
    pub net_run: bool,
    pub net_status: String,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            mode: Mode::Demo,
            diff: 1,
            state: State::Menu,
            teams: [Team::new("Die Durstigen", 1.0, [0.91, 0.69, 0.16]), Team::new("Kronkorken FC", -1.0, [0.18, 0.53, 0.72])],
            referee: Person::new(REF_POS, ref_face()),
            turn: 0,
            hitter: None,
            last_hitter: None,
            prev_state: State::Menu,
            run: None,
            win: None,
            ball: vec3(0.5, BR, 0.4),
            ball_vel: Vec3::ZERO,
            ball_rot: glam::Quat::IDENTITY,
            ball_curve: 0.0,
            ball_free: false,
            holder: None,
            theta: 0.0,
            omega: 0.0,
            tip_dir: vec3(1.0, 0.0, 0.0),
            bottle_up: true,
            bottle_falling: false,
            toppled: false,
            knocked: Vec::new(),
            ref_target: -1,
            ref_until: 0.0,
            ref_walk: 0,
            ref_walk_t: 0,
            ref_walk_i: 0,
            ref_walk_tm: 0.0,
            ref_bottle: None,
            ref_hold: false,
            gaze_col: [1.0; 3],
            gaze_op: 0.0,
            smoke: Vec::new(),
            smoke_next: 0.0,
            smoke_wisp: 0.0,
            splash: Vec::new(),
            game_time: 0.0,
            clock: 0.0,
            phys_acc: 0.0,
            flight_t: 0.0,
            flight_no: 0,
            ln_i: 0,
            throw_ring: 0,
            throw_curve: 0.0,
            timers: Vec::new(),
            gen: 0,
            sneaks: Vec::new(),
            my_sneak_t: -1.0,
            rng: Rng::new(seed),
            sfx: Vec::new(),
            banners: Vec::new(),
            trail: Vec::new(),
            show_over: false,
            bounds: None,
            train: None,
            train_result: None,
            tut: None,
            stat_ev: Vec::new(),
            net_ev: Vec::new(),
            my_team: None,
            my_players: [[false; 3]; 2],
            humans: [0; 2],
            mirror: false,
            pred: Default::default(),
            stop_grace: [0.0; 2],
            tap_ms: None,
            net_theta: 0.0,
            net_look: 0.0,
            net_pause: None,
            net_aim_left: -1.0,
            ball_target: None,
            net_sneak: [-1.0; 2],
            net_run: false,
            net_status: String::new(),
        };
        g.referee.anim.mode = AMode::Sit;
        g.reset_game();
        g
    }

    // ---------- who controls what ----------
    pub fn ctrl(&self, t: usize) -> Ctrl {
        match self.mode {
            Mode::Demo => Ctrl::Cpu,
            Mode::Cpu => {
                if t == 0 {
                    Ctrl::Local
                } else {
                    Ctrl::Cpu
                }
            }
            Mode::Hot => Ctrl::Local,
            Mode::Online => {
                if self.humans[t] > 0 {
                    Ctrl::Local
                } else {
                    Ctrl::Cpu
                }
            }
        }
    }
    pub fn persp(&self) -> usize {
        if self.mode == Mode::Hot {
            self.turn
        } else {
            0
        }
    }
    pub fn local_aiming(&self) -> bool {
        self.state == State::Aim && self.my_players[self.turn][self.teams[self.turn].idx] && !self.tut.map_or(false, |t| t.hold())
    }
    pub fn can_drink(&self, t: usize) -> bool {
        self.state == State::Run && self.hitter == Some(t)
    }
    pub fn run_phase_active(&self) -> bool {
        if self.mirror {
            return self.net_run;
        }
        self.run.as_ref().map_or(false, |r| r.phase != Phase::Done || r.phase2 != Phase::Done)
    }
    pub fn can_run(&self, t: usize) -> bool {
        self.state == State::Run && self.hitter == Some(1 - t) && self.run_phase_active()
    }
    pub fn false_start_window(&self, t: usize) -> bool {
        self.state == State::Flight && self.turn == 1 - t
    }
    pub fn in_match(&self) -> bool {
        self.mode != Mode::Demo && self.state != State::Menu
    }

    /// Set whose side this screen is on (offline modes).
    pub fn set_sides(&mut self) {
        match self.mode {
            Mode::Cpu => {
                self.my_team = Some(0);
                self.my_players = [[true; 3], [false; 3]];
            }
            Mode::Hot => {
                self.my_team = None;
                self.my_players = [[true; 3]; 2];
            }
            Mode::Demo => {
                self.my_team = None;
                self.my_players = [[false; 3]; 2];
            }
            Mode::Online => {}
        }
    }
    fn announce(&mut self, ev: Ev) {
        if self.mode != Mode::Demo {
            self.net_ev.push(ev);
            self.word(ev);
        }
    }
    fn mine_team(&self, t: usize) -> bool {
        self.my_team == Some(t)
    }
    /// Turn an event into a banner, from this screen's side.
    pub fn word(&mut self, ev: Ev) {
        let hot = self.mode == Mode::Hot;
        match ev {
            Ev::Turn(t, i) => {
                let name = self.teams[t].name;
                if self.my_players[t][i] {
                    self.banner(if hot { name } else { "Your throw" }, "Flick the ball at the bottle", 1.8);
                } else if self.mine_team(t) {
                    self.banner("Teammate throws", "Your team's turn", 1.5);
                } else if self.mine_team(1 - t) {
                    self.banner("Their throw", "Get your feet ready, but wait for the hit", 1.5);
                }
            }
            Ev::Hit(t, ring, curve) => {
                let mut labels: Vec<String> = Vec::new();
                let mut mult = 1.0;
                if ring > 0 {
                    mult *= RING_M[ring as usize];
                    labels.push(RING_L[ring as usize].into());
                }
                if curve {
                    mult *= 1.2;
                    labels.push("Curveball".into());
                }
                if mult > 1.001 {
                    labels.push(format!("×{} flow", (mult * 100.0f32).round() / 100.0));
                }
                let bonus = if labels.is_empty() { String::new() } else { labels.join(" · ") + " · " };
                let name = self.teams[t].name;
                if hot {
                    self.banner(format!("{name} hit!"), bonus + if t == 0 { "Left drinks, right runs" } else { "Right drinks, left runs" }, 2.2);
                } else if self.mine_team(t) {
                    self.banner("HIT!", bonus + "Hold DRINK", 2.0);
                } else if self.mine_team(1 - t) {
                    self.banner("They hit it", "Run! Tap L and R in turn", 1.6);
                } else {
                    self.banner(format!("{name} hit!"), bonus + "They drink", 1.6);
                }
            }
            Ev::Miss(_) => self.banner("Miss", "", 0.9),
            Ev::Stop => self.banner("STOP!", "Let go of your beer", 1.2),
            Ev::Out(t, left) => {
                let rest = format!("{left} {}", if left == 1 { "player left" } else { "players left" });
                let mine = !hot && self.mine_team(t);
                let title = if hot { format!("{}: one done", self.teams[t].name) } else if mine { "Empty!".into() } else { "One of theirs is done".into() };
                self.banner(title, if mine { format!("Off the pitch · {rest}") } else { rest }, 1.3);
            }
            Ev::Over(w) => {
                let title = if hot { self.teams[w].name.to_string() } else if self.mine_team(w) { "Empty!".into() } else { "They're empty".into() };
                self.banner(title, "", 1.4);
            }
            Ev::Straf(t, code) => {
                let who = if hot { format!("{}: +1 beer", self.teams[t].name) } else if self.mine_team(t) { "+1 beer for you".into() } else { "+1 beer for them".into() };
                if self.mode != Mode::Demo {
                    self.banners.push(Banner { title: "STRAFBIER!".into(), sub: format!("{who} · {}", code.text()), dur: 2.0, straf: true });
                }
            }
            Ev::Unseen(t, code) => {
                let what = code.text().to_lowercase();
                if hot {
                    let n = self.teams[t].name;
                    self.banner("Ref missed it!", format!("{n}: {what}"), 1.5);
                } else if self.mine_team(t) {
                    self.banner("Got away with it", format!("The ref didn't see your {what}"), 1.5);
                } else {
                    let art = if what.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
                    self.banner("Ref missed it!", format!("They got away with {art} {what}"), 1.5);
                }
            }
            Ev::SneakOk(t) => {
                if self.mine_team(t) {
                    self.banner("Sneaky sip!", "The ref didn't see a thing", 1.3);
                } else if self.mine_team(1 - t) {
                    self.banner("Ref missed it!", "They got away with an illegal sip", 1.5);
                }
            }
        }
    }
    fn stat(&mut self, e: StatEv) {
        if self.mode != Mode::Demo && self.train.is_none() && self.tut.is_none() {
            self.stat_ev.push(e);
        }
    }
    pub fn set_tut(&mut self, k: Tut) {
        if self.tut.is_some() && self.tut != Some(k) {
            self.tut = Some(k);
            self.banners.clear();
        }
    }
    fn tut_if(&mut self, k: Tut) {
        if self.tut.is_some() {
            self.set_tut(k);
        }
    }
    fn practice(&self) -> bool {
        self.train.is_some() || self.tut.is_some()
    }
    fn sfx(&mut self, s: Sfx) {
        if self.mode != Mode::Demo && !self.mirror {
            self.sfx.push(s);
        }
    }
    fn banner(&mut self, title: impl Into<String>, sub: impl Into<String>, dur: f32) {
        if self.mode != Mode::Demo {
            self.banners.push(Banner { title: title.into(), sub: sub.into(), dur, straf: false });
        }
    }
    fn wait(&mut self, secs: f32, a: Act) {
        self.timers.push((self.game_time + secs, self.gen, a));
    }

    // ---------- match flow ----------
    pub fn reset_game(&mut self) {
        self.gen += 1;
        self.timers.clear();
        for ti in 0..2 {
            let face = self.teams[ti].face();
            let mult = [0.9 + self.rng.f() * 0.25, 0.9 + self.rng.f() * 0.25, 0.9 + self.rng.f() * 0.25];
            let t = &mut self.teams[ti];
            t.beers = [1.0; 3];
            t.out = [false; 3];
            t.idx = 0;
            t.hits = 0;
            t.throws = 0;
            t.straf = 0;
            t.drinking = false;
            t.late_ok = false;
            t.early = 0.0;
            t.dc = DrinkCtl::default();
            t.rc = RunCtl::default();
            t.mult = mult;
            for i in 0..3 {
                let p = &mut t.players[i];
                p.pos = t.homes[i];
                p.yaw = face;
                p.visible = true;
                p.anim = Anim::default();
            }
        }
        self.theta = 0.0;
        self.omega = 0.0;
        self.bottle_up = true;
        self.bottle_falling = false;
        self.toppled = false;
        self.ref_sit();
        self.run = None;
        self.hitter = None;
        self.last_hitter = None;
        self.win = None;
        self.knocked.clear();
        self.sneaks.clear();
        self.my_sneak_t = -1.0;
        self.ball_free = false;
        self.holder = None;
        self.trail.clear();
    }

    pub fn start_match(&mut self, mode: Mode, diff: usize) {
        self.train = None;
        self.tut = None;
        self.mode = mode;
        self.diff = diff;
        self.set_sides();
        self.banners.clear();
        self.reset_game();
        self.start_turn(0);
    }

    /// Online server: restart once the sides are known.
    pub fn restart_online(&mut self) {
        self.reset_game();
        self.start_turn(0);
    }
    /// Online server: people dropped out or came back; nobody may be left waiting for a throw that
    /// can't come, and nobody keeps drinking on a thumb that is gone.
    pub fn sides_changed(&mut self) {
        for t in 0..2 {
            if self.ctrl(t) == Ctrl::Cpu {
                self.teams[t].dc.hold = false;
            }
        }
        if self.state == State::Aim && self.ctrl(self.turn) == Ctrl::Cpu {
            self.state = State::CpuAim;
            let t = self.turn;
            let i = self.teams[t].idx;
            self.holder = Some((t, i));
            self.wait(1.3, Act::CpuWind(t));
        }
    }
    /// Online server: throw for someone who isn't throwing, a bit sloppily.
    pub fn auto_throw(&mut self) {
        if self.state != State::Aim {
            return;
        }
        let t = self.turn;
        let o = self.teams[t].players[self.teams[t].idx].hand();
        let d = DIFFS[0];
        let yaw = (-o.x).atan2(-o.z) + self.rng.gauss() * d.aim_sd;
        let dist = (o.x * o.x + o.z * o.z).sqrt() - 0.3 + self.rng.gauss() * d.dist_sd;
        self.launch(t, ThrowSpec { origin: o, yaw, dist, curve: 0.0, ring: 0 });
    }
    pub fn start_tutorial(&mut self) {
        self.start_match(Mode::Cpu, 0);
        self.tut = Some(Tut::Intro);
        self.banners.clear();
    }

    pub fn begin_train(&mut self, k: Drill) {
        self.start_match(Mode::Cpu, 1);
        self.banners.clear();
        self.train = Some(Train { kind: k, n: 0, hits: 0, rings: 0, t: 0.0, rt0: 0.0, pen: 0.0, clock: false, runs: Vec::new(), done: false });
        self.reset_game();
        match k {
            Drill::Throw => self.start_turn(0),
            Drill::Run => self.start_turn(1),
            Drill::Drink => {
                self.state = State::Stopped;
                let a = &mut self.teams[0];
                a.mult = [1.0; 3];
                a.beers = [1.0, 0.0, 0.0];
                a.out = [false, true, true];
                self.banner("Drinking drill", "Get ready to hold DRINK", 1.4);
                self.wait(1.6, Act::DrinkGo);
            }
        }
    }
    /// The line under the drill's name while it runs.
    pub fn drill_hud(&self) -> Option<(String, String)> {
        let tr = self.train.as_ref()?;
        let v = match tr.kind {
            Drill::Throw => {
                let k = (tr.n + (self.state == State::Aim) as u32).clamp(1, DRILL_THROWS);
                format!("Throw {k} of {DRILL_THROWS} · {} {}", tr.hits, if tr.hits == 1 { "hit" } else { "hits" })
            }
            Drill::Drink => format!("{:.1} s{}", tr.t + tr.pen, if tr.pen > 0.0 { format!(" (+{} s)", tr.pen) } else { String::new() }),
            Drill::Run => {
                let cur = if tr.clock { tr.t - tr.rt0 } else { tr.runs.last().copied().unwrap_or(0.0) };
                let k = (tr.runs.len() + (tr.clock || tr.runs.is_empty()) as usize).min(DRILL_RUNS);
                format!("Run {k} of {DRILL_RUNS} · {cur:.1} s{}", if tr.pen > 0.0 { format!(" (+{} s)", tr.pen) } else { String::new() })
            }
        };
        Some((tr.kind.name().into(), v))
    }
    fn reset_bottle(&mut self) {
        self.theta = 0.0;
        self.omega = 0.0;
        self.bottle_up = true;
        self.bottle_falling = false;
        self.toppled = false;
    }
    fn train_finish(&mut self) {
        let Some(tr) = self.train.as_mut() else { return };
        if tr.done {
            return;
        }
        tr.done = true;
        tr.clock = false;
        let (v, note) = match tr.kind {
            Drill::Throw => (tr.hits as f32, if tr.hits == 10 { "Ten out of ten.".to_string() } else if tr.rings > 0 { format!("{} of your throws got a ring bonus.", tr.rings) } else { "Let go while the ring is small for a bonus.".to_string() }),
            Drill::Drink => (((tr.t + tr.pen) * 10.0).round() / 10.0, if tr.pen > 0.0 { format!("Includes +{} s for choking and spilling.", tr.pen) } else { "Not a drop spilled.".into() }),
            Drill::Run => {
                let sum: f32 = tr.runs.iter().sum();
                let best = tr.runs.iter().cloned().fold(f32::INFINITY, f32::min);
                (((sum + tr.pen) * 10.0).round() / 10.0, format!("Fastest run {best:.1} s.{}", if tr.pen > 0.0 { format!(" Includes +{} s for false starts.", tr.pen) } else { String::new() }))
            }
        };
        let k = tr.kind;
        self.state = State::Stopped;
        self.teams[0].drinking = false;
        self.train_result = Some((k, v, note));
    }

    pub fn to_menu(&mut self) {
        self.train = None;
        self.tut = None;
        self.mode = Mode::Demo;
        self.set_sides();
        self.reset_game();
        self.start_turn(0);
    }

    fn start_turn(&mut self, t: usize) {
        if let Some(tr) = self.train.as_ref() {
            if (tr.kind == Drill::Throw && t == 0) || (tr.kind == Drill::Run && t == 1) {
                self.teams[t].idx = 0;
            }
        }
        self.turn = t;
        self.hitter = None;
        for x in self.teams.iter_mut() {
            x.late_ok = false;
            x.early = 0.0;
        }
        self.knocked.clear();
        self.trail.clear();
        let idx = self.teams[t].active_idx(self.teams[t].idx);
        self.teams[t].idx = idx;
        let th = &mut self.teams[t].players[idx];
        th.yaw = (-th.pos.x).atan2(-th.pos.z);
        self.ball_free = false;
        self.ball_vel = Vec3::ZERO;
        self.ball_curve = 0.0;
        self.holder = if self.my_players[t][idx] { None } else { Some((t, idx)) };
        self.state = if self.ctrl(t) == Ctrl::Cpu { State::CpuAim } else { State::Aim };
        if self.train.is_some() {
            if self.ctrl(t) == Ctrl::Local {
                self.banner("Your throw", "", 0.9);
            }
        } else {
            self.announce(Ev::Turn(t, idx));
        }
        if self.state == State::CpuAim {
            self.wait(1.3, Act::CpuWind(t));
        }
    }

    fn range_for(v: f32, h0: f32, a: f32) -> f32 {
        let (mut x, mut y, mut vx, mut vy) = (0.0f32, h0, v * a.cos(), v * a.sin());
        let h = 1.0 / 120.0;
        for _ in 0..1500 {
            let sp = (vx * vx + vy * vy).sqrt();
            vx -= KD * sp * vx * h;
            vy -= (G + KD * sp * vy) * h;
            x += vx * h;
            y += vy * h;
            if y <= BR {
                return x;
            }
        }
        x
    }
    fn solve_speed(h0: f32, d: f32, a: f32) -> f32 {
        let (mut lo, mut hi) = (0.5f32, 40.0f32);
        for _ in 0..26 {
            let m = (lo + hi) / 2.0;
            if Self::range_for(m, h0, a) < d {
                lo = m
            } else {
                hi = m
            }
        }
        (lo + hi) / 2.0
    }

    pub fn launch(&mut self, t: usize, o: ThrowSpec) {
        let ti = self.teams[t].idx;
        self.teams[t].idx = (ti + 1) % 3;
        self.teams[t].throws += 1;
        self.ball = o.origin;
        let a = 0.5f32;
        let d = o.dist.max(1.2);
        let v = Self::solve_speed(o.origin.y, d, a);
        self.ball_vel = vec3(o.yaw.sin() * a.cos() * v, a.sin() * v, o.yaw.cos() * a.cos() * v);
        self.ball_curve = o.curve;
        self.phys_acc = 0.0;
        self.ln_i += 1;
        // a computer thrower is already winding up; anyone else jumps to the release in the animation
        let a = &mut self.teams[t].players[ti].anim;
        if a.throw_t < 0.0 || a.throw_t > crate::anim::THROW_RELEASE {
            a.throw_t = crate::anim::THROW_RELEASE;
        }
        self.teams[0].early = 0.0;
        self.teams[1].early = 0.0;
        self.trail.clear();
        let opp = 1 - t;
        self.stat(StatEv::Throw { team: t, idx: ti });
        if let Some(tr) = self.train.as_mut() {
            if tr.kind == Drill::Throw && t == 0 {
                tr.n += 1;
                if o.ring > 0 {
                    tr.rings += 1;
                }
            }
        }
        if t == 0 {
            self.tut_if(Tut::Fly);
        }
        if self.ctrl(opp) == Ctrl::Cpu && self.mode != Mode::Demo && !self.practice() && self.rng.f() < DIFFS[self.diff].straf * 3.0 {
            self.wait(0.26, Act::CpuFalse(opp));
        }
        self.throw_ring = o.ring;
        self.throw_curve = o.curve;
        self.holder = None;
        self.ball_free = true;
        self.state = State::Flight;
        self.flight_t = 0.0;
        self.sfx(Sfx::Throw);
    }

    fn cpu_wind(&mut self, t: usize) {
        if self.state != State::CpuAim {
            return;
        }
        let i = self.teams[t].idx;
        self.teams[t].players[i].anim.throw_t = 0.0;
        self.wait(0.38, Act::CpuLaunch(t));
    }
    fn cpu_launch(&mut self, t: usize) {
        if self.state != State::CpuAim || self.turn != t {
            return;
        }
        let d = DIFFS[self.diff];
        let o = self.teams[t].players[self.teams[t].idx].hand();
        // in the tutorial and the running drill the other team always aims true, so you get to run
        let sd = if self.practice() { 0.0 } else { 1.0 };
        let yaw = (-o.x).atan2(-o.z) + self.rng.gauss() * d.aim_sd * sd;
        let dist = (o.x * o.x + o.z * o.z).sqrt() - 0.3 + self.rng.gauss() * d.dist_sd * sd;
        self.launch(t, ThrowSpec { origin: o, yaw, dist, curve: 0.0, ring: 0 });
    }

    // ---------- physics ----------
    fn step_ball(&mut self, h: f32, can_hit: bool) -> &'static str {
        let (mut p, mut v) = (self.ball, self.ball_vel);
        if p.y > BR + 0.004 {
            let sp = v.length();
            v.x -= KD * sp * v.x * h;
            v.y -= (G + KD * sp * v.y) * h;
            v.z -= KD * sp * v.z * h;
            if self.ball_curve != 0.0 {
                let hs = (v.x * v.x + v.z * v.z).sqrt().max(1e-6);
                let (vx, vz) = (v.x, v.z);
                v.x += CURVE_A * self.ball_curve * (vz / hs) * h;
                v.z -= CURVE_A * self.ball_curve * (vx / hs) * h;
            }
        } else {
            v.y -= G * h;
        }
        p += v * h;
        if p.y < BR {
            p.y = BR;
            if v.y < -0.9 {
                let imp = -v.y;
                v.y = imp * 0.42;
                v.x *= 0.72;
                v.z *= 0.72;
                self.ball_curve *= 0.3;
                self.sfx(Sfx::Bounce(imp / 4.0));
            } else {
                v.y = 0.0;
            }
        }
        let grounded = p.y <= BR + 1e-4 && v.y == 0.0;
        let mut sp = (v.x * v.x + v.z * v.z).sqrt();
        if grounded && sp > 0.0 {
            let ns = (sp - (1.5 + 0.9 * sp) * h).max(0.0);
            v.x *= ns / sp;
            v.z *= ns / sp;
            sp = ns;
        }
        let mut ret = "";
        if can_hit && self.bottle_up && !self.bottle_falling {
            let d = (p.x * p.x + p.z * p.z).sqrt();
            if d < R + BR && p.y < H + BR * 0.4 && d > 1e-4 {
                let (nx, nz) = (p.x / d, p.z / d);
                let dot = v.x * nx + v.z * nz;
                if dot < 0.0 {
                    self.omega = -dot * (p.y.clamp(0.03, H) / HC) * 1.3;
                    self.theta = 0.0;
                    self.tip_dir = vec3(-nx, 0.0, -nz);
                    self.bottle_falling = true;
                    v.x -= 1.55 * dot * nx;
                    v.z -= 1.55 * dot * nz;
                    v.x *= 0.6;
                    v.z *= 0.6;
                    self.ball_curve = 0.0;
                    p.x = nx * (R + BR + 0.001);
                    p.z = nz * (R + BR + 0.001);
                    self.sfx(Sfx::Touch(-dot / 4.0));
                    ret = "touch";
                }
            }
        }
        if ret.is_empty() && p.y < 0.25 + BR && p.z.abs() > LINE - 0.2 {
            'outer: for ti in 0..2 {
                for i in 0..3 {
                    let b = self.teams[ti].beers[i];
                    let n = if b > 0.0 { (b - 1e-6).ceil() as usize } else { 0 };
                    for k in 0..n.min(4) {
                        if self.knocked.iter().any(|(key, _)| *key == (ti, i, k)) {
                            continue;
                        }
                        let cp = self.teams[ti].can_pos(i, k);
                        let (dx, dz) = (p.x - cp[0], p.z - cp[1]);
                        let d = (dx * dx + dz * dz).sqrt();
                        if d < 0.033 + BR && d > 1e-4 {
                            let (nx, nz) = (dx / d, dz / d);
                            let dot = v.x * nx + v.z * nz;
                            if dot < 0.0 {
                                self.knocked.push(((ti, i, k), [-nx, -nz]));
                                v.x -= 1.4 * dot * nx;
                                v.z -= 1.4 * dot * nz;
                                v.x *= 0.8;
                                v.z *= 0.8;
                                self.sfx(Sfx::CanHit);
                                self.splash_at(vec3(cp[0], 0.12, cp[1]), 14);
                                self.ball = p;
                                self.ball_vel = v;
                                self.judge(ti, Foul::Can);
                                return "can";
                            }
                        }
                    }
                }
                let _ = &mut ret;
                continue 'outer;
            }
        }
        if sp > 0.01 {
            let ax = vec3(v.z, 0.0, -v.x).normalize();
            self.ball_rot = glam::Quat::from_axis_angle(ax, sp * h / BR) * self.ball_rot;
        }
        if let Some((bx, bz)) = self.bounds {
            // walls and railings send the ball back
            let (lx, lz) = (bx - BR, bz - BR);
            if (p.x > lx && v.x > 0.0) || (p.x < -lx && v.x < 0.0) {
                p.x = p.x.clamp(-lx, lx);
                v.x *= -0.5;
                self.ball_curve = 0.0;
                self.sfx(Sfx::Bounce(v.x.abs() / 3.0));
            }
            if (p.z > lz && v.z > 0.0) || (p.z < -lz && v.z < 0.0) {
                p.z = p.z.clamp(-lz, lz);
                v.z *= -0.5;
                self.ball_curve = 0.0;
                self.sfx(Sfx::Bounce(v.z.abs() / 3.0));
            }
        }
        if (p.x * p.x + p.z * p.z).sqrt() > 24.0 {
            v.x = 0.0;
            v.z = 0.0;
        }
        self.ball = p;
        self.ball_vel = v;
        if !ret.is_empty() {
            return ret;
        }
        if grounded && sp < 0.05 {
            "stop"
        } else {
            ""
        }
    }

    fn step_bottle(&mut self, h: f32) -> bool {
        if !self.bottle_falling {
            return false;
        }
        let mut ev = false;
        self.omega += kb() * (HC * self.theta.sin() - R * self.theta.cos()) * h;
        self.theta += self.omega * h;
        if !self.toppled && self.theta > crit() {
            self.toppled = true;
            self.bottle_up = false;
            ev = true;
        }
        if self.theta <= 0.0 {
            self.theta = 0.0;
            if self.omega < 0.0 {
                self.omega = -self.omega * 0.3;
                if self.omega < 0.35 {
                    self.omega = 0.0;
                    self.bottle_falling = false;
                }
            }
        }
        let half = std::f32::consts::FRAC_PI_2;
        if self.theta >= half {
            self.theta = half;
            if self.omega > 1.5 {
                let o = self.omega;
                self.sfx(Sfx::Bottle(o / 8.0));
                self.omega = -self.omega * 0.22;
            } else {
                self.omega = 0.0;
                self.bottle_falling = false;
            }
        }
        ev
    }

    // ---------- hit → run & drink ----------
    fn on_hit(&mut self) {
        if self.train.as_ref().map_or(false, |t| t.kind == Drill::Throw) {
            // throwing drill: count it, stand the bottle back up, next throw
            let tr = self.train.as_mut().unwrap();
            tr.hits += 1;
            let hits = tr.hits;
            self.state = State::Stopped;
            let mut lb = Vec::new();
            if self.throw_ring > 0 {
                lb.push(RING_L[self.throw_ring as usize].to_string());
            }
            if self.throw_curve != 0.0 {
                lb.push("Curveball".into());
            }
            let sub = if lb.is_empty() { format!("{hits} down") } else { lb.join(" · ") };
            self.banner("HIT!", sub, 1.2);
            self.sfx(Sfx::Hit);
            self.wait(1.5, Act::TrainNext);
            return;
        }
        if self.train.is_some() {
            let o = 1 - self.turn;
            self.teams[o].early = 0.0;
        }
        let hit = self.turn;
        let runners = 1 - hit;
        self.teams[hit].hits += 1;
        let ri = self.teams[runners].active_idx(self.teams[runners].idx);
        self.teams[runners].idx = ri;
        let mut mult = 1.0;
        let mut labels: Vec<String> = Vec::new();
        if self.throw_ring > 0 {
            mult *= RING_M[self.throw_ring as usize];
            labels.push(RING_L[self.throw_ring as usize].into());
        }
        if self.throw_curve != 0.0 {
            mult *= 1.2;
            labels.push("Curveball".into());
        }
        if mult > 1.001 {
            labels.push(format!("×{} flow", (mult * 100.0f32).round() / 100.0));
        }
        let ri2 = if self.train.is_some() { None } else { self.teams[runners].runner2_idx() };
        self.stat(StatEv::Hit { team: hit, ring: self.throw_ring, curve: self.throw_curve != 0.0 });
        self.tut_if(if hit == 0 { Tut::Drink } else { Tut::Run });
        if let Some(tr) = self.train.as_mut() {
            if tr.kind == Drill::Run {
                tr.rt0 = tr.t;
                tr.clock = true;
            }
        }
        let (mut bx2, mut bz2) = (0.0, 0.0);
        if let Some(i2) = ri2 {
            let q = self.teams[runners].players[i2].pos;
            let d = (q.x * q.x + q.z * q.z).sqrt().max(1e-6);
            bx2 = q.x / d * 0.42;
            bz2 = q.z / d * 0.42;
            if ((bx2 - self.tip_dir.x * H).powi(2) + (bz2 - self.tip_dir.z * H).powi(2)).sqrt() < 0.35 {
                bx2 = q.x.signum() * 0.42;
                if q.x == 0.0 {
                    bx2 = 0.42;
                }
                bz2 = 0.0;
            }
        }
        self.run = Some(RunState {
            runners,
            ri,
            phase: Phase::Ball,
            boost: self.teams[runners].early * 0.6,
            mult,
            t: 0.0,
            pt: 0.0,
            ft: 0.0,
            th0: 0.0,
            bx: 0.0,
            bz: 0.0,
            ri2,
            phase2: if ri2.is_some() { Phase::Bottle } else { Phase::Done },
            ft2: 0.0,
            th2: 0.0,
            bx2,
            bz2,
        });
        self.teams[runners].early = 0.0;
        self.ref_until = 0.0;
        self.hitter = Some(hit);
        self.teams[hit].drinking = true;
        self.state = State::Run;
        self.sfx(Sfx::Hit);
        let _ = labels;
        self.announce(Ev::Hit(hit, self.throw_ring, self.throw_curve != 0.0));
    }

    fn move_toward(p: &mut Person, tx: f32, tz: f32, speed: f32, dt: f32, steps: &mut bool) -> f32 {
        let (dx, dz) = (tx - p.pos.x, tz - p.pos.z);
        let d = (dx * dx + dz * dz).sqrt();
        if d < 1e-3 {
            return 0.0;
        }
        // accelerate like a person (about 5 m/s²) and ease off before arriving
        let want = speed.min((2.0 * 7.0 * d).sqrt() + 0.35);
        p.vel += (want - p.vel).clamp(-9.0 * dt, 5.5 * dt);
        let s = d.min(p.vel.max(0.3) * dt);
        p.pos.x += dx / d * s;
        p.pos.z += dz / d * s;
        p.yaw = dx.atan2(dz);
        let a = &mut p.anim;
        let prev = a.phase.sin();
        a.mode = AMode::Run;
        a.speed = speed;
        if prev * (a.phase + dt * speed.max(1.2) * 2.7).sin() < 0.0 {
            *steps = true;
        }
        d - s
    }
    fn still(p: &mut Person, m: AMode) {
        p.anim.mode = m;
        p.vel = 0.0;
        p.pos.y = 0.0;
    }

    fn update_run(&mut self, dt: f32) {
        let d = DIFFS[self.diff];
        let Some(mut r) = self.run.take() else { return };
        r.boost *= (-2.2 * dt).exp();
        r.t += dt;
        let speed = if self.ctrl(r.runners) == Ctrl::Cpu { d.run * (0.95 + 0.1 * (r.t * 3.0).sin()) + r.boost * 0.5 } else { 1.4 + r.boost };
        let mut step = false;
        let tip = self.tip_dir;
        // first runner: the ball
        match r.phase {
            Phase::Ball => {
                let bp = self.ball;
                let p = &mut self.teams[r.runners].players[r.ri];
                Self::move_toward(p, bp.x, bp.z, speed, dt, &mut step);
                let pp = p.pos;
                let bv = (self.ball_vel.x.powi(2) + self.ball_vel.z.powi(2)).sqrt();
                if ((bp.x - pp.x).powi(2) + (bp.z - pp.z).powi(2)).sqrt() < 0.5 && bv < 3.0 {
                    self.ball_free = false;
                    self.holder = Some((r.runners, r.ri));
                    r.phase = Phase::Pick;
                    r.pt = 0.3;
                }
            }
            Phase::Pick => {
                let p = &mut self.teams[r.runners].players[r.ri];
                Self::still(p, AMode::Bend);
                r.pt -= dt;
                if r.pt <= 0.0 && r.ri2.is_some() {
                    r.phase = Phase::Home;
                } else if r.pt <= 0.0 {
                    let d = (p.pos.x.powi(2) + p.pos.z.powi(2)).sqrt().max(1e-6);
                    r.bx = p.pos.x / d * 0.42;
                    r.bz = p.pos.z / d * 0.42;
                    if (r.bx - tip.x * H).abs() < 0.3 && (r.bz - tip.z * H).abs() < 0.3 {
                        r.bx = -r.bx;
                        r.bz = -r.bz;
                    }
                    r.phase = Phase::Bottle;
                }
            }
            Phase::Bottle => {
                let p = &mut self.teams[r.runners].players[r.ri];
                if Self::move_toward(p, r.bx, r.bz, speed, dt, &mut step) < 0.03 {
                    r.phase = Phase::Fix;
                    r.ft = 0.0;
                    r.th0 = self.theta;
                    self.bottle_falling = false;
                    self.omega = 0.0;
                    p.yaw = (-p.pos.x).atan2(-p.pos.z);
                }
            }
            Phase::Fix => {
                let p = &mut self.teams[r.runners].players[r.ri];
                Self::still(p, AMode::Bend);
                r.ft += dt;
                let k = (r.ft / 0.7).min(1.0);
                self.theta = r.th0 * (1.0 - ease(k));
                if k >= 1.0 {
                    self.bottle_up = true;
                    self.toppled = false;
                    self.theta = 0.0;
                    r.phase = Phase::Home;
                }
            }
            Phase::Home => {
                let h = self.teams[r.runners].homes[r.ri];
                let p = &mut self.teams[r.runners].players[r.ri];
                if Self::move_toward(p, h.x, h.z, speed, dt, &mut step) < 0.02 {
                    r.phase = Phase::Done;
                    Self::still(p, AMode::Idle);
                    p.yaw = (-p.pos.x).atan2(-p.pos.z);
                }
            }
            Phase::Done => {}
        }
        // second runner: the bottle
        if let Some(i2) = r.ri2 {
            match r.phase2 {
                Phase::Bottle => {
                    let q = &mut self.teams[r.runners].players[i2];
                    if Self::move_toward(q, r.bx2, r.bz2, speed, dt, &mut step) < 0.03 {
                        r.phase2 = Phase::Fix;
                        r.ft2 = 0.0;
                        r.th2 = self.theta;
                        self.bottle_falling = false;
                        self.omega = 0.0;
                        q.yaw = (-q.pos.x).atan2(-q.pos.z);
                    }
                }
                Phase::Fix => {
                    let q = &mut self.teams[r.runners].players[i2];
                    Self::still(q, AMode::Bend);
                    r.ft2 += dt;
                    let k = (r.ft2 / 0.7).min(1.0);
                    self.theta = r.th2 * (1.0 - ease(k));
                    self.bottle_falling = false;
                    self.omega = 0.0;
                    if k >= 1.0 {
                        self.bottle_up = true;
                        self.toppled = false;
                        self.theta = 0.0;
                        r.phase2 = Phase::Home;
                    }
                }
                Phase::Home => {
                    let h = self.teams[r.runners].homes[i2];
                    let q = &mut self.teams[r.runners].players[i2];
                    if Self::move_toward(q, h.x, h.z, speed, dt, &mut step) < 0.02 {
                        r.phase2 = Phase::Done;
                        Self::still(q, AMode::Idle);
                        q.yaw = (-q.pos.x).atan2(-q.pos.z);
                    }
                }
                _ => {}
            }
        }
        if step {
            self.sfx(Sfx::Step);
        }
        let hitter = 1 - r.runners;
        let done = r.phase == Phase::Done && r.phase2 == Phase::Done;
        self.run = Some(r);
        if done {
            self.stop_run();
            return;
        }
        if self.ctrl(hitter) == Ctrl::Cpu && self.train.is_none() {
            let amt = d.drink * dt * (0.8 + 0.4 * self.rng.f());
            self.drink(hitter, amt, None);
        }
    }

    /// Short text for the status line during a run.
    pub fn status_text(&self) -> String {
        if self.mirror {
            let mine = self.my_team.map_or(false, |t| self.hitter == Some(1 - t));
            return if mine { self.net_status.replace("Their runners", "Your team") } else { self.net_status.clone() };
        }
        self.status_text_for(self.my_team)
    }
    pub fn status_text_for(&self, me: Option<usize>) -> String {
        let (Some(h), Some(r)) = (self.hitter, self.run.as_ref()) else { return String::new() };
        if self.state != State::Run {
            return String::new();
        }
        let rn = 1 - h;
        let ph = if r.ri2.is_none() {
            r.phase
        } else if r.phase2 == Phase::Fix {
            Phase::Fix
        } else if matches!(r.phase, Phase::Ball | Phase::Pick) {
            r.phase
        } else if r.phase2 == Phase::Bottle {
            Phase::Bottle
        } else {
            Phase::Home
        };
        let who = if self.mode == Mode::Hot {
            format!("{} runners", self.teams[rn].name)
        } else if me == Some(rn) {
            "You".to_string()
        } else {
            "Their runners".to_string()
        };
        let what = match ph {
            Phase::Ball | Phase::Pick => "get the ball",
            Phase::Bottle | Phase::Fix => "stand the bottle up",
            Phase::Home => "back behind the line",
            Phase::Done => return String::new(),
        };
        format!("{who}: {what}")
    }

    fn drink(&mut self, t: usize, amt: f32, _idxs: Option<&[usize]>) {
        if self.hitter != Some(t) || !(self.state == State::Run || (self.state == State::Stopped && self.teams[t].late_ok)) {
            return;
        }
        let tm = &mut self.teams[t];
        for i in 0..3 {
            tm.beers[i] = (tm.beers[i] - amt * tm.mult[i]).max(0.0);
        }
        if tm.beers.iter().all(|b| *b <= 0.0) {
            tm.out = [true; 3];
            if self.train.is_some() {
                self.train_finish();
                return;
            }
            self.end_game(t);
            return;
        }
        for i in 0..3 {
            if self.teams[t].beers[i] <= 0.0 && !self.teams[t].out[i] {
                self.teams[t].out[i] = true;
                let left = self.teams[t].beers.iter().filter(|b| **b > 0.0).count();
                self.announce(Ev::Out(t, left));
            }
        }
    }

    fn stop_run(&mut self) {
        let Some(r) = self.run.as_ref() else { return };
        let (runners, ri, ri2) = (r.runners, r.ri, r.ri2);
        let hitter = 1 - runners;
        if self.train.as_ref().map_or(false, |t| t.kind == Drill::Run) {
            // running drill: the runner is home
            self.state = State::Stopped;
            self.teams[hitter].drinking = false;
            Self::still(&mut self.teams[runners].players[ri], AMode::Idle);
            let tr = self.train.as_mut().unwrap();
            let tm = tr.t - tr.rt0;
            tr.clock = false;
            tr.runs.push(tm);
            let n = tr.runs.len();
            self.sfx(Sfx::Stop);
            self.banner(format!("{tm:.1} s"), format!("Run {n} of {DRILL_RUNS}"), 1.3);
            self.wait(1.5, Act::TrainRunNext);
            return;
        }
        self.stat(StatEv::Stop);
        self.tut_if(if hitter == 0 { Tut::Wait } else { Tut::Ref });
        self.state = State::Stopped;
        self.teams[hitter].drinking = false;
        Self::still(&mut self.teams[runners].players[ri], AMode::Idle);
        if let Some(i2) = ri2 {
            Self::still(&mut self.teams[runners].players[i2], AMode::Idle);
        }
        self.announce(Ev::Stop);
        self.sfx(Sfx::Stop);
        self.ref_until = 0.0;
        if self.ctrl(hitter) == Ctrl::Cpu && self.mode != Mode::Demo && !self.practice() && self.rng.f() < DIFFS[self.diff].straf * 2.5 {
            self.wait(0.35, Act::CpuLate(hitter));
        }
        self.wait(1.4, Act::StartTurn(runners));
    }

    fn on_miss(&mut self) {
        if self.state != State::Flight {
            return;
        }
        self.state = State::Stopped;
        self.announce(Ev::Miss(self.turn));
        self.sfx(Sfx::Miss);
        let t0 = self.turn;
        if t0 == 0 {
            self.tut_if(Tut::Retry);
        }
        if self.train.as_ref().map_or(false, |t| t.kind == Drill::Throw && t.n >= DRILL_THROWS) {
            self.wait(1.1, Act::TrainFinish);
            return;
        }
        // in the tutorial and in drills a miss means: try again
        self.wait(1.1, Act::StartTurn(if self.practice() { t0 } else { 1 - t0 }));
    }

    fn end_game(&mut self, winner: usize) {
        if self.train.is_some() {
            self.train_finish();
            return;
        }
        self.stat(StatEv::Over { winner });
        self.state = State::Over;
        self.win = Some(winner);
        self.teams[0].drinking = false;
        self.teams[1].drinking = false;
        if self.mode == Mode::Demo {
            self.wait(4.0, Act::DemoRestart(winner));
            return;
        }
        self.announce(Ev::Over(winner));
        self.sfx(Sfx::Win);
        self.wait(1.5, Act::ShowOver(winner));
    }

    /// Strafbier: the offender gets one more full beer on top of what they have left.
    fn add_straf(&mut self, t: usize, code: Foul) {
        if matches!(self.state, State::Over | State::Menu) || self.teams[t].straf >= MAX_STRAF {
            return;
        }
        if self.tut.is_some() {
            // practice round: fouls are shown, never punished
            self.banner("That's a foul", format!("{} · it doesn't count in practice", code.text()), 1.5);
            return;
        }
        self.stat(StatEv::Straf { team: t });
        self.teams[t].straf += 1;
        let tm = &self.teams[t];
        let mut i = tm.active_idx(0);
        let mut lo = f32::INFINITY;
        for k in 0..3 {
            if tm.beers[k] > 0.0 && tm.beers[k] < lo {
                lo = tm.beers[k];
                i = k;
            }
        }
        self.teams[t].beers[i] += 1.0;
        self.announce(Ev::Straf(t, code));
        self.sfx(Sfx::Straf);
        match code {
            Foul::Spill => {
                let p = (0..3).find(|k| self.teams[t].beers[*k] > 0.0).unwrap_or(0);
                let hp = self.teams[t].players[p].hand();
                self.splash_at(hp, 18);
            }
            Foul::Stop => {
                self.teams[t].dc.hold = false;
                self.teams[t].dc.late = false;
            }
            Foul::False => self.teams[t].rc.caught_for = self.flight_no,
            Foul::Sneak => {
                if self.ctrl(t) == Ctrl::Local {
                    self.my_sneak_t = -1.0;
                }
            }
            _ => {}
        }
        self.ref_go_to(t, i);
    }

    // ---------- the referee ----------
    fn team_yaw(&self, t: usize) -> f32 {
        (-REF_POS.x).atan2(self.teams[t].side * (LINE + 0.5) - REF_POS.z)
    }
    pub fn ref_head_yaw(&self) -> f32 {
        self.referee.yaw + self.referee.anim.look
    }
    pub fn ref_sees(&self, t: usize) -> bool {
        if self.ref_walk != 0 {
            return self.ref_walk != 3 && self.ref_walk_t == t;
        }
        !self.referee.anim.smoking && wrap_a(self.ref_head_yaw() - self.team_yaw(t)).abs() < GAZE_HALF
    }
    fn ref_sit(&mut self) {
        self.ref_walk = 0;
        self.ref_bottle = None;
        self.ref_hold = false;
        let a = &mut self.referee.anim;
        a.mode = AMode::Sit;
        a.reach = false;
        a.hold = false;
        a.point = false;
        a.smoking = false;
        self.referee.pos = REF_POS;
        self.referee.yaw = ref_face();
    }
    fn ref_go_to(&mut self, t: usize, i: usize) {
        // a second foul while the first bottle is on its way: that one's there at once, as he goes back for another
        self.ref_walk = 4;
        self.ref_walk_t = t;
        self.ref_walk_i = i;
        self.ref_walk_tm = 0.0;
        self.ref_bottle = Some((t, i));
        self.ref_hold = false;
        self.referee.anim.smoking = false;
        self.referee.anim.point = false;
    }
    fn ref_walk_step(&mut self, dt: f32) {
        self.referee.anim.smoking = false;
        self.referee.anim.look = 0.0;
        self.referee.anim.hold = self.ref_hold;
        self.ref_walk_tm += dt;
        let mut st = false;
        match self.ref_walk {
            4 => {
                // still on the crate: down to the opening between his feet, and up with a bottle
                let tm = self.ref_walk_tm;
                self.referee.anim.reach = tm < 0.6;
                if tm > 0.42 {
                    self.ref_hold = true;
                }
                if tm > 0.85 {
                    self.referee.anim.reach = false;
                    self.ref_walk = 1;
                    self.ref_walk_tm = 0.0;
                }
            }
            1 => {
                let t = &self.teams[self.ref_walk_t];
                let spot = self.ref_spot();
                let (tx, tz) = (spot[0], spot[1] - t.side * 0.5);
                if Self::move_toward(&mut self.referee, tx, tz, 2.0, dt, &mut st) < 0.05 {
                    self.ref_walk = 2;
                    self.ref_walk_tm = 0.0;
                    self.sfx(Sfx::Whistle);
                }
            }
            2 => {
                let tm = self.ref_walk_tm;
                let p = self.teams[self.ref_walk_t].players[self.ref_walk_i].pos;
                let spot = self.ref_spot();
                let rp = self.referee.pos;
                // bend down and put it on the floor in front of the player, then point at them
                let (lx, lz) = if tm < 0.9 { (spot[0], spot[1]) } else { (p.x, p.z) };
                self.referee.yaw = (lx - rp.x).atan2(lz - rp.z);
                if tm > 0.5 && self.ref_hold {
                    self.ref_hold = false;
                    self.ref_bottle = None;
                    self.sfx(Sfx::Bottle(0.3));
                }
                let a = &mut self.referee.anim;
                a.mode = if tm < 0.85 { AMode::Bend } else { AMode::Idle };
                a.speed = 0.0;
                a.point = tm > 1.1;
                if tm > 2.5 {
                    a.point = false;
                    self.ref_walk = 3;
                }
            }
            3 => {
                if Self::move_toward(&mut self.referee, REF_POS.x, REF_POS.z, 1.8, dt, &mut st) < 0.03 {
                    self.ref_sit();
                    self.ref_until = 0.0;
                }
            }
            _ => {}
        }
    }
    /// Where on the floor the bottle he's bringing goes: the next free place in that player's row.
    fn ref_spot(&self) -> [f32; 2] {
        let t = &self.teams[self.ref_walk_t];
        let i = self.ref_walk_i;
        let n = (t.beers[i] - 1e-6).ceil().max(1.0) as usize;
        t.can_pos(i, (n - 1).min(3))
    }
    fn ref_choose(&mut self) {
        let tt = self.turn;
        let mut w: Vec<(i8, f32)> = Vec::new();
        let h = self.hitter.map(|x| x as i8).unwrap_or(-1);
        let lh = self.last_hitter.map(|x| x as i8).unwrap_or(-1);
        match self.state {
            State::Aim | State::CpuAim => w.extend([(tt as i8, 4.0), (1 - tt as i8, 2.0), (-1, 1.0), (2, 2.2)]),
            State::Flight => w.extend([(-1, 5.0), (1 - tt as i8, 3.0), (tt as i8, 1.0), (2, 1.0)]),
            State::Run => w.extend([(h, 4.0), (-1, 3.0), (if h >= 0 { 1 - h } else { -1 }, 1.2), (2, 1.0)]),
            State::Stopped => w.extend([(lh, 5.0), (-1, 1.0), (2, 1.2)]),
            _ => w.extend([(-1, 1.0), (2, 1.0), (0, 0.5), (1, 0.5)]),
        }
        let tot: f32 = w.iter().map(|x| x.1).sum();
        let mut r = self.rng.f() * tot;
        for (k, n) in w {
            r -= n;
            if r <= 0.0 {
                self.ref_target = k;
                break;
            }
        }
        self.ref_until = self.game_time + 0.9 + self.rng.f() * 1.8;
    }
    fn update_ref(&mut self, dt: f32) {
        if self.ref_walk != 0 {
            self.ref_walk_step(dt);
        } else {
            if self.game_time >= self.ref_until {
                self.ref_choose();
            }
            if self.tut == Some(Tut::Ref) {
                self.ref_target = 0;
                self.ref_until = self.game_time + 0.5;
            }
            let smoking = self.ref_target == 2;
            if smoking && !self.referee.anim.smoking {
                self.referee.anim.smoke_t = 0.0;
            }
            self.referee.anim.smoking = smoking;
            let tgt = match self.ref_target {
                0 => self.team_yaw(0),
                1 => self.team_yaw(1),
                _ => ref_face(),
            };
            let want = wrap_a(tgt - ref_face()).clamp(-1.45, 1.45);
            // the head turns quickly but not instantly
            let a = &mut self.referee.anim;
            a.look += (want - a.look) * (1.0 - (-10.0 * dt).exp());
            if a.mode != AMode::Sit {
                a.mode = AMode::Sit;
            }
        }
        if self.referee.anim.smoking {
            self.referee.anim.smoke_t += dt;
        }
        // the gaze fan on the lawn
        let live = self.state != State::Menu;
        let (mut tgt, mut op) = ([1.0, 1.0, 1.0], if live { 0.3 } else { 0.0 });
        if self.referee.anim.smoking || self.ref_walk == 3 {
            op = 0.0;
        } else if self.ref_sees(0) {
            tgt = [0.95, 0.71, 0.2];
            op = if live { 0.6 } else { 0.0 };
        } else if self.ref_sees(1) {
            tgt = [0.33, 0.73, 0.85];
            op = if live { 0.6 } else { 0.0 };
        }
        let k = 1.0 - (-7.0 * dt).exp();
        for c in 0..3 {
            self.gaze_col[c] += (tgt[c] - self.gaze_col[c]) * k;
        }
        let rate = if op > self.gaze_op { 5.0 } else { 9.0 };
        self.gaze_op += (op - self.gaze_op) * (1.0 - (-rate * dt).exp());
    }

    fn update_smoke(&mut self, dt: f32, mouth: Vec3, tip: Vec3) {
        let far = self.mode == Mode::Demo;
        let (s, o) = if far { (1.9, 1.35) } else { (1.0, 1.0) };
        if self.referee.anim.exhale() {
            self.smoke_next -= dt;
            if self.smoke_next <= 0.0 {
                self.smoke_next = 0.09;
                let f = self.referee.yaw;
                let v = vec3(f.sin() * 0.3 + (self.rng.f() - 0.5) * 0.12, 0.38 + self.rng.f() * 0.14, f.cos() * 0.3 + (self.rng.f() - 0.5) * 0.12);
                self.puff(mouth, v, 2.2, 0.05 * s, 0.42 * s, (0.55f32 * o).min(0.8));
            }
        }
        self.smoke_wisp -= dt;
        if self.smoke_wisp <= 0.0 {
            self.smoke_wisp = 0.32;
            let v = vec3((self.rng.f() - 0.5) * 0.05, 0.24, (self.rng.f() - 0.5) * 0.05);
            self.puff(tip, v, 1.8, 0.02 * s, 0.14 * s, (0.3f32 * o).min(0.6));
        }
        let clock = self.clock;
        for p in self.smoke.iter_mut() {
            p.life -= dt;
            p.vel *= 1.0 - 0.9 * dt;
            p.vel.y += 0.05 * dt;
            p.pos += p.vel * dt;
            p.pos.x += (clock * 0.7 + p.max).sin() * 0.03 * dt;
        }
        self.smoke.retain(|p| p.life > 0.0);
    }
    fn puff(&mut self, at: Vec3, v: Vec3, life: f32, s0: f32, s1: f32, o: f32) {
        if self.smoke.len() < 40 {
            self.smoke.push(Smoke { pos: at, vel: v, life, max: life, s0, s1, o });
        }
    }
    pub(crate) fn splash_at(&mut self, at: Vec3, n: usize) {
        for _ in 0..n {
            let a = self.rng.f() * std::f32::consts::TAU;
            let s = 0.6 + self.rng.f() * 1.2;
            self.splash.push(Splash { pos: at, vel: vec3(a.cos() * s, 1.2 + self.rng.f() * 1.6, a.sin() * s), life: 0.5 + self.rng.f() * 0.4 });
        }
    }

    /// Violations become Strafbier only when the referee sees them; a knocked-over can is always noticed.
    pub fn judge(&mut self, t: usize, code: Foul) -> bool {
        if matches!(self.state, State::Over | State::Menu) {
            return false;
        }
        if let Some(tr) = self.train.as_mut() {
            // drills: every foul of yours costs time, seen or not
            if code == Foul::Can || t != 0 || tr.done {
                return false;
            }
            tr.pen += 2.0;
            if code == Foul::False {
                self.teams[0].early = 0.0;
            }
            self.banners.push(Banner { title: "+2 seconds".into(), sub: code.text().into(), dur: 1.3, straf: true });
            self.sfx(Sfx::Straf);
            return true;
        }
        if code == Foul::False && self.teams[t].false_straf_for == self.ln_i {
            return false;
        }
        if code == Foul::Can || self.ref_sees(t) {
            if code == Foul::False {
                self.teams[t].false_straf_for = self.ln_i;
                self.teams[t].early = 0.0;
            }
            if code == Foul::Stop {
                self.teams[t].late_ok = false;
            }
            self.add_straf(t, code);
            return true;
        }
        if code == Foul::Stop {
            self.teams[t].late_ok = true;
        }
        if code == Foul::False {
            if self.teams[t].unseen_for == self.ln_i {
                return false;
            }
            self.teams[t].unseen_for = self.ln_i;
        }
        self.stat(StatEv::Unseen { team: t });
        self.announce(Ev::Unseen(t, code));
        false
    }

    // ---------- local controls ----------
    pub fn drink_press(&mut self, t: usize) {
        let can = self.can_drink(t);
        if !can && self.teams[t].dc.stop_show <= 0.0 {
            return;
        }
        if self.bot_tap(t) {
            self.judge(t, Foul::Auto);
            return;
        }
        if self.teams[t].dc.cough > 0.0 || !can {
            return;
        }
        let m = 0.8 + self.rng.f() * 0.45;
        let d = &mut self.teams[t].dc;
        d.hold = true;
        d.mul = m;
    }
    pub fn drink_release(&mut self, t: usize) {
        self.teams[t].dc.hold = false;
    }
    fn bot_tap(&mut self, t: usize) -> bool {
        let ts = self.tap_ms.unwrap_or(self.clock as f64 * 1000.0);
        let a = &mut self.teams[t].rc.times;
        a.push(ts);
        if a.len() > 16 {
            a.remove(0);
        }
        let n = a.len();
        if n < 10 {
            return false;
        }
        let bot = {
            if 8000.0 / (a[n - 1] - a[n - 9]).max(1.0) > 18.0 {
                true
            } else {
                let iv: Vec<f64> = (1..n).map(|i| a[i] - a[i - 1]).collect();
                let m = iv.iter().sum::<f64>() / iv.len() as f64;
                let sd = (iv.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / iv.len() as f64).sqrt();
                m < 160.0 && sd < 3.5
            }
        };
        if bot {
            a.clear();
        }
        bot
    }
    pub fn pad_press(&mut self, t: usize, foot: Foot) {
        self.teams[t].rc.down[foot as usize] = 0.08;
        if self.teams[t].rc.lock > 0.0 {
            return;
        }
        if self.bot_tap(t) {
            self.teams[t].rc.lock = 2.5;
            self.judge(t, Foul::Auto);
            return;
        }
        if self.false_start_window(t) {
            if self.teams[t].rc.last != Some(foot) {
                self.teams[t].early = (self.teams[t].early + 1.0).min(6.0);
            }
            self.teams[t].rc.last = Some(foot);
            let rc = &self.teams[t].rc;
            if rc.caught_for != self.flight_no && (rc.false_for != self.flight_no || self.game_time - rc.false_t > 0.8) {
                let fno = self.flight_no;
                let gt = self.game_time;
                self.teams[t].rc.false_for = fno;
                self.teams[t].rc.false_t = gt;
                self.judge(t, Foul::False);
            }
            return;
        }
        if !self.can_run(t) {
            return;
        }
        if self.teams[t].rc.last == Some(foot) {
            if let Some(r) = self.run.as_mut() {
                r.boost *= 0.45;
            }
            self.teams[t].rc.msg = "Stumble! Alternate L and R".into();
            self.teams[t].rc.msg_t = 0.9;
        } else {
            if let Some(r) = self.run.as_mut() {
                if r.runners == t {
                    r.boost = (r.boost + 0.75).min(5.5);
                }
            }
            self.sfx(Sfx::Step);
        }
        self.teams[t].rc.last = Some(foot);
    }

    fn update_drink_ctl(&mut self, t: usize, dt: f32) {
        let can = self.can_drink(t);
        let mut consume = 0.0;
        let mut spill = false;
        let mut glug = false;
        let mut stop = false;
        {
            let d = &mut self.teams[t].dc;
            if d.cough > 0.0 {
                d.cough -= dt;
                d.p = (d.p - dt * 0.5).max(0.0);
            }
            let late = d.late && d.stop_show > 0.0;
            if (can || late) && d.hold && d.cough <= 0.0 {
                d.ht += dt;
                consume = 0.09 * (0.5 + d.ht.min(2.5) * 0.6) * dt;
                d.p += dt * (0.35 + d.ht * 0.28) * d.mul;
                d.glug += dt;
                if d.glug > 0.32 {
                    d.glug = 0.0;
                    glug = true;
                }
                if d.p >= 1.0 {
                    d.p = 1.0;
                    d.cough = 1.4;
                    d.hold = false;
                    d.ht = 0.0;
                    spill = true;
                }
            } else {
                if !d.hold {
                    d.ht = 0.0;
                }
                d.p = (d.p - dt * 0.9).max(0.0);
            }
            if d.stop_watch > 0.0 {
                if !d.hold {
                    d.stop_watch = 0.0;
                } else {
                    d.stop_watch -= dt;
                    if d.stop_watch <= 0.0 {
                        d.late = true;
                        stop = true;
                    }
                }
            }
            if d.stop_show > 0.0 {
                d.stop_show -= dt;
            } else {
                d.late = false;
            }
        }
        if consume > 0.0 {
            let m = self.run.as_ref().map_or(1.0, |r| r.mult);
            self.drink(t, consume * m, None);
        }
        if glug {
            self.sfx(Sfx::Glug);
        }
        if spill {
            self.sfx(Sfx::Cough);
            self.judge(t, Foul::Spill);
        }
        if stop {
            self.judge(t, Foul::Stop);
        }
        let rc = &mut self.teams[t].rc;
        if rc.lock > 0.0 {
            rc.lock -= dt;
        }
        if rc.msg_t > 0.0 {
            rc.msg_t -= dt;
        }
        for d in rc.down.iter_mut() {
            *d -= dt;
        }
    }

    fn watch_stop(&mut self) {
        if self.state == State::Run {
            self.last_hitter = self.hitter;
        }
        if self.prev_state == State::Run && self.state != State::Run && self.state != State::Over {
            if let Some(lh) = self.last_hitter {
                if self.ctrl(lh) == Ctrl::Local {
                    let grace = self.stop_grace[lh];
                    let d = &mut self.teams[lh].dc;
                    d.stop_show = 1.1;
                    if d.hold {
                        d.stop_watch = 0.35 + grace;
                    }
                }
            }
        }
        if self.prev_state != State::Flight && self.state == State::Flight {
            self.flight_no += 1;
            for t in self.teams.iter_mut() {
                t.rc.last = None;
            }
        }
        self.prev_state = self.state;
    }

    // ---------- sneaky sips ----------
    fn sneak_idxs(&self, t: usize) -> Vec<usize> {
        (0..3)
            .filter(|&i| self.teams[t].beers[i] > SNEAK_LEFT && !(self.state == State::Run && self.run.as_ref().map_or(false, |r| r.runners == t && (r.ri == i || r.ri2 == Some(i)))))
            .collect()
    }
    fn sneak_ok(&self, t: usize, idxs: &[usize]) -> bool {
        matches!(self.state, State::Aim | State::CpuAim | State::Flight | State::Stopped | State::Run) && !self.can_drink(t) && !idxs.is_empty()
    }
    pub fn sneak_allowed(&self) -> bool {
        match self.my_team {
            Some(t) => self.sneak_allowed_for(t) && self.sneak_progress_team(t).is_none(),
            None => false,
        }
    }
    pub fn sneak_allowed_for(&self, t: usize) -> bool {
        let people = match self.mode {
            Mode::Cpu => t == 0,
            Mode::Online => self.humans[t] > 0,
            _ => false,
        };
        if self.mirror {
            return people && matches!(self.state, State::Aim | State::CpuAim | State::Flight | State::Stopped | State::Run) && !self.can_drink(t);
        }
        people && !self.practice() && self.sneak_ok(t, &self.sneak_idxs(t))
    }
    /// How far along a sneaky sip by people on team `t` is.
    pub fn sneak_progress_team(&self, t: usize) -> Option<f32> {
        if self.mirror {
            return (self.net_sneak[t] >= 0.0).then_some(self.net_sneak[t]);
        }
        self.sneaks.iter().find(|s| s.t == t && s.human).map(|s| ((self.game_time - s.start) / SNEAK_DUR).min(1.0))
    }
    pub fn start_sneak(&mut self) {
        if let Some(t) = self.my_team {
            self.start_sneak_for(t);
        }
    }
    pub fn start_sneak_for(&mut self, t: usize) {
        if self.sneak_progress_team(t).is_some() || !self.sneak_allowed_for(t) || self.mirror {
            return;
        }
        if self.my_team == Some(t) {
            self.my_sneak_t = self.game_time;
        }
        self.sfx(Sfx::Glug);
        if self.ref_sees(t) {
            self.my_sneak_t = -1.0;
            self.judge(t, Foul::Sneak);
            return;
        }
        let idxs = self.sneak_idxs(t);
        self.sneaks.push(Sneak { t, idxs, start: self.game_time, mine: self.my_team == Some(t), human: true });
    }
    pub fn sneak_vis(&self, t: usize, i: usize) -> bool {
        self.sneaks.iter().any(|s| s.t == t && s.idxs.contains(&i))
    }
    pub fn sneak_progress(&self) -> Option<f32> {
        self.my_team.and_then(|t| self.sneak_progress_team(t))
    }
    fn sneak_tick(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.sneaks.len() {
            let (t, start, mine) = (self.sneaks[i].t, self.sneaks[i].start, self.sneaks[i].mine);
            let live = matches!(self.state, State::Aim | State::CpuAim | State::Flight | State::Stopped | State::Run) && !self.can_drink(t);
            if !live {
                self.sneaks.remove(i);
                if mine {
                    self.my_sneak_t = -1.0;
                }
                continue;
            }
            if self.ref_sees(t) {
                self.sneaks.remove(i);
                if mine {
                    self.my_sneak_t = -1.0;
                }
                self.judge(t, Foul::Sneak);
                continue;
            }
            if self.game_time - start >= SNEAK_DUR {
                let s = self.sneaks.remove(i);
                let tm = &mut self.teams[t];
                for &k in &s.idxs {
                    if tm.beers[k] > SNEAK_LEFT {
                        tm.beers[k] = (tm.beers[k] - SNEAK_AMT * tm.mult[k]).max(SNEAK_LEFT);
                    }
                }
                self.stat(StatEv::Sneak { team: t });
                if mine {
                    self.my_sneak_t = -1.0;
                }
                self.announce(Ev::SneakOk(t));
                continue;
            }
            i += 1;
        }
        // the computer sneaks a sip now and then, mostly while the ref is smoking
        if self.mode != Mode::Hot && !self.practice() {
            for t in 0..2 {
                if self.ctrl(t) != Ctrl::Cpu || self.sneaks.iter().any(|s| s.t == t) || self.ref_sees(t) {
                    continue;
                }
                let rate = 0.045 * [0.6, 1.0, 1.7][self.diff] * if self.referee.anim.smoking { 2.5 } else { 1.0 };
                if self.rng.f() < dt * rate {
                    let ix = self.sneak_idxs(t);
                    if self.sneak_ok(t, &ix) {
                        self.sneaks.push(Sneak { t, idxs: ix, start: self.game_time, mine: false, human: false });
                    }
                }
            }
        }
    }

    fn run_timers(&mut self) {
        let now = self.game_time;
        let gen = self.gen;
        let mut due: Vec<Act> = Vec::new();
        self.timers.retain(|(at, g, a)| {
            if *g != gen {
                return false;
            }
            if *at <= now {
                due.push(*a);
                false
            } else {
                true
            }
        });
        for a in due {
            match a {
                Act::CpuWind(t) => self.cpu_wind(t),
                Act::CpuLaunch(t) => self.cpu_launch(t),
                Act::StartTurn(t) => {
                    if matches!(self.state, State::Stopped) {
                        self.run = None;
                        self.start_turn(t)
                    }
                }
                Act::ShowOver(_) => self.show_over = true,
                Act::CpuFalse(t) => {
                    if self.state == State::Flight {
                        self.teams[t].early = 4.0;
                        self.judge(t, Foul::False);
                    }
                }
                Act::CpuLate(t) => {
                    if !self.judge(t, Foul::Stop) {
                        self.teams[t].late_ok = true;
                        self.drink(t, 0.04, None);
                    }
                }
                Act::DrinkGo => {
                    if self.train.as_ref().map_or(false, |t| t.kind == Drill::Drink) {
                        self.state = State::Run;
                        self.hitter = Some(0);
                        self.teams[0].drinking = true;
                        self.train.as_mut().unwrap().clock = true;
                        self.sfx(Sfx::Whistle);
                        self.banner("Drink!", "Let go before the ring fills", 1.5);
                    }
                }
                Act::TrainNext => {
                    if let Some(n) = self.train.as_ref().map(|t| t.n) {
                        self.reset_bottle();
                        if n >= DRILL_THROWS {
                            self.train_finish();
                        } else {
                            self.start_turn(0);
                        }
                    }
                }
                Act::TrainRunNext => {
                    if let Some(n) = self.train.as_ref().map(|t| t.runs.len()) {
                        self.run = None;
                        self.teams[0].idx = 0;
                        if n >= DRILL_RUNS {
                            self.train_finish();
                        } else {
                            self.start_turn(1);
                        }
                    }
                }
                Act::TrainFinish => self.train_finish(),
                Act::DemoRestart(w) => {
                    if self.mode == Mode::Demo {
                        self.reset_game();
                        self.start_turn(1 - w);
                    }
                }
            }
        }
    }

    /// Advance everything by `dt` seconds. `ref_mouth` and `ref_tip` come from the renderer's rig.
    pub fn tick(&mut self, dt: f32, ref_mouth: Vec3, ref_tip: Vec3) {
        if self.mirror {
            self.mirror_tick(dt, ref_mouth, ref_tip);
            return;
        }
        self.clock += dt;
        self.game_time += dt;
        if let Some(tr) = self.train.as_mut() {
            if tr.clock && !tr.done {
                tr.t += dt;
            }
        }
        self.run_timers();
        self.phys_acc = (self.phys_acc + dt).min(0.1);
        while self.phys_acc >= STEP {
            self.phys_acc -= STEP;
            if self.ball_free {
                let r = self.step_ball(STEP, self.state == State::Flight);
                if r == "stop" {
                    self.ball_free = false;
                }
            }
            if self.step_bottle(STEP) && self.state == State::Flight {
                self.on_hit();
            }
        }
        if self.state == State::Flight {
            self.flight_t += dt;
            if (!self.ball_free && !self.bottle_falling) || self.flight_t > 8.0 {
                self.on_miss();
            }
            if self.trail.last().map_or(true, |p| p.distance(self.ball) > 0.12) {
                self.trail.push(self.ball);
                if self.trail.len() > 40 {
                    self.trail.remove(0);
                }
            }
        }
        if let Some((t, i)) = self.holder {
            if !self.ball_free {
                self.ball = self.teams[t].players[i].hand();
            }
        }
        if self.state == State::Run && self.run.is_some() {
            self.update_run(dt);
        }
        // everyone not running stands still, cheers, or walks off when done
        let over = self.state == State::Over;
        let win = self.win;
        for ti in 0..2 {
            for i in 0..3 {
                let running = self.state == State::Run && self.run.as_ref().map_or(false, |r| r.runners == ti && (r.ri == i || r.ri2 == Some(i)));
                if running {
                    continue;
                }
                let cheer = over && win == Some(ti);
                if self.teams[ti].out[i] {
                    let side = self.teams[ti].side;
                    let e = (-4.3 - i as f32 * 0.65, side * (LINE + 1.1));
                    let mut st = false;
                    let p = &mut self.teams[ti].players[i];
                    if Self::move_toward(p, e.0, e.1, 1.7, dt, &mut st) <= 0.02 {
                        Self::still(p, if cheer { AMode::Cheer } else { AMode::Idle });
                        p.yaw = (-p.pos.x).atan2(-p.pos.z);
                    }
                } else {
                    Self::still(&mut self.teams[ti].players[i], if cheer { AMode::Cheer } else { AMode::Idle });
                }
            }
        }
        self.watch_stop();
        for t in 0..2 {
            if self.ctrl(t) == Ctrl::Local {
                self.update_drink_ctl(t, dt);
            }
        }
        // sips: who lifts their can
        for ti in 0..2 {
            let c = self.ctrl(ti);
            for i in 0..3 {
                let mut s = false;
                if self.teams[ti].drinking {
                    s = if c == Ctrl::Local { self.teams[ti].dc.hold && self.teams[ti].dc.cough <= 0.0 } else { (self.clock * 2.2 + i as f32 * 0.8).sin() > -0.45 };
                }
                let running = self.run.as_ref().map_or(false, |r| r.runners == ti && (r.ri == i || r.ri2 == Some(i)));
                let sv = self.sneak_vis(ti, i);
                let beer = self.teams[ti].beers[i];
                let drinking = self.teams[ti].drinking;
                let hold = matches!(self.state, State::Aim | State::CpuAim) && self.turn == ti && i == self.teams[ti].idx && c == Ctrl::Cpu;
                let a = &mut self.teams[ti].players[i].anim;
                a.sip = (s || sv) && beer > 0.0 && !running;
                a.carry = a.sip || (drinking && beer > 0.0);
                a.hold = hold;
            }
        }
        // animation clocks
        for ti in 0..2 {
            for i in 0..3 {
                Self::anim_clock(&mut self.teams[ti].players[i].anim, dt);
            }
        }
        Self::anim_clock(&mut self.referee.anim, dt);
        self.sneak_tick(dt);
        self.update_ref(dt);
        self.update_smoke(dt, ref_mouth, ref_tip);
        for s in self.splash.iter_mut() {
            s.life -= dt;
            s.vel.y -= G * dt;
            s.pos += s.vel * dt;
            if s.pos.y < 0.01 {
                s.pos.y = 0.01;
                s.vel = Vec3::ZERO;
            }
        }
        self.splash.retain(|s| s.life > 0.0);
    }
    /// Animation clocks, smoke and splashes (also run on a guest's mirror).
    pub fn mirror_anim(&mut self, dt: f32, ref_mouth: Vec3, ref_tip: Vec3) {
        for ti in 0..2 {
            for i in 0..3 {
                Self::anim_clock(&mut self.teams[ti].players[i].anim, dt);
            }
        }
        Self::anim_clock(&mut self.referee.anim, dt);
        self.update_smoke(dt, ref_mouth, ref_tip);
        for s in self.splash.iter_mut() {
            s.life -= dt;
            s.vel.y -= G * dt;
            s.pos += s.vel * dt;
            if s.pos.y < 0.01 {
                s.pos.y = 0.01;
                s.vel = Vec3::ZERO;
            }
        }
        self.splash.retain(|s| s.life > 0.0);
    }
    fn anim_clock(a: &mut Anim, dt: f32) {
        if a.mode == AMode::Run {
            a.phase += dt * a.speed.max(1.2) * 2.7;
        } else {
            a.phase += dt * 1.0;
        }
        if a.throw_t >= 0.0 {
            a.throw_t += dt;
            if a.throw_t > crate::anim::THROW_LEN {
                a.throw_t = -1.0;
            }
        }
        a.cheer_t += dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A computer-vs-computer match has to end on its own.
    #[test]
    fn demo_match_finishes() {
        let mut g = Game::new(7);
        g.start_match(Mode::Demo, 1);
        let mut over = false;
        for _ in 0..(60 * 60 * 20) {
            g.tick(1.0 / 60.0, REF_POS + vec3(0.0, 1.5, 0.0), REF_POS + vec3(0.0, 1.3, 0.1));
            if g.state == State::Over {
                over = true;
                break;
            }
        }
        assert!(over, "no winner after 20 minutes; beers {:?} {:?}", g.teams[0].beers, g.teams[1].beers);
    }
    fn step(g: &mut Game, secs: f32) {
        for _ in 0..(secs * 60.0) as usize {
            g.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        }
    }
    fn perfect(g: &mut Game) {
        let t = g.turn;
        let o = g.teams[t].players[g.teams[t].idx].hand();
        g.launch(t, ThrowSpec { origin: o, yaw: (-o.x).atan2(-o.z), dist: (o.x * o.x + o.z * o.z).sqrt() - 0.3, curve: 0.0, ring: 3 });
    }
    #[test]
    fn drills_finish() {
        // throwing: ten perfect throws
        let mut g = Game::new(5);
        g.begin_train(Drill::Throw);
        for _ in 0..40 {
            if g.train_result.is_some() {
                break;
            }
            if g.local_aiming() {
                perfect(&mut g);
            }
            step(&mut g, 1.0);
        }
        let r = g.train_result.clone().expect("throw drill result");
        assert!(r.1 >= 9.0, "hits {}", r.1);
        // drinking: hold, breathe, hold again
        let mut g = Game::new(6);
        g.begin_train(Drill::Drink);
        for i in 0..400 {
            if g.train_result.is_some() {
                break;
            }
            if i % 5 == 0 {
                g.drink_press(0)
            }
            if i % 5 == 3 {
                g.drink_release(0)
            }
            step(&mut g, 0.1);
        }
        let r = g.train_result.clone().expect("drink drill result");
        assert!(r.1 > 3.0 && r.1 < 60.0, "drink time {}", r.1);
        // running: alternate feet every 0.15 s
        let mut g = Game::new(8);
        g.begin_train(Drill::Run);
        let mut f = Foot::L;
        for _ in 0..4000 {
            if g.train_result.is_some() {
                break;
            }
            if g.can_run(0) {
                g.pad_press(0, f);
                f = if f == Foot::L { Foot::R } else { Foot::L };
            }
            step(&mut g, 0.15);
        }
        let r = g.train_result.clone().expect("run drill result");
        assert!(r.1 > 3.0 && r.1 < 90.0, "run time {}", r.1);
    }
    #[test]
    fn tutorial_walks_through() {
        let mut g = Game::new(9);
        g.start_tutorial();
        assert_eq!(g.tut, Some(Tut::Intro));
        g.set_tut(Tut::Throw);
        let mut seen = vec![];
        for _ in 0..3000 {
            let t = g.tut.unwrap();
            if !seen.contains(&t) {
                seen.push(t);
            }
            if t == Tut::Ref {
                break;
            }
            if g.local_aiming() {
                perfect(&mut g);
            }
            if g.can_drink(0) {
                g.drink_press(0);
            } else {
                g.drink_release(0);
            }
            if g.can_run(0) {
                let f = if (g.clock * 7.0) as i32 % 2 == 0 { Foot::L } else { Foot::R };
                g.pad_press(0, f);
            }
            step(&mut g, 0.07);
        }
        assert!(seen.contains(&Tut::Drink) && seen.contains(&Tut::Run) && seen.contains(&Tut::Ref), "{seen:?}");
    }
    #[test]
    fn strafbier_comes_from_the_crate_to_the_floor() {
        let mut g = Game::new(4);
        g.start_match(Mode::Cpu, 1);
        g.judge(0, Foul::Can);
        assert_eq!(g.teams[0].straf, 1);
        assert_eq!(g.ref_bottle, Some((0, 0)), "not at their feet yet");
        assert!(!g.ref_hold, "he has yet to take it");
        let (mut held, mut arrived_near) = (false, false);
        for _ in 0..(60 * 30) {
            g.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
            held |= g.ref_hold;
            if g.ref_bottle.is_none() && !arrived_near {
                arrived_near = true;
                assert!(held, "put down without having been in his hand");
                let cp = g.teams[0].can_pos(0, 1);
                let d = ((g.referee.pos.x - cp[0]).powi(2) + (g.referee.pos.z - cp[1]).powi(2)).sqrt();
                assert!(d < 0.7, "{d} m from where the bottle goes");
            }
            if arrived_near && g.referee.anim.mode == AMode::Sit && g.referee.pos.distance(REF_POS) < 0.01 {
                assert!(!g.ref_hold);
                return;
            }
        }
        panic!("the referee never got back to his crate; delivered: {arrived_near}");
    }
    #[test]
    fn emotes_change_nothing_and_end() {
        use crate::anim::{animate, EMOTES};
        for i in 0..EMOTES.len() {
            let mut p = Person::new(Vec3::ZERO, 0.0);
            p.anim.emote = i as u8 + 1;
            let mut t = 0.0;
            while p.anim.emote > 0 {
                animate(&mut p, 1.0 / 60.0, None, 0.1);
                let o = p.vis.pose;
                assert!(o.hip.is_finite() && o.lean.is_finite() && o.arm.iter().flatten().all(|x| x.is_finite()), "{}", EMOTES[i].0);
                t += 1.0 / 60.0;
                assert!(t < 10.0, "{} doesn't end", EMOTES[i].0);
            }
            assert!(t + 0.1 > EMOTES[i].2);
        }
        // the match wanting them cuts it short
        let mut p = Person::new(Vec3::ZERO, 0.0);
        p.anim.emote = 3;
        animate(&mut p, 0.1, None, 0.1);
        p.anim.mode = AMode::Run;
        animate(&mut p, 0.1, None, 0.1);
        assert_eq!(p.anim.emote, 0);
    }
    #[test]
    fn straight_throw_hits() {
        let mut g = Game::new(3);
        g.start_match(Mode::Cpu, 1);
        let o = g.teams[0].players[0].hand();
        let yaw = (-o.x).atan2(-o.z);
        let dist = (o.x * o.x + o.z * o.z).sqrt() - 0.3;
        g.launch(0, ThrowSpec { origin: o, yaw, dist, curve: 0.0, ring: 2 });
        for _ in 0..600 {
            g.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
            if g.state == State::Run {
                break;
            }
        }
        assert_eq!(g.state, State::Run);
    }
}
