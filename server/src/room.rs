//! A room: its seats, its lobby and the match, which runs here and nowhere else.

use crate::{debug, info, warn};
use flunkiebal_core::game::{Ev, Foot, Game, Mode, State, ThrowSpec};
use flunkiebal_core::proto::*;
use glam::Vec3;
use std::collections::VecDeque;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

/// How long the match waits when someone's connection goes, per drop…
pub const PAUSE_ONE: f32 = 15.0;
/// …and in total per person per match, so nobody can hold everyone hostage.
pub const PAUSE_TOTAL: f32 = 40.0;
/// A little breath after someone is back.
const PAUSE_BACK: f32 = 1.5;
/// Nothing heard from a screen for this long (the app is asleep, a dead connection): they're away.
pub const SILENT: f32 = 4.0;
/// A seat stays reserved this long after the connection went.
const SEAT_KEEP_LOBBY: f32 = 20.0;
const SEAT_KEEP_MATCH: f32 = 120.0;
/// An idle thrower gets this long.
pub const AIM_LIMIT: f32 = 30.0;
/// From this round trip on someone's connection is called slow.
const SLOW_RTT: f32 = 0.25;
/// How much late STOP-hearing we forgive, at most.
const MAX_STOP_GRACE: f32 = 0.5;
const DIGESTS: usize = 200;

#[derive(Clone)]
pub struct Conn {
    pub tx: mpsc::UnboundedSender<String>,
    pub snap: watch::Sender<String>,
    pub id: u64,
}

pub struct Seat {
    pub m: Member,
    pub token: String,
    pub conn: Option<Conn>,
    pub last_heard: Instant,
    pub gone_at: Option<Instant>,
    pub rtt: f32,
    pub ack: u32,
    pub hold: bool,
    /// a verified friend code: this person's matches count for stats with friends
    account: Option<String>,
    throws: u32,
    hits: u32,
    emoted: Option<Instant>,
    /// was here when the current match started (who joins afterwards waits for the next)
    in_match: bool,
    /// device clock → server clock, for tap times
    tap_off: Option<f64>,
    pause_used: f32,
    /// has been told to everyone as gone
    away_told: bool,
    slow_told: bool,
    desyncs: u32,
    desync_run: u32,
    rejected: u32,
    late_snaps: u32,
    rtt_max: f32,
}

pub struct Pause {
    who: u32,
    left: f32,
    /// they're back, the match goes on after `left`
    back: bool,
}

pub struct Room {
    pub code: String,
    pub host: u32,
    pub open: bool,
    pub arena: String,
    pub diff: u8,
    pub phase: u8,
    pub seats: Vec<Seat>,
    pub empty_since: Option<Instant>,
    /// the match that just ended, for the hub to file
    pub finished: Option<crate::social::Finished>,
    last_thrower: Option<u32>,
    game: Game,
    log: EvLog,
    match_id: u32,
    seq: u32,
    snap_acc: f32,
    now: f32,
    started: Instant,
    pause: Option<Pause>,
    aim_t: f32,
    digests: VecDeque<(u32, u32)>,
    matches_played: u32,
    status_acc: f32,
    roster_dirty: bool,
}

pub fn out(m: &FromServer) -> String {
    serde_json::to_string(m).unwrap_or_default()
}

