//! Online play. The server runs every match (the same `Game` as offline) and owns its state; each
//! screen is a mirror that draws ~20 full snapshots a second, moves things smoothly in between and
//! predicts its own taps and throws until the server has acknowledged them.

use crate::game::{AMode, Ev, Foot, Game, Mode, Sfx, State, StatEv, ThrowSpec, BR, G, KD, CURVE_A};
use glam::{vec3, Vec3};
use serde::{Deserialize, Serialize};

/// Bumped whenever the messages change; a mismatch is told to the player instead of misbehaving.
pub const PROTOCOL: u32 = 9;
pub const SNAP_HZ: f32 = 20.0;
pub const MAX_PEOPLE: usize = 6;
pub const ARENAS: [&str; 6] = ["vondel", "evening", "roof", "cafe", "club", "krakau"];

/// Where the ball bounces back in an arena.
pub fn arena_bounds(id: &str) -> Option<(f32, f32)> {
    match id {
        "roof" => Some((11.6, 14.6)),
        "gym" => Some((9.2, 13.2)),
        // both are tight rooms round the pitch
        "cafe" => Some((6.7, 8.7)),
        "club" => Some((4.55, 7.65)), // the crowd barriers
        // the parked cars on both sides, the hostel and the fence behind the lines
        "krakau" => Some((8.2, 14.4)),
        _ => None,
    }
}

