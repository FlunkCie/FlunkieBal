//! The hub owns all rooms and connections, on one task: socket events come in as messages, and it
//! ticks every match at 60 Hz. Nothing here is shared, so there is nothing to lock. Only password
//! hashing goes elsewhere (it takes tens of milliseconds) and comes back as an event.

use crate::room::{out, Conn, Room};
use crate::social::Social;
use crate::{debug, info, warn};
use flunkiebal_core::proto::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub enum Event {
    Open { conn: Conn, addr: SocketAddr },
    Text { conn: u64, text: String },
    Close { conn: u64, why: &'static str },
    Stuck { conn: u64 },
    /// A sign-up's password, hashed.
    Hashed { conn: u64, user: String, name: String, hash: Result<String, String> },
    /// A login's password, checked.
    Checked { conn: u64, user: String, code: String, name: String, ok: bool },
}

/// Logins and sign-ups a connection may try per minute, and wrong passwords per username per 15 minutes.
const AUTH_PER_MIN: u32 = 10;
const FAILS_PER_USER: u32 = 10;
/// Passwords being hashed at once (each takes ~19 MB for a moment).
const MAX_HASHING: usize = 8;

fn hash_password(pass: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| format!("no randomness: {e}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(pass.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}
/// Checked against for a username nobody has.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| hash_password("not anyone's password").unwrap_or_default())
}
fn password_ok(pass: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(hash).map_or(false, |h| argon2::Argon2::default().verify_password(pass.as_bytes(), &h).is_ok())
}

struct Client {
    conn: Conn,
    addr: SocketAddr,
    /// the seat this connection sits in: (room, seat)
    seat: Option<(String, u32)>,
    bad: u32,
    /// messages in the current second, to stop a runaway client
    rate: u32,
    last_heard: Instant,
    /// logins and sign-ups this minute, and whether one is being worked on
    auths: u32,
    auth_window: Instant,
    authing: bool,
}

pub struct Hub {
    social: Social,
    clients: HashMap<u64, Client>,
    rooms: HashMap<String, Room>,
    next_seat: u32,
    seed: u64,
    played: u64,
    /// where hashed passwords come back to
    me: mpsc::UnboundedSender<Event>,
    hashing: usize,
    /// username → (since, wrong passwords)
    fails: HashMap<String, (Instant, u32)>,
}

const TICK: Duration = Duration::from_micros(16_667);
const MAX_RATE: u32 = 200;
const MAX_ROOMS: usize = 2000;

fn clean(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).take(16).collect();
    if t.trim().is_empty() {
        "Player".into()
    } else {
        t.trim().to_string()
    }
}

impl Hub {
    pub fn new(me: mpsc::UnboundedSender<Event>) -> Hub {
        Hub { social: Social::load(), clients: HashMap::new(), rooms: HashMap::new(), next_seat: 0, seed: 0, played: 0, me, hashing: 0, fails: HashMap::new() }
    }
    /// Hand what the friends logic wants said to whoever is online.
    fn deliver(&self, out_msgs: Vec<(String, FromServer)>) {
        for (code, m) in out_msgs {
            if let Some(conn) = self.social.conn_of(&code) {
                self.tell(conn, &m);
            }
        }
    }
    fn rand(&mut self) -> u64 {
        // seeded from the clock, mixed; only used for codes, tokens and the matches' dice
        if self.seed == 0 {
            self.seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
    fn new_code(&mut self) -> String {
        const L: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        loop {
            let mut x = self.rand();
            let c: String = (0..4)
                .map(|_| {
                    let ch = L[(x % L.len() as u64) as usize] as char;
                    x /= L.len() as u64;
                    ch
                })
                .collect();
            if !self.rooms.contains_key(&c) {
                return c;
            }
        }
    }
    fn token(&mut self) -> String {
        format!("{:016x}{:016x}", self.rand(), self.rand())
    }
    fn tell(&self, conn: u64, m: &FromServer) {
        if let Some(c) = self.clients.get(&conn) {
            let _ = c.conn.tx.send(out(m));
        }
    }

    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Event>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        let mut report = Instant::now();
        let mut rates = Instant::now();
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Some(e) => self.event(e),
                    None => return,
                },
                _ = tick.tick() => {
                    let dt = last.elapsed().as_secs_f32();
                    last = Instant::now();
                    if dt > 0.1 {
                        warn!("server stalled: {:.0} ms between ticks ({} rooms)", dt * 1000.0, self.rooms.len());
                    }
                    self.tick_rooms(dt.min(0.1));
                    if rates.elapsed().as_secs_f32() >= 1.0 {
                        rates = Instant::now();
                        for c in self.clients.values_mut() {
                            c.rate = 0;
                        }
                    }
                    if rates.elapsed().as_millis() < 20 {
                        // about once a second: connections to friends that went quiet, and saving
                        let dead: Vec<u64> = self.clients.iter().filter(|(id, c)| self.social.is_online_conn(**id) && c.last_heard.elapsed().as_secs() > 45).map(|(id, _)| *id).collect();
                        for id in dead {
                            let m = self.social.disconnect(id);
                            self.deliver(m);
                        }
                        self.social.save_if_due();
                    }
                    if report.elapsed().as_secs() >= 60 {
                        report = Instant::now();
                        self.report();
                    }
                }
            }
        }
    }
    fn report(&self) {
        let playing = self.rooms.values().filter(|r| r.phase == PHASE_PLAYING).count();
        info!("status: {} connections, {} rooms, {playing} matches running, {} started so far, {}", self.clients.len(), self.rooms.len(), self.played, self.social.summary());
        for r in self.rooms.values() {
            info!("status: {}", r.summary());
        }
    }
    fn tick_rooms(&mut self, dt: f32) {
        let mut dead = Vec::new();
        for (code, r) in self.rooms.iter_mut() {
            if !r.tick(dt) {
                dead.push(code.clone());
            }
            if let Some(f) = r.finished.take() {
                self.social.record(&f);
            }
        }
        for code in dead {
            if let Some(r) = self.rooms.remove(&code) {
                info!("room={code} closed: nobody left ({})", r.summary());
                for c in self.clients.values_mut() {
                    if c.seat.as_ref().map_or(false, |s| s.0 == code) {
                        c.seat = None;
                    }
                }
            }
        }
    }
    fn event(&mut self, e: Event) {
        match e {
            Event::Open { conn, addr } => {
                debug!("conn={} open from {addr}", conn.id);
                self.clients.insert(conn.id, Client { addr, seat: None, bad: 0, rate: 0, last_heard: Instant::now(), auths: 0, auth_window: Instant::now(), authing: false, conn });
            }
            Event::Close { conn, why } => self.closed(conn, why),
            Event::Stuck { conn } => {
                warn!("conn={conn} write timed out, dropping it");
                self.closed(conn, "writes stuck");
            }
            Event::Hashed { conn, user, name, hash } => {
                self.hashing -= 1;
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.authing = false;
                let res = hash.map_err(|e| {
                    warn!("conn={conn} couldn't hash a password: {e}");
                    "Something went wrong, try again".to_string()
                });
                match res.and_then(|h| self.social.register(conn, &user, h, &name)) {
                    Ok(m) => self.deliver(m),
                    Err(msg) => self.tell(conn, &FromServer::AuthErr { msg }),
                }
            }
            Event::Checked { conn, user, code, name, ok } => {
                self.hashing -= 1;
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.authing = false;
                if ok {
                    self.fails.remove(&user);
                    info!("friends: conn={conn} logged in as user {user}");
                    let m = self.social.attach(conn, &code, &name);
                    self.deliver(m);
                } else {
                    let f = self.fails.entry(user.clone()).or_insert((Instant::now(), 0));
                    f.1 += 1;
                    if f.1 == FAILS_PER_USER {
                        warn!("friends: {FAILS_PER_USER} wrong passwords for user {user}, waiting a while");
                    }
                    self.tell(conn, &FromServer::AuthErr { msg: "Wrong username or password".into() });
                }
            }
            Event::Text { conn, text } => {
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.rate += 1;
                c.last_heard = Instant::now();
                if c.rate == MAX_RATE {
                    warn!("conn={conn} ({}) is flooding: {MAX_RATE} messages in a second", c.addr);
                }
                if c.rate >= MAX_RATE {
                    return;
                }
                match serde_json::from_str::<ToServer>(&text) {
                    Ok(m) => self.handle(conn, m),
                    Err(e) => {
                        c.bad += 1;
                        if c.bad <= 3 {
                            warn!("conn={conn} ({}) unreadable message: {e}", c.addr);
                        }
                    }
                }
            }
        }
    }
    fn closed(&mut self, conn: u64, why: &str) {
        let m = self.social.disconnect(conn);
        self.deliver(m);
        let Some(c) = self.clients.remove(&conn) else { return };
        if let Some((code, seat)) = c.seat {
            if let Some(r) = self.rooms.get_mut(&code) {
                r.conn_lost(seat, conn, why);
            }
        }
    }
    fn leave_current(&mut self, conn: u64) {
        let Some(c) = self.clients.get_mut(&conn) else { return };
        if let Some((code, seat)) = c.seat.take() {
            if let Some(r) = self.rooms.get_mut(&code) {
                r.remove(seat, "left the room");
            }
        }
    }
    fn version_ok(&self, conn: u64, v: u32) -> bool {
        if v == PROTOCOL {
            return true;
        }
        if let Some(c) = self.clients.get(&conn) {
            warn!("conn={conn} ({}) has protocol {v}, server has {PROTOCOL}", c.addr);
        }
        let msg = if v < PROTOCOL { "Your game is out of date. Update it to play online." } else { "The server is out of date for this game." };
        self.tell(conn, &FromServer::Err { msg: msg.into(), fatal: true });
        false
    }
    /// May this connection try a login or sign-up now? Then it's counted, and hashing has begun.
    fn may_auth(&mut self, conn: u64, user: &str) -> Result<(), String> {
        let Some(c) = self.clients.get_mut(&conn) else { return Err(String::new()) };
        if c.authing {
            return Err("Hold on, still checking".into());
        }
        if c.auth_window.elapsed().as_secs() >= 60 {
            c.auth_window = Instant::now();
            c.auths = 0;
        }
        c.auths += 1;
        if c.auths > AUTH_PER_MIN {
            return Err("Too many tries, wait a minute".into());
        }
        self.fails.retain(|_, f| f.0.elapsed().as_secs() < 15 * 60);
        if self.fails.get(user).map_or(false, |f| f.1 >= FAILS_PER_USER) {
            return Err("Too many wrong passwords for that username, try again later".into());
        }
        if self.hashing >= MAX_HASHING {
            return Err("The server is busy, try again in a moment".into());
        }
        c.authing = true;
        self.hashing += 1;
        Ok(())
    }
    fn handle(&mut self, conn: u64, m: ToServer) {
        let seat = self.clients.get(&conn).and_then(|c| c.seat.clone());
        match m {
            ToServer::Hb { c, r, s, d } => {
                self.tell(conn, &FromServer::Pong { c });
                if let Some((code, id)) = seat {
                    if let Some(room) = self.rooms.get_mut(&code) {
                        room.heartbeat(id, r, s, d);
                    }
                }
            }
            ToServer::SHello { v, code, secret, name, only } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                if only && !self.social.known(&code, &secret) {
                    info!("friends: conn={conn} came back as {code}, which isn't an account (any more)");
                    self.tell(conn, &FromServer::AuthErr { msg: "You've been logged out, log in again".into() });
                    return;
                }
                match self.social.hello(conn, &code, &secret, &clean(&name)) {
                    Ok(m) => self.deliver(m),
                    Err(msg) => self.tell(conn, &FromServer::SocialErr { msg }),
                }
            }
            ToServer::Register { v, user, pass, .. } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                // the player's name is the username, as typed
                let name = clean(&user);
                let checked = clean_user(&user).and_then(|u| {
                    if pass.chars().count() < PASS_MIN {
                        Err(format!("A password has at least {PASS_MIN} characters"))
                    } else if pass.len() > PASS_MAX {
                        Err("That password is too long".into())
                    } else if self.social.user(&u).is_some() {
                        Err("That username is taken".into())
                    } else {
                        Ok(u)
                    }
                });
                let user = match checked.and_then(|u| self.may_auth(conn, &u).map(|_| u)) {
                    Ok(u) => u,
                    Err(msg) => return self.tell(conn, &FromServer::AuthErr { msg }),
                };
                let me = self.me.clone();
                tokio::task::spawn_blocking(move || {
                    let hash = hash_password(&pass);
                    let _ = me.send(Event::Hashed { conn, user, name, hash });
                });
            }
            ToServer::Login { v, user, pass, name } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                let user = user.trim().to_lowercase();
                if let Err(msg) = self.may_auth(conn, &user) {
                    return self.tell(conn, &FromServer::AuthErr { msg });
                }
                let name = clean(&name);
                let found = self.social.user(&user);
                let me = self.me.clone();
                tokio::task::spawn_blocking(move || {
                    // an unknown name takes as long as a wrong password
                    let (code, hash) = found.unwrap_or_default();
                    let ok = password_ok(&pass, if hash.is_empty() { dummy_hash() } else { &hash }) && !code.is_empty();
                    let _ = me.send(Event::Checked { conn, user, code, name, ok });
                });
            }
            ToServer::SName { .. } | ToServer::SProgress { .. } | ToServer::SStatus { .. } | ToServer::FriendAdd { .. } | ToServer::FriendRemove { .. } | ToServer::Say { .. } | ToServer::ChatOpen { .. } | ToServer::FriendStats { .. } | ToServer::Invite { .. } => {
                let room_ok = |room: &str| self.rooms.get(room).map_or(false, |r| r.phase != PHASE_PLAYING && r.seats.len() < MAX_PEOPLE);
                let can_invite = if let ToServer::Invite { room, .. } = &m { room_ok(room) } else { true };
                match self.social.handle(conn, m, can_invite) {
                    Ok(out) => self.deliver(out),
                    Err(msg) => self.tell(conn, &FromServer::SocialErr { msg }),
                }
            }
            ToServer::List => {
                let rooms = self.rooms.iter().filter(|(_, r)| r.open && r.phase != PHASE_PLAYING && r.seats.len() < MAX_PEOPLE).map(|(c, r)| RoomInfo { code: c.clone(), name: r.host_name(), n: r.seats.len() }).take(20).collect();
                self.tell(conn, &FromServer::Rooms { rooms });
            }
            ToServer::Host { v, name, look, me } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                if self.rooms.len() >= MAX_ROOMS {
                    warn!("room limit reached ({MAX_ROOMS})");
                    self.tell(conn, &FromServer::Err { msg: "The server is full. Try again in a bit.".into(), fatal: false });
                    return;
                }
                self.leave_current(conn);
                let code = self.new_code();
                self.next_seat += 1;
                let id = self.next_seat;
                let token = self.token();
                let seed = self.rand();
                let name = clean(&name);
                let mut r = Room::new(code.clone(), id, seed);
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.seat = Some((code.clone(), id));
                let cn = c.conn.clone();
                let addr = c.addr;
                let _ = cn.tx.send(out(&FromServer::Welcome { code: code.clone(), id, token: token.clone(), resumed: false }));
                r.add_seat(id, name.clone(), look, token, cn);
                r.set_account(id, self.social.verified(&me));
                info!("room={code} created by player={id} ({name}) from {addr}");
                self.rooms.insert(code, r);
            }
            ToServer::Join { v, code, name, look, me } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                let code = code.trim().to_uppercase();
                let why = match self.rooms.get(&code) {
                    None => Some(format!("No match with code {code}")),
                    Some(r) if r.seats.len() >= MAX_PEOPLE => Some("That match is full".to_string()),
                    Some(r) if r.phase == PHASE_PLAYING => Some("That match has already started".to_string()),
                    _ => None,
                };
                if let Some(msg) = why {
                    debug!("conn={conn} couldn't join {code}: {msg}");
                    self.tell(conn, &FromServer::Err { msg, fatal: false });
                    return;
                }
                self.leave_current(conn);
                self.next_seat += 1;
                let id = self.next_seat;
                let token = self.token();
                let name = clean(&name);
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.seat = Some((code.clone(), id));
                let cn = c.conn.clone();
                let addr = c.addr;
                let _ = cn.tx.send(out(&FromServer::Welcome { code: code.clone(), id, token: token.clone(), resumed: false }));
                let account = self.social.verified(&me);
                let Some(r) = self.rooms.get_mut(&code) else { return };
                r.add_seat(id, name.clone(), look, token, cn);
                r.set_account(id, account);
                info!("room={code} player={id} ({name}) joined from {addr} ({} people)", r.seats.len());
            }
            ToServer::Resume { v, code, token } => {
                if !self.version_ok(conn, v) {
                    return;
                }
                let found = self.rooms.get(&code).and_then(|r| r.seat_by_token(&token));
                let Some(id) = found else {
                    info!("conn={conn} tried to come back to room={code}, but its seat is gone");
                    self.tell(conn, &FromServer::Closed { why: "That match is over: your seat has gone".into() });
                    return;
                };
                let Some(c) = self.clients.get_mut(&conn) else { return };
                c.seat = Some((code.clone(), id));
                let cn = c.conn.clone();
                let _ = cn.tx.send(out(&FromServer::Welcome { code: code.clone(), id, token, resumed: true }));
                if let Some(r) = self.rooms.get_mut(&code) {
                    r.hard_reset_ack(id);
                    r.resume(id, cn);
                }
            }
            other => {
                let Some((code, id)) = seat else { return };
                match other {
                    ToServer::Leave => self.leave_current(conn),
                    ToServer::Start => {
                        if let Some(r) = self.rooms.get_mut(&code) {
                            let was = r.phase;
                            r.start(id);
                            if r.phase == PHASE_PLAYING && was != PHASE_PLAYING {
                                self.played += 1;
                            }
                        }
                    }
                    _ => {
                        let Some(r) = self.rooms.get_mut(&code) else { return };
                        match other {
                            ToServer::Open { open } => {
                                if id == r.host {
                                    r.open = open;
                                    r.roster();
                                }
                            }
                            ToServer::Config { arena, diff } => r.config(id, arena, diff),
                            ToServer::Team { team } => r.set_team(id, team),
                            ToServer::Input { n, i } => r.input(id, n, i),
                            ToServer::Qm { i } => r.qm(id, i),
                            ToServer::Emote { i } => r.emote(id, i),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