impl Room {
    pub fn new(code: String, host: u32, seed: u64) -> Room {
        Room { code, host, open: false, arena: "vondel".into(), diff: 1, phase: PHASE_LOBBY, seats: Vec::new(), empty_since: None, finished: None, last_thrower: None, game: Game::new(seed), log: EvLog::default(), match_id: seed as u32 & 0xffff, seq: 0, snap_acc: 0.0, now: 0.0, started: Instant::now(), pause: None, aim_t: 0.0, digests: VecDeque::new(), matches_played: 0, status_acc: 0.0, roster_dirty: false }
    }
    pub fn host_name(&self) -> String {
        self.seats.iter().find(|s| s.m.id == self.host).map_or(String::new(), |s| s.m.name.clone())
    }
    pub fn seat(&mut self, id: u32) -> Option<&mut Seat> {
        self.seats.iter_mut().find(|s| s.m.id == id)
    }
    fn name(&self, id: u32) -> String {
        self.seats.iter().find(|s| s.m.id == id).map_or("Someone".into(), |s| s.m.name.clone())
    }
    pub fn send(&self, id: u32, m: &FromServer) {
        if let Some(c) = self.seats.iter().find(|s| s.m.id == id).and_then(|s| s.conn.as_ref()) {
            let _ = c.tx.send(out(m));
        }
    }
    pub fn all(&self, m: &FromServer) {
        let t = out(m);
        for s in &self.seats {
            if let Some(c) = &s.conn {
                let _ = c.tx.send(t.clone());
            }
        }
    }
    fn others(&self, except: u32, m: &FromServer) {
        let t = out(m);
        for s in self.seats.iter().filter(|s| s.m.id != except) {
            if let Some(c) = &s.conn {
                let _ = c.tx.send(t.clone());
            }
        }
    }
    fn members(&self) -> Vec<Member> {
        self.seats.iter().map(|s| s.m.clone()).collect()
    }
    pub fn roster(&mut self) {
        self.roster_dirty = false;
        let m = FromServer::Roster { members: self.members(), host: self.host, arena: self.arena.clone(), diff: self.diff, phase: self.phase, open: self.open };
        self.all(&m);
    }
    pub fn notice(&self, title: impl Into<String>, sub: impl Into<String>) {
        self.all(&FromServer::Notice { title: title.into(), sub: sub.into() });
    }