// ---------- messages ----------
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum ToServer {
    /// `me`: friend code and secret, if there is an account, so the match counts for the stats with friends.
    Host { v: u32, name: String, look: [u8; 7], me: (String, String) },
    Join { v: u32, code: String, name: String, look: [u8; 7], me: (String, String) },
    /// Back after a lost connection: take the same seat again.
    Resume { v: u32, code: String, token: String },
    List,
    Open { open: bool },
    Config { arena: String, diff: u8 },
    Team { team: u8 },
    Start,
    /// Something done in the match; `n` counts up and comes back as the snapshot's `a`.
    Input { n: u32, i: Input },
    Qm { i: u8 },
    /// An animation for everyone to see. Nothing in the match changes.
    Emote { i: u8 },
    Leave,
    /// Once a second: `c` is echoed for the round trip, `r` is the last one measured, `s`/`d` say
    /// which snapshot was last applied and what the state looked like after it.
    Hb { c: f64, r: f32, s: u32, d: u32 },
    // ---------- friends: on a connection of its own, open as long as the game runs ----------
    /// Log in, or with no code yet, get an account. `only`: never a new account; a code and secret the
    /// server doesn't know get `AuthErr` instead (the web version, which logs in with a password).
    SHello {
        v: u32,
        code: String,
        secret: String,
        name: String,
        #[serde(default)]
        only: bool,
    },
    /// A new account with a username and password (the web version).
    Register { v: u32, user: String, pass: String, name: String },
    /// Log in with a username and password: the answer is `Me`, as after `SHello`, or `AuthErr`.
    Login { v: u32, user: String, pass: String, name: String },
    /// Your progression (XP, records, look), kept on the server for an account with a password.
    /// `data` is the game's own; the server only stores it. `rev` counts up with every change.
    SProgress { rev: u64, data: String },
    SName { name: String },
    /// What you're up to, for your friends: see `STATUS_*`.
    SStatus { status: u8 },
    /// Send a request, or accept one by sending one back.
    FriendAdd { code: String },
    /// Decline a request, take one back, or end a friendship.
    FriendRemove { code: String },
    Say { to: String, text: String },
    /// Open a chat: returns recent messages and marks them read.
    ChatOpen { with: String },
    Invite { to: String, room: String },
    /// What you two have played, the server keeps count.
    FriendStats { with: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum Input {
    Throw { o: [f32; 3], yaw: f32, dist: f32, curve: f32, ring: u8 },
    /// `at` is the tap's time on the player's device, so network hiccups don't look like an autoclicker.
    Drink { hold: bool, at: f64 },
    Pad { foot: u8, at: f64 },
    Sneak,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum FromServer {
    Welcome { code: String, id: u32, token: String, resumed: bool },
    Roster { members: Vec<Member>, host: u32, arena: String, diff: u8, phase: u8, open: bool },
    /// `a`: the last input of yours this snapshot includes.
    Snap { a: u32, s: Box<Snap> },
    /// Everyone's connection, once a second.
    Status { l: Vec<(u32, u16, u8)> },
    /// Something that went wrong or changed, in words, for right now.
    Notice { title: String, sub: String },
    /// An input the server turned down.
    Reject { n: u32, why: String },
    Qm { i: u8, from: u32 },
    Emote { i: u8, from: u32 },
    Rooms { rooms: Vec<RoomInfo> },
    Pong { c: f64 },
    Err { msg: String, fatal: bool },
    Closed { why: String },
    // ---------- friends ----------
    /// `user`: the username, for an account with a password (filed in lower case; `name` is it as typed,
    /// and that account's name for good).
    Me {
        code: String,
        secret: String,
        name: String,
        #[serde(default)]
        user: String,
    },
    /// After `Me`, for an account with a password: the progression kept for it (`data` empty if none yet).
    Progress { rev: u64, data: String },
    /// Login, sign-up or a kept session that didn't work; the session is over.
    AuthErr { msg: String },
    Friends { friends: Vec<Friend>, requests: Vec<Friend>, sent: Vec<Friend> },
    Said { with: String, m: ChatMsg },
    History { with: String, msgs: Vec<ChatMsg> },
    Invited { code: String, name: String, room: String },
    FriendStats { with: String, s: PairStats },
    /// Not fatal, for the friends screen.
    SocialErr { msg: String },
}

pub const STATUS_FREE: u8 = 0;
pub const STATUS_LOBBY: u8 = 1;
pub const STATUS_MATCH: u8 = 2;
pub const CODE_LEN: usize = 8;
pub const MAX_CHAT: usize = 300;
/// The longest `SProgress` the server keeps.
pub const MAX_PROGRESS: usize = 96 * 1024;
pub const USER_MIN: usize = 3;
/// as long as a player's name: the username is the name
pub const USER_MAX: usize = 16;
pub const PASS_MIN: usize = 8;
pub const PASS_MAX: usize = 128;

/// A username as the server files it: lower case, letters, digits, `.`, `_` and `-`.
pub fn clean_user(u: &str) -> Result<String, String> {
    let u = u.trim().to_lowercase();
    if u.chars().count() < USER_MIN || u.chars().count() > USER_MAX {
        return Err(format!("A username has {USER_MIN} to {USER_MAX} characters"));
    }
    if !u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err("A username has only letters, digits, . _ and -".into());
    }
    Ok(u)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Friend {
    pub code: String,
    pub name: String,
    pub online: bool,
    pub status: u8,
    pub unread: u32,
}

/// Your record with one friend, seen from your side. Hits and throws are the two of you, not your teams.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PairStats {
    pub together: u32,
    pub together_wins: u32,
    pub against: u32,
    pub my_wins: u32,
    pub their_wins: u32,
    pub my_throws: u32,
    pub my_hits: u32,
    pub their_throws: u32,
    pub their_hits: u32,
    pub seconds: u32,
    /// the latest first
    pub recent: Vec<PairMatch>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PairMatch {
    pub at: u64,
    pub arena: String,
    pub together: bool,
    pub won: bool,
    pub my_hits: u32,
    pub their_hits: u32,
    pub seconds: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatMsg {
    pub id: u64,
    /// the sender's friend code
    pub from: String,
    pub text: String,
    /// unix seconds
    pub at: u64,
}

pub const PHASE_LOBBY: u8 = 0;
pub const PHASE_PLAYING: u8 = 1;
pub const PHASE_OVER: u8 = 2;
pub const CONN_GOOD: u8 = 0;
pub const CONN_SLOW: u8 = 1;
pub const CONN_AWAY: u8 = 2;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RoomInfo {
    pub code: String,
    pub name: String,
    pub n: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Member {
    pub id: u32,
    pub name: String,
    pub team: u8,
    pub look: [u8; 7], // shirt, skin, hair, shorts, voice, hat, level
    /// connection gone; their seat is kept for them
    pub away: bool,
}

pub fn team_of(ms: &[Member], t: u8) -> Vec<&Member> {
    ms.iter().filter(|m| m.team == t).collect()
}
/// Who plays player `i` of team `t`: person k plays player k; a short team shares the spares, and
/// someone who dropped out is covered by whoever is still there. Nobody there: the computer.
pub fn owner_of(ms: &[Member], t: usize, i: usize) -> Option<&Member> {
    let hs: Vec<&Member> = ms.iter().filter(|m| m.team as usize == t && !m.away).collect();
    if hs.is_empty() {
        None
    } else {
        Some(hs[i % hs.len()])
    }
}
/// Set a game's sides from the roster. `me` is None on the server, which is on nobody's side.
pub fn set_sides(g: &mut Game, ms: &[Member], me: Option<u32>) {
    g.mode = Mode::Online;
    for t in 0..2 {
        g.humans[t] = ms.iter().filter(|m| m.team as usize == t && !m.away).count() as u8;
    }
    g.my_team = me.and_then(|id| ms.iter().find(|m| m.id == id)).map(|m| m.team as usize);
    for t in 0..2 {
        for i in 0..3 {
            g.my_players[t][i] = me.is_some() && owner_of(ms, t, i).map(|m| m.id) == me;
        }
    }
}

// ---------- snapshots ----------
/// Everything a screen needs to draw the match. Always complete, so any snapshot repairs a screen.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Snap {
    pub m: u32, // which match
    pub q: u32, // snapshot number
    pub st: u8,
    pub tu: u8,
    pub hi: i8,
    pub wi: i8,
    pub be: [f32; 6],
    pub ou: [bool; 6],
    pub sf: [u32; 2],
    pub ht: [u32; 2],
    pub th: [u32; 2],
    pub ix: [u8; 2],
    pub pl: Vec<[f32; 8]>, // 6 players + referee: x, z, yaw, mode, phase, flags(sip|hold<<1|point<<2|smoking<<3|reach<<4), throw_t, look
    pub rs: f32,           // referee smoke clock
    pub ba: [f32; 6],      // ball position, velocity
    pub bh: i8,
    pub bf: bool,
    pub bc: f32,
    pub bo: [f32; 3], // bottle: theta, tip x, tip z
    pub gz: [f32; 4],
    pub dc: [[f32; 6]; 2], // drink: gauge, cough, stop_show, hold, hold time, breath rate
    pub rc: [[f32; 3]; 2], // run pads: last foot, lock, early
    pub kn: Vec<[f32; 5]>,
    pub sn: [f32; 2],
    pub rp: bool, // runners still out
    pub status: String,
    pub pz: f32, // match paused: seconds until it goes on anyway (below 0: not paused)
    pub pw: u32, // …waiting for this person
    pub al: f32, // seconds until an idle thrower's ball is thrown for them (below 0: no clock)
    pub rb: i8,  // the Strafbier on its way: team * 3 + player (below 0: none)
    pub rh: bool, // …in the referee's hand
    pub dg: u32, // digest of the state, see `Game::digest`
    pub ev: Vec<(u32, Ev)>,
    pub sx: Vec<(u32, Sfx)>,
}

fn st_code(s: State) -> u8 {
    match s {
        State::Menu => 0,
        State::Aim => 1,
        State::CpuAim => 2,
        State::Flight => 3,
        State::Run => 4,
        State::Stopped => 5,
        State::Over => 6,
    }
}
fn st_of(c: u8) -> State {
    [State::Menu, State::Aim, State::CpuAim, State::Flight, State::Run, State::Stopped, State::Over][c.min(6) as usize]
}
fn mode_code(m: AMode) -> f32 {
    match m {
        AMode::Idle => 0.0,
        AMode::Run => 1.0,
        AMode::Bend => 2.0,
        AMode::Cheer => 3.0,
        AMode::Sit => 4.0,
    }
}
fn mode_of(c: f32) -> AMode {
    [AMode::Idle, AMode::Run, AMode::Bend, AMode::Cheer, AMode::Sit][(c as usize).min(4)]
}
/// Millimetres are plenty, and keep the JSON short.
fn q(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

/// The server keeps recent events and sounds, numbered, so a slow screen that skipped snapshots still
/// gets them and notices when it has missed some. Each is kept for a limited time.
#[derive(Default)]
pub struct EvLog {
    pub ev: Vec<(u32, Ev)>,
    pub sx: Vec<(u32, Sfx)>,
    ev_t: Vec<f32>,
    sx_t: Vec<f32>,
    ev_seq: u32,
    sx_seq: u32,
}
impl EvLog {
    pub fn push_ev(&mut self, e: Ev, now: f32) {
        self.ev_seq += 1;
        self.ev.push((self.ev_seq, e));
        self.ev_t.push(now);
    }
    pub fn push_sx(&mut self, s: Sfx, now: f32) {
        self.sx_seq += 1;
        self.sx.push((self.sx_seq, s));
        self.sx_t.push(now);
    }
    pub fn prune(&mut self, now: f32) {
        while self.ev_t.first().map_or(false, |t| now - *t > 5.0) {
            self.ev_t.remove(0);
            self.ev.remove(0);
        }
        while self.sx_t.first().map_or(false, |t| now - *t > 0.6) {
            self.sx_t.remove(0);
            self.sx.remove(0);
        }
    }
}

/// What a screen has predicted and the server has not confirmed yet (see `apply_snapshot`).
#[derive(Clone, Default)]
pub struct Pred {
    /// last inputs sent, by kind
    pub throw_n: u32,
    pub hold_n: u32,
    pub pad_n: u32,
    pub sent_n: u32,
    /// how long ago a predicted throw was made; a throw nobody confirms is dropped
    pub throw_age: f32,
    /// measured round trip: how far this screen's own actions run ahead of the snapshots
    pub rtt: f32,
    /// how far ahead of the server's the ball on this screen is (our own throw)
    pub lead: f32,
    /// sounds already played for a prediction, not to be played again when the server's come in
    pub skip_step: u32,
    pub skip_throw: u32,
    pub last_ev: u32,
    pub last_sx: u32,
    pub last_q: u32,
    pub match_id: u32,
    /// events the server no longer had by the time a snapshot got through
    pub missed_ev: u32,
    pub skipped_snaps: u32,
    pub digest: u32,
    pub snap_age: f32,
}

/// One step of the ball's flight, the same as `Game::step_ball` without anything to hit.
fn fly(p: &mut Vec3, v: &mut Vec3, curve: f32, h: f32) {
    if p.y > BR + 0.004 {
        let sp = v.length();
        v.x -= KD * sp * v.x * h;
        v.y -= (G + KD * sp * v.y) * h;
        v.z -= KD * sp * v.z * h;
        if curve != 0.0 {
            let hs = (v.x * v.x + v.z * v.z).sqrt().max(1e-6);
            let (vx, vz) = (v.x, v.z);
            v.x += CURVE_A * curve * (vz / hs) * h;
            v.z -= CURVE_A * curve * (vx / hs) * h;
        }
    } else {
        v.y -= G * h;
    }
    *p += *v * h;
    if p.y < BR {
        p.y = BR;
        if v.y < -0.9 {
            v.y = -v.y * 0.42;
            v.x *= 0.72;
            v.z *= 0.72;
        } else {
            v.y = 0.0;
            let sp = (v.x * v.x + v.z * v.z).sqrt();
            if sp > 0.0 {
                let ns = (sp - (1.5 + 0.9 * sp) * h).max(0.0);
                v.x *= ns / sp;
                v.z *= ns / sp;
            }
        }
    }
}

impl Game {
    pub fn snapshot(&self, m: u32, seq: u32, log: &EvLog) -> Snap {
        let mut pl = Vec::with_capacity(7);
        let pack = |p: &crate::game::Person| {
            let a = &p.anim;
            let flags = (a.sip as u8) | (a.hold as u8) << 1 | (a.point as u8) << 2 | (a.smoking as u8) << 3 | (a.reach as u8) << 4;
            [q(p.pos.x), q(p.pos.z), q(p.yaw), mode_code(a.mode), q(a.phase), flags as f32, q(a.throw_t), q(a.look)]
        };
        for t in &self.teams {
            for p in &t.players {
                pl.push(pack(p));
            }
        }
        pl.push(pack(&self.referee));
        let mut be = [0.0; 6];
        let mut ou = [false; 6];
        for t in 0..2 {
            for i in 0..3 {
                be[t * 3 + i] = q(self.teams[t].beers[i]);
                ou[t * 3 + i] = self.teams[t].out[i];
            }
        }
        let dc = [0, 1].map(|t| {
            let d = &self.teams[t].dc;
            [q(d.p), q(d.cough), q(d.stop_show), d.hold as u8 as f32, q(d.ht), q(d.mul)]
        });
        let rc = [0, 1].map(|t| {
            let r = &self.teams[t].rc;
            [match r.last {
                None => 0.0,
                Some(Foot::L) => 1.0,
                Some(Foot::R) => 2.0,
            }, q(r.lock), self.teams[t].early]
        });
        Snap {
            m,
            q: seq,
            st: st_code(self.state),
            tu: self.turn as u8,
            hi: self.hitter.map_or(-1, |h| h as i8),
            wi: self.win.map_or(-1, |w| w as i8),
            be,
            ou,
            sf: [self.teams[0].straf, self.teams[1].straf],
            ht: [self.teams[0].hits, self.teams[1].hits],
            th: [self.teams[0].throws, self.teams[1].throws],
            ix: [self.teams[0].idx as u8, self.teams[1].idx as u8],
            pl,
            rs: q(self.referee.anim.smoke_t),
            ba: [q(self.ball.x), q(self.ball.y), q(self.ball.z), q(self.ball_vel.x), q(self.ball_vel.y), q(self.ball_vel.z)],
            bh: self.holder.map_or(-1, |(t, i)| (t * 3 + i) as i8),
            bf: self.ball_free,
            bc: self.ball_curve,
            bo: [q(self.theta), self.tip_dir.x, self.tip_dir.z],
            gz: [q(self.gaze_col[0]), q(self.gaze_col[1]), q(self.gaze_col[2]), q(self.gaze_op)],
            dc,
            rc,
            kn: self.knocked.iter().map(|((t, i, k), d)| [*t as f32, *i as f32, *k as f32, d[0], d[1]]).collect(),
            sn: [0, 1].map(|t| self.sneak_progress_team(t).map_or(-1.0, q)),
            rp: self.run_phase_active(),
            status: self.status_text_for(None),
            pz: -1.0,
            pw: 0,
            al: -1.0,
            rb: self.ref_bottle.map_or(-1, |(t, i)| (t * 3 + i) as i8),
            rh: self.ref_hold,
            dg: self.digest(),
            ev: log.ev.clone(),
            sx: log.sx.clone(),
        }
    }

    /// A fingerprint of what decides the match. The server compares a screen's fingerprint for a
    /// snapshot with its own, which catches a screen that isn't showing the real match.
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut add = |v: i32| {
            for b in v.to_le_bytes() {
                h = (h ^ b as u32).wrapping_mul(0x0100_0193);
            }
        };
        add(st_code(self.state) as i32);
        add(self.turn as i32);
        add(self.hitter.map_or(-1, |x| x as i32));
        add(self.win.map_or(-1, |x| x as i32));
        add(self.holder.map_or(-1, |(t, i)| (t * 3 + i) as i32));
        for t in &self.teams {
            for i in 0..3 {
                add((t.beers[i] * 1000.0).round() as i32);
                add(t.out[i] as i32);
            }
            add(t.straf as i32);
            add(t.hits as i32);
            add(t.throws as i32);
            add(t.idx as i32);
        }
        h
    }

    /// Mirror: take a snapshot from the server. `ack` is the last of our inputs it includes; whatever
    /// we predicted beyond that stays on screen. Returns the new sounds; events are worded here.
    pub fn apply_snapshot(&mut self, s: &Snap, ack: u32) -> Vec<Sfx> {
        self.mirror = true;
        self.mode = Mode::Online;
        let prev = self.state;
        if s.q != self.pred.last_q.wrapping_add(1) && self.pred.last_q != 0 && s.q > self.pred.last_q {
            self.pred.skipped_snaps += s.q - self.pred.last_q - 1;
        }
        self.pred.last_q = s.q;
        self.pred.snap_age = 0.0;
        // our own predictions, kept through this snapshot if the server hasn't seen them yet
        let holding = ack < self.pred.hold_n;
        let padding = ack < self.pred.pad_n;
        let throwing = ack < self.pred.throw_n && self.pred.throw_age < 2.0;
        let kept_dc: [_; 2] = [self.teams[0].dc.clone(), self.teams[1].dc.clone()];
        let kept_last = [self.teams[0].rc.last, self.teams[1].rc.last];
        let (kept_ball, kept_target) = (self.ball, self.ball_target);
        let kept_curve = self.ball_curve;

        self.state = st_of(s.st);
        self.turn = s.tu as usize & 1;
        self.hitter = (s.hi >= 0).then_some(s.hi as usize & 1);
        self.win = (s.wi >= 0).then_some(s.wi as usize & 1);
        for t in 0..2 {
            for i in 0..3 {
                self.teams[t].beers[i] = s.be[t * 3 + i];
                self.teams[t].out[i] = s.ou[t * 3 + i];
            }
            self.teams[t].straf = s.sf[t];
            self.teams[t].hits = s.ht[t];
            self.teams[t].throws = s.th[t];
            self.teams[t].idx = s.ix[t] as usize % 3;
            let d = &mut self.teams[t].dc;
            d.cough = s.dc[t][1];
            d.stop_show = s.dc[t][2];
            d.hold = s.dc[t][3] > 0.5;
            d.ht = s.dc[t][4];
            d.mul = s.dc[t][5];
            // the gauge on your own screen runs a round trip ahead of what the server reports
            let lead = if self.my_team == Some(t) && d.hold { self.pred.rtt } else { 0.0 };
            d.p = s.dc[t][0] + lead * (0.35 + d.ht * 0.28) * d.mul;
            let r = &mut self.teams[t].rc;
            r.last = match s.rc[t][0] as u8 {
                1 => Some(Foot::L),
                2 => Some(Foot::R),
                _ => None,
            };
            r.lock = s.rc[t][1];
            self.teams[t].early = s.rc[t][2];
            self.teams[t].drinking = self.hitter == Some(t) && self.state == State::Run;
        }
        let dt_snap = 1.0 / SNAP_HZ;
        let unpack = |p: &mut crate::game::Person, v: &[f32; 8]| {
            let tg = vec3(v[0], 0.0, v[1]);
            let was = p.net_target.unwrap_or(tg);
            // keep moving at this pace until the next snapshot comes in
            p.net_vel = if tg.distance(was) > 1.5 { Vec3::ZERO } else { (tg - p.net_from) / dt_snap };
            p.net_from = tg;
            p.net_target = Some(tg);
            p.yaw = v[2];
            let a = &mut p.anim;
            a.mode = mode_of(v[3]);
            if (a.phase - v[4]).abs() > 1.0 {
                a.phase = v[4];
            }
            let f = v[5] as u8;
            a.sip = f & 1 != 0;
            a.hold = f & 2 != 0;
            a.point = f & 4 != 0;
            a.smoking = f & 8 != 0;
            a.reach = f & 16 != 0;
            if v[6] >= 0.0 && a.throw_t < 0.0 {
                a.throw_t = v[6];
            }
        };
        for (k, v) in s.pl.iter().enumerate().take(7) {
            if k < 6 {
                unpack(&mut self.teams[k / 3].players[k % 3], v);
                self.teams[k / 3].players[k % 3].anim.look = v[7];
            } else {
                unpack(&mut self.referee, v);
                self.net_look = v[7];
            }
        }
        for t in 0..2 {
            let drinking = self.teams[t].drinking;
            for i in 0..3 {
                let beer = self.teams[t].beers[i];
                let a = &mut self.teams[t].players[i].anim;
                a.carry = a.sip || (drinking && beer > 0.0);
            }
        }
        self.referee.anim.smoke_t = s.rs;
        // the ball on our screen is ahead of the server's while it is our own throw
        let (mut bp, mut bv) = (vec3(s.ba[0], s.ba[1], s.ba[2]), vec3(s.ba[3], s.ba[4], s.ba[5]));
        if s.bf && self.pred.lead > 0.0 {
            let mut left = self.pred.lead;
            while left > 0.0 {
                let h = left.min(1.0 / 120.0);
                fly(&mut bp, &mut bv, s.bc, h);
                left -= h;
            }
        }
        self.ball_target = Some((bp, bv));
        self.holder = (s.bh >= 0).then_some(((s.bh / 3) as usize & 1, (s.bh % 3) as usize));
        self.ball_free = s.bf;
        self.ball_curve = s.bc;
        self.net_theta = s.bo[0];
        self.tip_dir = vec3(s.bo[1], 0.0, s.bo[2]);
        self.gaze_col = [s.gz[0], s.gz[1], s.gz[2]];
        self.gaze_op = s.gz[3];
        self.knocked = s.kn.iter().map(|v| ((v[0] as usize & 1, v[1] as usize % 3, v[2] as usize % 4), [v[3], v[4]])).collect();
        self.net_sneak = s.sn;
        self.net_run = s.rp;
        self.net_status = s.status.clone();
        self.net_pause = (s.pz >= 0.0).then_some((s.pw, s.pz));
        self.net_aim_left = s.al;
        self.ref_bottle = (s.rb >= 0).then_some(((s.rb / 3) as usize & 1, (s.rb % 3) as usize));
        self.ref_hold = s.rh;
        self.pred.digest = self.digest();

        // and now put back what we know better than this snapshot does
        if self.state == State::Aim && self.my_players[self.turn][self.teams[self.turn].idx] {
            self.holder = None; // the ball is in your hand on the screen, see `aim_ball`
        }
        if let Some(t) = self.my_team {
            if holding {
                self.teams[t].dc.hold = kept_dc[t].hold;
                self.teams[t].dc.ht = kept_dc[t].ht;
                self.teams[t].dc.p = kept_dc[t].p;
            }
            if padding {
                self.teams[t].rc.last = kept_last[t];
            }
        }
        if throwing && self.state == State::Aim {
            let t = self.turn;
            self.teams[t].idx = (self.teams[t].idx + 1) % 3;
            self.teams[t].throws += 1;
            self.state = State::Flight;
            self.holder = None;
            self.ball_free = true;
            self.ball = kept_ball;
            self.ball_target = kept_target;
            self.ball_curve = kept_curve;
        }
        if self.state != State::Flight {
            self.pred.lead = 0.0;
        }
        if self.state != State::Run {
            self.pred.skip_step = 0;
        }
        if prev != State::Flight && self.state == State::Flight {
            self.trail.clear();
        }
        // events and sounds we haven't had yet
        if let Some((first, _)) = s.ev.first() {
            if *first > self.pred.last_ev + 1 {
                self.pred.missed_ev += *first - self.pred.last_ev - 1;
            }
        }
        let evs: Vec<Ev> = s.ev.iter().filter(|(n, _)| *n > self.pred.last_ev).map(|(_, e)| *e).collect();
        let sxs: Vec<Sfx> = s.sx.iter().filter(|(n, _)| *n > self.pred.last_sx).map(|(_, e)| *e).collect();
        self.pred.last_ev = s.ev.last().map_or(self.pred.last_ev, |x| x.0.max(self.pred.last_ev));
        self.pred.last_sx = s.sx.last().map_or(self.pred.last_sx, |x| x.0.max(self.pred.last_sx));
        for e in evs {
            self.mirror_event(e);
        }
        let mut out = Vec::with_capacity(sxs.len());
        for x in sxs {
            match x {
                Sfx::Step if self.pred.skip_step > 0 => self.pred.skip_step -= 1,
                Sfx::Throw if self.pred.skip_throw > 0 => self.pred.skip_throw -= 1,
                Sfx::CanHit => {
                    self.splash_at(self.ball, 14);
                    out.push(x);
                }
                _ => out.push(x),
            }
        }
        out
    }

    fn mirror_event(&mut self, e: Ev) {
        self.word(e);
        // the stats page and the little effects that only exist on a screen
        match e {
            Ev::Hit(team, ring, curve) => self.stat_ev.push(StatEv::Hit { team, ring, curve }),
            Ev::Stop => self.stat_ev.push(StatEv::Stop),
            Ev::Unseen(team, _) => self.stat_ev.push(StatEv::Unseen { team }),
            Ev::SneakOk(team) => self.stat_ev.push(StatEv::Sneak { team }),
            Ev::Straf(team, foul) => {
                self.stat_ev.push(StatEv::Straf { team });
                if foul == crate::game::Foul::Spill {
                    let p = (0..3).find(|k| self.teams[team].beers[*k] > 0.0).unwrap_or(0);
                    let hp = self.teams[team].players[p].hand();
                    self.splash_at(hp, 18);
                }
            }
            _ => {}
        }
    }

    /// Mirror: show our own throw right away. The server's snapshots take over once it has it.
    pub fn predict_throw(&mut self, spec: ThrowSpec, n: u32) {
        let t = self.turn;
        self.launch(t, spec);
        self.ball_target = Some((self.ball, self.ball_vel));
        self.pred.throw_n = n;
        self.pred.throw_age = 0.0;
        self.pred.skip_throw += 1;
        self.pred.lead = self.pred.rtt;
    }

    /// Mirror: start showing a new match.
    pub fn mirror_begin(&mut self, m: u32) {
        self.reset_game();
        self.mirror = true;
        self.mode = Mode::Online;
        let rtt = self.pred.rtt;
        self.pred = Pred { match_id: m, rtt, ..Pred::default() };
        self.stat_ev.clear();
    }

    /// Mirror: move things smoothly toward the last snapshot.
    pub fn mirror_tick(&mut self, dt: f32, ref_mouth: Vec3, ref_tip: Vec3) {
        self.clock += dt;
        self.game_time += dt;
        self.pred.snap_age += dt;
        self.pred.throw_age += dt;
        let k = 1.0 - (-14.0 * dt).exp();
        // carry on for a moment if a snapshot is late, but don't run off when the server is gone
        let on = self.pred.snap_age < 0.2 && self.net_pause.is_none();
        let ease = |p: &mut crate::game::Person| {
            if let Some(tg) = p.net_target.as_mut() {
                if on {
                    *tg += p.net_vel * dt;
                }
                let tg = *tg;
                if p.pos.distance(tg) > 3.0 {
                    p.pos = tg;
                } else {
                    p.pos = p.pos.lerp(tg, k);
                }
            }
        };
        for t in 0..2 {
            for i in 0..3 {
                ease(&mut self.teams[t].players[i]);
            }
        }
        ease(&mut self.referee);
        self.theta += (self.net_theta - self.theta) * (1.0 - (-30.0 * dt).exp());
        self.referee.anim.look += (self.net_look - self.referee.anim.look) * (1.0 - (-18.0 * dt).exp());
        let curve = self.ball_curve;
        if let Some((bp, bv)) = self.ball_target.as_mut() {
            if self.ball_free && (on || self.pred.snap_age < 0.5) {
                // fly on between snapshots with the real physics, then settle onto the server's ball
                let mut left = dt;
                while left > 0.0 {
                    let h = left.min(1.0 / 120.0);
                    fly(bp, bv, curve, h);
                    left -= h;
                }
            }
            let (tgt, vel) = (*bp, *bv);
            self.ball_vel = vel;
            let was = self.ball;
            self.ball = if self.ball.distance(tgt) > 2.0 { tgt } else { self.ball.lerp(tgt, 1.0 - (-20.0 * dt).exp()) };
            if self.ball_free {
                let d = self.ball - was;
                let sp = (d.x * d.x + d.z * d.z).sqrt();
                if sp > 1e-4 {
                    let ax = vec3(d.z, 0.0, -d.x).normalize();
                    self.ball_rot = glam::Quat::from_axis_angle(ax, sp / BR) * self.ball_rot;
                }
            }
        }
        if let Some((t, i)) = self.holder {
            if !self.ball_free {
                self.ball = self.teams[t].players[i].hand();
            }
        }
        if self.state == State::Flight && self.trail.last().map_or(true, |p| p.distance(self.ball) > 0.12) {
            self.trail.push(self.ball);
            if self.trail.len() > 40 {
                self.trail.remove(0);
            }
        }
        for t in 0..2 {
            let can = self.state == State::Run && self.hitter == Some(t);
            let d = &mut self.teams[t].dc;
            if d.cough > 0.0 {
                d.cough -= dt;
                d.p = (d.p - dt * 0.5).max(0.0);
            } else if d.hold && can {
                d.ht += dt;
                d.p = (d.p + dt * (0.35 + d.ht * 0.28) * d.mul.max(0.8)).min(1.0);
            } else {
                d.p = (d.p - dt * 0.9).max(0.0);
            }
            if d.stop_show > 0.0 {
                d.stop_show -= dt;
            }
            let rc = &mut self.teams[t].rc;
            for d in rc.down.iter_mut() {
                *d -= dt;
            }
            if rc.msg_t > 0.0 {
                rc.msg_t -= dt;
            }
            if rc.lock > 0.0 {
                rc.lock -= dt;
            }
        }
        self.mirror_anim(dt, ref_mouth, ref_tip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Foul, ThrowSpec};

    fn two() -> Vec<Member> {
        vec![Member { id: 1, name: "A".into(), team: 0, look: [0; 7], away: false }, Member { id: 2, name: "B".into(), team: 1, look: [0; 7], away: false }]
    }
    fn server() -> (Game, EvLog) {
        let mut g = Game::new(11);
        g.start_match(Mode::Online, 1);
        set_sides(&mut g, &two(), None);
        g.restart_online();
        (g, EvLog::default())
    }
    fn screen(me: u32) -> Game {
        let mut g = Game::new(1);
        g.mirror_begin(1);
        set_sides(&mut g, &two(), Some(me));
        g
    }
    fn perfect(g: &Game) -> ThrowSpec {
        let o = g.teams[0].players[0].hand();
        ThrowSpec { origin: o, yaw: (-o.x).atan2(-o.z), dist: (o.x * o.x + o.z * o.z).sqrt() - 0.3, curve: 0.0, ring: 2 }
    }
    fn wire(s: &Snap) -> Snap {
        serde_json::from_str(&serde_json::to_string(s).unwrap()).unwrap()
    }

    #[test]
    fn digest_survives_the_wire() {
        let (mut srv, log) = server();
        let mut m = screen(2);
        for q in 1..600 {
            srv.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
            if q == 30 {
                srv.launch(0, perfect(&srv));
            }
            if q % 3 == 0 {
                let s = wire(&srv.snapshot(1, q, &log));
                m.apply_snapshot(&s, 0);
                assert_eq!(m.pred.digest, s.dg, "snapshot {q}");
            }
        }
    }
    #[test]
    fn own_throw_is_shown_at_once_and_stale_snapshots_dont_undo_it() {
        let (mut srv, log) = server();
        let mut m = screen(1);
        m.apply_snapshot(&wire(&srv.snapshot(1, 1, &log)), 0);
        assert!(m.local_aiming());
        assert_eq!(m.holder, None, "the ball is in your hand");
        let spec = perfect(&m);
        m.predict_throw(ThrowSpec { origin: spec.origin, yaw: spec.yaw, dist: spec.dist, curve: 0.0, ring: 2 }, 1);
        assert_eq!(m.state, State::Flight);
        assert!(!m.local_aiming(), "no second throw");
        for _ in 0..6 {
            m.mirror_tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        }
        let flown = m.ball;
        // a snapshot from before the server got the throw
        srv.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        m.apply_snapshot(&wire(&srv.snapshot(1, 2, &log)), 0);
        assert_eq!(m.state, State::Flight);
        assert_eq!(m.teams[0].throws, 1);
        assert!(!m.local_aiming());
        assert_eq!(m.ball, flown, "the ball doesn't jump back");
        // the server has it now
        srv.launch(0, perfect(&srv));
        m.apply_snapshot(&wire(&srv.snapshot(1, 3, &log)), 1);
        assert_eq!(m.state, State::Flight);
        assert_eq!(m.teams[0].throws, 1);
        // a throw the server never confirms is given up on
        let mut m2 = screen(1);
        let (srv2, log2) = server();
        m2.apply_snapshot(&wire(&srv2.snapshot(1, 1, &log2)), 0);
        m2.predict_throw(perfect(&m2), 1);
        for _ in 0..130 {
            m2.mirror_tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        }
        m2.apply_snapshot(&wire(&srv2.snapshot(1, 2, &log2)), 0);
        assert_eq!(m2.state, State::Aim);
    }
    #[test]
    fn hold_prediction_waits_for_the_ack() {
        let (mut srv, log) = server();
        srv.launch(0, perfect(&srv));
        for _ in 0..200 {
            srv.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        }
        assert_eq!(srv.state, State::Run);
        let mut m = screen(1);
        m.teams[0].dc.hold = true;
        m.pred.hold_n = 5;
        m.apply_snapshot(&wire(&srv.snapshot(1, 1, &log)), 4);
        assert!(m.teams[0].dc.hold, "the server hasn't got the press yet");
        m.apply_snapshot(&wire(&srv.snapshot(1, 2, &log)), 5);
        assert!(!m.teams[0].dc.hold, "now the server decides");
    }
    #[test]
    fn events_are_delivered_once_and_gaps_are_noticed() {
        let (srv, mut log) = server();
        let mut m = screen(2);
        log.push_ev(Ev::Straf(1, Foul::False), 0.0);
        m.apply_snapshot(&wire(&srv.snapshot(1, 1, &log)), 0);
        let n = m.stat_ev.len();
        assert_eq!(n, 1);
        m.apply_snapshot(&wire(&srv.snapshot(1, 2, &log)), 0);
        assert_eq!(m.stat_ev.len(), n, "not twice");
        // later events, the ones between were pruned before this screen got them
        log.push_ev(Ev::Stop, 1.0);
        log.push_ev(Ev::Stop, 9.0);
        log.prune(9.0);
        m.apply_snapshot(&wire(&srv.snapshot(1, 9, &log)), 0);
        assert_eq!(m.pred.missed_ev, 1);
        assert_eq!(m.pred.skipped_snaps, 6);
    }
    #[test]
    fn nobody_waits_for_a_thrower_who_is_gone() {
        let (mut srv, _) = server();
        assert_eq!(srv.state, State::Aim);
        let mut ms = two();
        ms[0].away = true;
        set_sides(&mut srv, &ms, None);
        srv.sides_changed();
        assert_eq!(srv.state, State::CpuAim);
        for _ in 0..240 {
            srv.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
        }
        assert_ne!(srv.state, State::CpuAim, "the computer threw");
    }
    #[test]
    fn stop_grace_forgives_a_slow_connection() {
        let run = |grace: f32| {
            let (mut g, _) = server();
            g.stop_grace = [grace; 2];
            g.launch(0, perfect(&g));
            for _ in 0..200 {
                g.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
            }
            g.drink_press(0);
            let mut t = 0;
            while g.state == State::Run && t < 30000 {
                g.tick(1.0 / 60.0, Vec3::ZERO, Vec3::ZERO);
                g.teams[0].dc.p = 0.0; // never choke, keep holding
                g.teams[0].beers = [1.0; 3];
                t += 1;
            }
            g.teams[0].dc.stop_watch
        };
        assert!(run(0.4) > run(0.0) + 0.3);
    }
}