    // ---------- people coming and going ----------
    pub fn add_seat(&mut self, id: u32, name: String, look: [u8; 7], token: String, conn: Conn) {
        // join the smaller team
        let t = if team_of(&self.members(), 1).len() <= team_of(&self.members(), 0).len() && !self.seats.is_empty() { 1 } else { 0 };
        self.seats.push(Seat { m: Member { id, name, team: t, look, away: false }, token, conn: Some(conn), last_heard: Instant::now(), gone_at: None, rtt: 0.0, ack: 0, hold: false, account: None, throws: 0, hits: 0, emoted: None, in_match: false, tap_off: None, pause_used: 0.0, away_told: false, slow_told: false, desyncs: 0, desync_run: 0, rejected: 0, late_snaps: 0, rtt_max: 0.0 });
        self.empty_since = None;
        self.roster();
    }
    pub fn set_account(&mut self, id: u32, account: Option<String>) {
        if let Some(s) = self.seat(id) {
            s.account = account;
        }
    }
    /// The socket under this seat closed.
    pub fn conn_lost(&mut self, id: u32, conn_id: u64, why: &str) {
        let code = self.code.clone();
        let Some(s) = self.seat(id) else { return };
        if s.conn.as_ref().map_or(true, |c| c.id != conn_id) {
            return; // an old socket: they have already resumed on a new one
        }
        s.conn = None;
        s.gone_at = Some(Instant::now());
        warn!("room={code} player={id} ({}) connection lost: {why}; rtt={:.0}ms", s.m.name, s.rtt * 1000.0);
        self.mark_away(id);
    }
    fn mark_away(&mut self, id: u32) {
        let Some(s) = self.seat(id) else { return };
        if s.away_told {
            return;
        }
        s.away_told = true;
        s.m.away = true;
        s.hold = false;
        let (name, used) = (s.m.name.clone(), s.pause_used);
        if self.phase == PHASE_PLAYING {
            self.refresh_sides();
            let budget = (PAUSE_TOTAL - used).min(PAUSE_ONE);
            if budget > 0.5 && self.pause.as_ref().map_or(true, |p| p.back) {
                self.pause = Some(Pause { who: id, left: budget, back: false });
                self.others(id, &FromServer::Notice { title: "Connection lost".into(), sub: format!("Waiting for {name} to come back") });
                info!("room={} match={} paused for {name} (up to {budget:.0}s)", self.code, self.match_id);
            } else {
                self.others(id, &FromServer::Notice { title: format!("{name} lost connection"), sub: self.cover_text(id) });
            }
        } else {
            self.notice(format!("{name} lost connection"), "Their seat is kept for a while");
        }
        self.pick_host();
        self.roster_dirty = true;
    }
    /// Who plays on when this person is gone, in words.
    fn cover_text(&self, id: u32) -> String {
        let Some(s) = self.seats.iter().find(|s| s.m.id == id) else { return String::new() };
        let mates = self.seats.iter().any(|o| o.m.team == s.m.team && !o.m.away);
        if mates { "Their teammates cover for them".into() } else { "The computer takes over their players".into() }
    }
    /// Someone is back, on this new socket.
    pub fn resume(&mut self, id: u32, conn: Conn) {
        let code = self.code.clone();
        let Some(s) = self.seat(id) else { return };
        let gone = s.gone_at.take().map_or(0.0, |t| t.elapsed().as_secs_f32());
        s.conn = Some(conn);
        s.last_heard = Instant::now();
        s.tap_off = None;
        let was_away = s.away_told;
        let name = s.m.name.clone();
        info!("room={code} player={id} ({name}) reconnected after {gone:.1}s");
        if was_away {
            self.back(id);
        }
        self.pick_host();
        self.roster();
    }
    fn back(&mut self, id: u32) {
        let Some(s) = self.seat(id) else { return };
        s.away_told = false;
        s.m.away = false;
        let name = s.m.name.clone();
        if self.phase == PHASE_PLAYING {
            self.refresh_sides();
        }
        self.others(id, &FromServer::Notice { title: format!("{name} is back"), sub: String::new() });
        if let Some(p) = self.pause.as_mut() {
            if p.who == id && !p.back {
                p.back = true;
                p.left = PAUSE_BACK;
            }
        }
        self.roster_dirty = true;
    }
    /// They left for good. Their seat goes; the match goes on without them.
    pub fn remove(&mut self, id: u32, why: &str) {
        let Some(ix) = self.seats.iter().position(|s| s.m.id == id) else { return };
        let s = self.seats.remove(ix);
        info!("room={} player={id} ({}) left: {why}; seen rtt max={:.0}ms desyncs={} rejected={} {}", self.code, s.m.name, s.rtt_max * 1000.0, s.desyncs, s.rejected, if self.phase == PHASE_PLAYING { "during a match" } else { "" });
        if self.pause.as_ref().map_or(false, |p| p.who == id) {
            self.pause = None;
        }
        if self.seats.is_empty() {
            return;
        }
        if self.phase == PHASE_PLAYING {
            self.refresh_sides();
        }
        let sub = if self.phase != PHASE_PLAYING { String::new() } else if self.seats.iter().any(|o| o.m.team == s.m.team && !o.m.away) { "Their teammates cover for them".to_string() } else { "The computer takes over their players".to_string() };
        self.notice(format!("{} left", s.m.name), sub);
        self.pick_host();
        self.roster();
    }
    /// The host only runs the lobby: when they're gone somebody else does.
    fn pick_host(&mut self) {
        let here = self.seats.iter().any(|s| s.m.id == self.host && !s.m.away);
        if !here {
            if let Some(n) = self.seats.iter().find(|s| !s.m.away).map(|s| s.m.id) {
                info!("room={} host is now player={n}", self.code);
                self.host = n;
                self.roster_dirty = true;
            }
        }
    }
    fn refresh_sides(&mut self) {
        let ms = self.members();
        set_sides(&mut self.game, &ms, None);
        // players can change hands: each drinks on only while whoever works them now holds DRINK
        for t in 0..2 {
            for i in 0..3 {
                let hold = owner_of(&ms, t, i).and_then(|m| self.seats.iter().find(|s| s.m.id == m.id)).map_or(false, |s| s.hold);
                if !hold {
                    let mut who = [false; 3];
                    who[i] = true;
                    self.game.drink_release(t, who);
                }
            }
        }
        self.game.sides_changed();
    }

    // ---------- lobby ----------
    pub fn set_team(&mut self, id: u32, t: u8) {
        if self.phase == PHASE_PLAYING {
            self.send(id, &FromServer::Err { msg: "You can't switch teams during a match".into(), fatal: false });
            return;
        }
        let t = t.min(1);
        let full = self.seats.iter().filter(|s| s.m.team == t && s.m.id != id).count() >= TEAM_MAX;
        if full {
            self.send(id, &FromServer::Err { msg: "That team is full: three a side".into(), fatal: false });
            return;
        }
        if let Some(s) = self.seat(id) {
            s.m.team = t;
        }
        self.roster();
    }
    pub fn config(&mut self, id: u32, arena: String, diff: u8) {
        if id != self.host || self.phase == PHASE_PLAYING {
            return;
        }
        if ARENAS.contains(&arena.as_str()) {
            self.arena = arena;
        }
        self.diff = diff.min(2);
        self.roster();
    }
    pub fn start(&mut self, id: u32) {
        if id != self.host {
            self.send(id, &FromServer::Err { msg: "Only the host can start the match".into(), fatal: false });
            return;
        }
        if self.phase == PHASE_PLAYING {
            return;
        }
        self.match_id = self.match_id.wrapping_add(1);
        self.seq = 0;
        self.log = EvLog::default();
        self.digests.clear();
        self.pause = None;
        self.aim_t = 0.0;
        self.phase = PHASE_PLAYING;
        self.started = Instant::now();
        self.matches_played += 1;
        for s in self.seats.iter_mut() {
            s.pause_used = 0.0;
            s.hold = false;
            s.in_match = true;
            s.throws = 0;
            s.hits = 0;
            s.desyncs = 0;
            s.desync_run = 0;
            s.rejected = 0;
        }
        let g = &mut self.game;
        g.bounds = arena_bounds(&self.arena);
        g.start_match(Mode::Online, self.diff as usize);
        let ms = self.members();
        set_sides(&mut self.game, &ms, None);
        self.game.restart_online();
        self.game.net_ev.clear();
        self.game.sfx.clear();
        self.game.stat_ev.clear();
        let team = |t: u8| self.seats.iter().filter(|s| s.m.team == t).map(|s| s.m.name.clone()).collect::<Vec<_>>().join(",");
        info!("room={} match={} started arena={} diff={} teamA=[{}] teamB=[{}]", self.code, self.match_id, self.arena, self.diff, team(0), team(1));
        self.roster();
        self.snap_acc = 1.0; // the first snapshot goes out right away
    }

    // ---------- in the match ----------
    pub fn qm(&self, id: u32, i: u8) {
        if (i as usize) < 9 {
            self.others(id, &FromServer::Qm { i, from: id });
        }
    }
    /// Just passed on: emotes live on the screens and change nothing in the match.
    pub fn emote(&mut self, id: u32, i: u8) {
        if i as usize >= flunkiebal_core::anim::EMOTES.len() {
            return;
        }
        let Some(s) = self.seat(id) else { return };
        if s.emoted.map_or(false, |t| t.elapsed().as_secs_f32() < 0.5) {
            return;
        }
        s.emoted = Some(Instant::now());
        self.others(id, &FromServer::Emote { i, from: id });
    }
    /// A device's tap time on the server's clock (in ms).
    fn tap_time(&mut self, id: u32, at: f64) -> Option<f64> {
        let now = self.started.elapsed().as_secs_f64() * 1000.0;
        let s = self.seat(id)?;
        // the smallest difference seen is the one with the least network in it
        let off = s.tap_off.map_or(now - at, |o: f64| o.min(now - at));
        s.tap_off = Some(off);
        Some((at + off).min(now))
    }
    pub fn input(&mut self, id: u32, n: u32, i: Input) {
        let Some(seat) = self.seat(id) else { return };
        if n <= seat.ack {
            return; // old news after a reconnect
        }
        seat.ack = n;
        let team = seat.m.team as usize;
        if self.phase != PHASE_PLAYING {
            return;
        }
        // everyone works their own players: their own beer, their own runner
        let who = owned(&self.members(), team, id);
        let paused = self.pause.is_some();
        match i {
            Input::Throw { o, yaw, dist, curve, ring } => {
                let g = &self.game;
                let t = g.turn;
                let idx = g.teams[t].idx;
                let owner = owner_of(&self.members(), t, idx).map(|m| m.id);
                let why = if g.state != State::Aim {
                    Some("It's not time to throw")
                } else if owner != Some(id) {
                    Some("It's someone else's throw")
                } else if paused {
                    Some("The match is paused")
                } else if ![o[0], o[1], o[2], yaw, dist, curve].iter().all(|x| x.is_finite()) || Vec3::from_array(o).distance(g.teams[t].players[idx].pos) > 4.0 {
                    warn!("room={} player={id} sent an impossible throw {o:?}", self.code);
                    Some("That throw didn't look right")
                } else {
                    None
                };
                if let Some(w) = why {
                    self.reject(id, n, w);
                    return;
                }
                debug!("room={} player={id} throws dist={dist:.1} ring={ring}", self.code);
                if let Some(s) = self.seat(id) {
                    s.throws += 1;
                }
                self.last_thrower = Some(id);
                self.game.launch(t, ThrowSpec { origin: Vec3::from_array(o), yaw, dist: dist.clamp(1.2, 13.0), curve: curve.clamp(-1.0, 1.0), ring: ring.min(3) });
                self.aim_t = 0.0;
            }
            Input::Drink { hold, at } => {
                if let Some(s) = self.seat(id) {
                    s.hold = hold;
                }
                if hold {
                    if paused {
                        return;
                    }
                    self.game.tap_ms = self.tap_time(id, at);
                    self.game.drink_press(team, who);
                    self.game.tap_ms = None;
                } else {
                    self.game.drink_release(team, who);
                }
            }
            Input::Pad { foot, at } => {
                if paused {
                    return;
                }
                self.game.tap_ms = self.tap_time(id, at);
                self.game.pad_press(team, who, if foot == 0 { Foot::L } else { Foot::R });
                self.game.tap_ms = None;
            }
            Input::Sneak => {
                if !paused {
                    self.game.start_sneak_for(team, who);
                }
            }
        }
    }
    fn reject(&mut self, id: u32, n: u32, why: &str) {
        let code = self.code.clone();
        if let Some(s) = self.seat(id) {
            s.rejected += 1;
            warn!("room={code} player={id} ({}) input {n} rejected: {why}; rtt={:.0}ms", s.m.name, s.rtt * 1000.0);
        }
        self.send(id, &FromServer::Reject { n, why: why.into() });
    }
    /// A heartbeat. Also where a screen's view of the match is checked against the real one.
    pub fn heartbeat(&mut self, id: u32, r: f32, s_no: u32, d: u32) {
        let expected = self.digests.iter().find(|(q, _)| *q == s_no).map(|x| x.1);
        let (code, match_id, seq, playing) = (self.code.clone(), self.match_id, self.seq, self.phase == PHASE_PLAYING);
        let Some(s) = self.seat(id) else { return };
        s.last_heard = Instant::now();
        if r > 0.0 {
            s.rtt = if s.rtt == 0.0 { r } else { s.rtt * 0.7 + r * 0.3 };
            s.rtt_max = s.rtt_max.max(r);
        }
        let slow = s.rtt > SLOW_RTT;
        if slow && !s.slow_told {
            s.slow_told = true;
            warn!("room={code} player={id} ({}) slow connection: rtt={:.0}ms", s.m.name, s.rtt * 1000.0);
        } else if s.slow_told && s.rtt < SLOW_RTT * 0.7 {
            s.slow_told = false;
            info!("room={code} player={id} ({}) connection recovered: rtt={:.0}ms", s.m.name, s.rtt * 1000.0);
        }
        // the screen is falling behind: it reported a snapshot long gone
        if playing && s_no != 0 && seq.saturating_sub(s_no) > 3 * SNAP_HZ as u32 {
            s.late_snaps += 1;
            if s.late_snaps % 5 == 1 {
                warn!("room={code} match={match_id} player={id} ({}) is {} snapshots behind", s.m.name, seq - s_no);
            }
        }
        if let Some(e) = expected {
            if e != d {
                s.desyncs += 1;
                s.desync_run += 1;
                warn!("room={code} match={match_id} player={id} ({}) DESYNC at snapshot {s_no}: screen {d:08x} server {e:08x} ({} in a row, {} total)", s.m.name, s.desync_run, s.desyncs);
                if s.desync_run == 2 {
                    self.send(id, &FromServer::Notice { title: "Out of sync".into(), sub: "Repairing your view of the match…".into() });
                }
            } else {
                if s.desync_run > 0 {
                    info!("room={code} match={match_id} player={id} ({}) back in sync after {} bad checks", s.m.name, s.desync_run);
                }
                s.desync_run = 0;
            }
        }
        // heard from again after going silent
        if self.seats.iter().any(|x| x.m.id == id && x.away_told && x.conn.is_some()) {
            info!("room={code} player={id} woke up again");
            self.back(id);
        }
    }
    pub fn hard_reset_ack(&mut self, id: u32) {
        if let Some(s) = self.seat(id) {
            s.ack = 0;
        }
    }

    /// One step of the room. Returns false when the room is done for.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.watch_seats();
        if self.seats.is_empty() {
            return false;
        }
        if self.seats.iter().all(|s| s.conn.is_none()) {
            let since = *self.empty_since.get_or_insert_with(Instant::now);
            return since.elapsed().as_secs_f32() < 60.0;
        }
        self.empty_since = None;
        self.status_acc += dt;
        if self.status_acc >= 1.0 {
            self.status_acc = 0.0;
            let l = self.seats.iter().map(|s| (s.m.id, (s.rtt * 1000.0).min(9999.0) as u16, if s.m.away { CONN_AWAY } else if s.rtt > SLOW_RTT { CONN_SLOW } else { CONN_GOOD })).collect();
            self.all(&FromServer::Status { l });
            if self.phase == PHASE_PLAYING {
                let g = |t: usize| self.seats.iter().filter(|s| s.m.team as usize == t && !s.m.away).map(|s| s.rtt).fold(0.0f32, f32::max);
                self.game.stop_grace = [g(0).min(MAX_STOP_GRACE), g(1).min(MAX_STOP_GRACE)];
            }
        }
        if self.roster_dirty {
            self.roster();
        }
        // after the final whistle the winners still get to cheer
        if self.phase != PHASE_LOBBY {
            self.tick_match(dt);
        }
        true
    }
    fn watch_seats(&mut self) {
        let keep = if self.phase == PHASE_PLAYING { SEAT_KEEP_MATCH } else { SEAT_KEEP_LOBBY };
        let mut drop_ids = Vec::new();
        let mut silent = Vec::new();
        for s in &self.seats {
            if s.gone_at.map_or(false, |t| t.elapsed().as_secs_f32() > keep) {
                drop_ids.push(s.m.id);
            } else if s.conn.is_some() && !s.away_told && s.last_heard.elapsed().as_secs_f32() > SILENT {
                silent.push(s.m.id);
            }
        }
        for id in drop_ids {
            self.remove(id, "didn't come back in time");
        }
        for id in silent {
            warn!("room={} player={id} ({}) silent for {SILENT}s (app asleep or dead connection)", self.code, self.name(id));
            self.mark_away(id);
        }
    }
    fn tick_match(&mut self, dt: f32) {
        let mut running = true;
        let mut left = -1.0;
        let mut who = 0;
        if let Some(p) = self.pause.as_mut() {
            p.left -= dt;
            left = p.left.max(0.0);
            who = p.who;
            if !p.back {
                let w = p.who;
                if let Some(s) = self.seat(w) {
                    s.pause_used += dt;
                }
            }
            let done = self.pause.as_ref().map_or(false, |p| p.left <= 0.0);
            if done {
                let back = self.pause.take().map_or(true, |p| p.back);
                if !back {
                    let (n, sub) = (self.name(who), self.cover_text(who));
                    warn!("room={} match={} {n} didn't come back in time; playing on", self.code, self.match_id);
                    self.notice(format!("Playing on without {n}"), sub);
                }
            } else {
                running = false;
            }
        }
        if running {
            self.game.tick(dt, Vec3::ZERO, Vec3::ZERO);
            self.now += dt;
            // an idle thrower doesn't hold up the match forever
            if self.game.state == State::Aim {
                self.aim_t += dt;
                if self.aim_t >= AIM_LIMIT {
                    self.aim_t = 0.0;
                    let t = self.game.turn;
                    let who = owner_of(&self.members(), t, self.game.teams[t].idx).map_or(String::from("Nobody"), |m| m.name.clone());
                    info!("room={} match={} {who} took too long, throwing for them", self.code, self.match_id);
                    self.notice("Too slow!", format!("{who} didn't throw, so the ball was thrown for them"));
                    self.last_thrower = None;
                    self.game.auto_throw();
                }
            } else {
                self.aim_t = 0.0;
            }
            let (now, evs, sfx) = (self.now, std::mem::take(&mut self.game.net_ev), std::mem::take(&mut self.game.sfx));
            for e in evs {
                self.log_event(e);
                self.log.push_ev(e, now);
            }
            for s in sfx {
                self.log.push_sx(s, now);
            }
            self.log.prune(now);
            self.game.banners.clear();
            self.game.stat_ev.clear();
            if self.game.show_over {
                self.game.show_over = false;
            }
        }
        self.snap_acc += dt;
        if self.snap_acc >= 1.0 / SNAP_HZ {
            // keep the remainder: starting from zero would slip to every fourth tick, 15 a second
            self.snap_acc = (self.snap_acc - 1.0 / SNAP_HZ).min(1.0 / SNAP_HZ);
            self.seq += 1;
            let mut s = self.game.snapshot(self.match_id, self.seq, &self.log);
            s.pz = left;
            s.pw = who;
            s.al = if self.game.state == State::Aim { (AIM_LIMIT - self.aim_t).max(0.0) } else { -1.0 };
            self.digests.push_back((self.seq, s.dg));
            if self.digests.len() > DIGESTS {
                self.digests.pop_front();
            }
            let body = serde_json::to_string(&s).unwrap_or_default();
            for seat in self.seats.iter().filter(|s| s.in_match) {
                if let Some(c) = &seat.conn {
                    // the newest snapshot replaces one not yet sent, so a slow phone skips rather than lags
                    let _ = c.snap.send(format!(r#"{{"t":"snap","a":{},"s":{}}}"#, seat.ack, body));
                }
            }
        }
    }
    fn log_event(&mut self, e: Ev) {
        if let Ev::Hit(t, ..) = e {
            // to who threw it; an automatic throw has nobody to credit
            let who = self.last_thrower.take();
            if let Some(s) = who.and_then(|id| self.seat(id)).filter(|s| s.m.team as usize == t) {
                s.hits += 1;
            }
        }
        match e {
            Ev::Over(w) => {
                self.phase = PHASE_OVER;
                self.pause = None;
                self.finished = Some(crate::social::Finished {
                    arena: self.arena.clone(),
                    seconds: self.started.elapsed().as_secs() as u32,
                    winner: w as u8,
                    players: self.seats.iter().filter(|s| s.in_match).filter_map(|s| s.account.clone().map(|c| (c, s.m.team, s.throws, s.hits))).collect(),
                });
                let g = &self.game;
                let dur = self.started.elapsed().as_secs();
                let rtts: Vec<String> = self.seats.iter().map(|s| format!("{}:{:.0}ms/{:.0}ms", s.m.name, s.rtt * 1000.0, s.rtt_max * 1000.0)).collect();
                let desyncs: u32 = self.seats.iter().map(|s| s.desyncs).sum();
                let used: f32 = self.seats.iter().map(|s| s.pause_used).sum();
                info!(
                    "room={} match={} finished winner={} duration={dur}s hits/throws={}/{} vs {}/{} strafbier={}/{} paused={used:.0}s desyncs={desyncs} rtt(now/max)=[{}]",
                    self.code, self.match_id, g.teams[w].name, g.teams[0].hits, g.teams[0].throws, g.teams[1].hits, g.teams[1].throws, g.teams[0].straf, g.teams[1].straf, rtts.join(" ")
                );
                self.roster_dirty = true;
            }
            Ev::Hit(t, ring, curve) => debug!("room={} {} hit ring={ring} curve={curve}", self.code, self.game.teams[t].name),
            Ev::Straf(t, f) => debug!("room={} strafbier {} {}", self.code, self.game.teams[t].name, f.text()),
            Ev::Out(t, left) => debug!("room={} {} player out, {left} left", self.code, self.game.teams[t].name),
            _ => {}
        }
    }
    pub fn summary(&self) -> String {
        let ph = ["lobby", "playing", "over"][self.phase.min(2) as usize];
        let rtt: Vec<String> = self.seats.iter().map(|s| format!("{}={:.0}ms{}", s.m.name, s.rtt * 1000.0, if s.m.away { "(away)" } else { "" })).collect();
        format!("room={} {ph} matches={} state={:?} [{}]", self.code, self.matches_played, self.game.state, rtt.join(" "))
    }
    pub fn seat_by_token(&self, token: &str) -> Option<u32> {
        self.seats.iter().find(|s| s.token == token).map(|s| s.m.id)
    }
    #[cfg(test)]
    pub fn game(&mut self) -> &mut Game {
        &mut self.game
    }
    #[cfg(test)]
    pub fn paused(&self) -> bool {
        self.pause.is_some()
    }
}
