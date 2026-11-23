//! Friends, on their own connection to the server, open as long as the game runs and independent of
//! any match: who's online, requests, messages, invites. The server keeps it all (`server/src/social.rs`).
//! Where logging in is needed (`save::needs_login`) it's also the way in: a login or sign-up, then the
//! session kept in the save; logged out, it stays closed.

use crate::net::{BoardRow, ChatMsg, Friend, FromServer, Note, PairStats, Socket, ToServer, PROTOCOL};
use std::collections::HashMap;

/// Nothing from the server for this long: the connection is dead, whatever the socket says.
const DEAD_AFTER: f64 = 12.0;
const PING_EVERY: f64 = 10.0;
/// An invite you don't answer goes away.
const INVITE_FOR: f64 = 60.0;

/// What the app should know of from this poll.
pub enum Got {
    /// logged in: friend code, secret and (with a password) the username
    /// (`name`: the account's name, which with a password is the username as typed)
    Me { code: String, secret: String, user: String, name: String },
    /// the progression the server keeps for this account
    Progress { rev: u64, data: String },
    /// coins and hats bought, kept on the server; `gain` and `why`: what changed, to say so
    Wallet { coins: u32, owned: Vec<u8>, gain: i64, why: String },
    /// the kept session is gone
    LoggedOut,
}

pub struct Invite {
    pub code: String,
    pub name: String,
    pub room: String,
    pub at: f64,
}

#[derive(Default)]
pub struct Social {
    sock: Option<Socket>,
    url: String,
    next_try: f64,
    tries: u32,
    last_rx: f64,
    ping_t: f64,
    status: u8,
    sent_name: String,
    /// logged in
    pub ready: bool,
    /// the game is too old for this server: no point in trying again
    pub stopped: bool,
    pub code: String,
    pub friends: Vec<Friend>,
    pub requests: Vec<Friend>,
    pub sent: Vec<Friend>,
    pub chats: HashMap<String, Vec<ChatMsg>>,
    pub open_chat: Option<String>,
    pub open_stats: Option<String>,
    pub stats: HashMap<String, PairStats>,
    pub invite: Option<Invite>,
    pub err: String,
    pub add_in: String,
    pub say_in: String,
    /// someone's message arrived that isn't the open chat: (their name, text)
    pub toast: Option<(String, String)>,
    /// a login or sign-up to send once connected, and whether its answer is still to come
    pub auth: Option<ToServer>,
    pub busy: bool,
    auth_sent: Option<f64>,
    pub auth_err: String,
    pub user: String,
    /// the newest build of the game there is, as the server says (0: not heard yet)
    pub latest: u32,
    /// notifications that came in, for the app to show
    pub notes: Vec<Note>,
    /// a bug report on its way, and the server's answer: (kept, why not)
    pub bug_busy: bool,
    pub bug_res: Option<(bool, String)>,
    bug_at: f64,
    /// the leaderboards that came in, by `BOARD_*`: the top, and our own line
    pub boards: HashMap<u8, (Vec<BoardRow>, Option<BoardRow>)>,
    /// a purchase on its way
    pub buying: bool,
    /// the leaderboard last asked for, asked again once connected
    board_want: Option<u8>,
}

impl Social {
    pub fn send(&mut self, m: ToServer) {
        if let (true, Some(s)) = (self.ready, self.sock.as_ref()) {
            if let Ok(t) = serde_json::to_string(&m) {
                s.send(&t);
            }
        }
    }
    /// A bug report: on this connection whether logged in or not, it's there as long as the game runs.
    pub fn send_bug(&mut self, now: f64, text: String, info: String) -> bool {
        let Some(s) = self.sock.as_ref().filter(|s| s.state() == 1) else { return false };
        let Ok(t) = serde_json::to_string(&ToServer::Bug { v: PROTOCOL, text, info }) else { return false };
        s.send(&t);
        self.bug_busy = true;
        self.bug_res = None;
        self.bug_at = now;
        true
    }
    pub fn unread(&self) -> u32 {
        self.friends.iter().map(|f| f.unread).sum()
    }
    pub fn name_of(&self, code: &str) -> String {
        self.friends.iter().find(|f| f.code == code).map_or_else(|| code.to_string(), |f| f.name.clone())
    }
    pub fn open(&mut self, code: &str) {
        self.open_chat = Some(code.to_string());
        self.err.clear();
        self.send(ToServer::ChatOpen { with: code.into() });
    }
    pub fn open_stats_for(&mut self, code: &str) {
        self.open_stats = Some(code.to_string());
        self.err.clear();
        self.send(ToServer::FriendStats { with: code.into() });
    }
    /// Ask for a leaderboard (it comes in `boards`).
    pub fn ask_board(&mut self, by: u8) {
        self.board_want = Some(by);
        self.send(ToServer::Board { by });
    }
    pub fn buy(&mut self, item: u8) {
        self.err.clear();
        self.buying = true;
        self.send(ToServer::Buy { item });
    }
    pub fn say_open_chat(&mut self) {
        let text = self.say_in.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(to) = self.open_chat.clone() else { return };
        self.say_in.clear();
        self.send(ToServer::Say { to, text });
    }
    /// Log in, or make an account (`new`), with a username and password.
    pub fn log_in(&mut self, user: &str, pass: &str, name: &str, new: bool) {
        let (user, pass, name, v) = (user.trim().into(), pass.into(), name.into(), PROTOCOL);
        self.auth = Some(if new { ToServer::Register { v, user, pass, name } } else { ToServer::Login { v, user, pass, name } });
        self.busy = true;
        self.auth_err.clear();
        self.next_try = 0.0;
    }
    /// Close the connection and forget who we were.
    pub fn log_out(&mut self) {
        if let Some(s) = self.sock.take() {
            s.close();
        }
        *self = Social { url: std::mem::take(&mut self.url), latest: self.latest, ..Default::default() };
    }
    /// Keep connected, take in news. `only`: log in with a password or a kept session, never get a new
    /// account by just connecting; logged out, stay closed.
    pub fn poll(&mut self, now: f64, url: &str, known: (&str, &str), name: &str, status: u8, only: bool) -> Vec<Got> {
        let mut got = Vec::new();
        if url != self.url {
            // another server knows other people
            *self = Social { url: url.into(), ..Default::default() };
        }
        if self.bug_busy && now - self.bug_at > 10.0 {
            self.bug_busy = false;
            self.bug_res = Some((false, "No answer from the server, try again".into()));
        }
        if self.stopped || url.trim().is_empty() {
            self.busy = false;
            return got;
        }
        // logged out: the connection stays open, without logging in, so a login goes out at once
        let idle = only && known.0.is_empty() && self.auth.is_none() && !self.busy;
        if idle && self.ready {
            if let Some(s) = self.sock.take() {
                s.close();
            }
            self.ready = false;
        }
        if let Some(i) = self.invite.as_ref() {
            if now - i.at > INVITE_FOR {
                self.invite = None;
            }
        }
        let st = self.sock.as_ref().map_or(2, |s| s.state());
        // the keepalive gets an answer every second or two, logged in or not: silence means it's dead
        if st == 2 || (st == 1 && now - self.last_rx > DEAD_AFTER) || (st == 0 && now - self.last_rx > 10.0) {
            // gone: once more in a moment
            let had = self.sock.take().is_some();
            self.ready = false;
            if self.busy && had {
                // a login waits for no reconnecting: say so, and try again when asked
                self.busy = false;
                self.auth = None;
                self.auth_err = if self.sent_name.is_empty() { format!("Can't reach the server ({})", url.trim()) } else { "Lost the connection, try again".into() };
                return got;
            }
            if had {
                self.next_try = now + (1.0 + self.tries as f64 * 2.0).min(30.0);
                self.tries += 1;
            }
            if now >= self.next_try {
                self.next_try = now + 2.0;
                self.last_rx = now;
                match Socket::connect(url.trim()) {
                    Ok(mut s) => {
                        s.keepalive();
                        self.sock = Some(s);
                    }
                    Err(e) => {
                        self.err = e.clone();
                        if self.auth.take().is_some() {
                            self.busy = false;
                            self.auth_err = e;
                        }
                    }
                }
                self.status = 255;
                self.sent_name.clear();
            }
            return got;
        }
        let Some(s) = self.sock.as_ref() else { return got };
        if st == 1 && !self.ready && self.sent_name.is_empty() && !idle {
            // once, right after opening
            self.sent_name = name.to_string();
            let hello = match self.auth.take() {
                Some(a) => {
                    self.auth_sent = Some(now);
                    a
                }
                None => ToServer::SHello { v: PROTOCOL, code: known.0.into(), secret: known.1.into(), name: name.into(), only },
            };
            if let Ok(t) = serde_json::to_string(&hello) {
                s.send(&t);
            }
        }
        let mut inbox = Vec::new();
        while let Some((t, _)) = s.recv() {
            inbox.push(t);
        }
        for t in inbox {
            self.last_rx = now;
            let Ok(m) = serde_json::from_str::<FromServer>(&t) else { continue };
            match m {
                FromServer::Me { code, secret, user, name } => {
                    self.ready = true;
                    self.busy = false;
                    self.tries = 0;
                    self.err.clear();
                    self.auth_err.clear();
                    self.code = code.clone();
                    self.user = user.clone();
                    got.push(Got::Me { code, secret, user, name });
                    // the open chat may have moved on while we were away
                    if let Some(c) = self.open_chat.clone() {
                        self.send(ToServer::ChatOpen { with: c });
                    }
                    if let Some(by) = self.board_want {
                        self.send(ToServer::Board { by });
                    }
                }
                FromServer::Friends { friends, requests, sent } => {
                    self.friends = friends;
                    self.requests = requests;
                    self.sent = sent;
                }
                FromServer::History { with, msgs } => {
                    self.chats.insert(with, msgs);
                }
                FromServer::Said { with, m } => {
                    if self.open_chat.as_deref() == Some(&with) {
                        // read as it arrives
                        if m.from != self.code {
                            self.send(ToServer::ChatOpen { with: with.clone() });
                        }
                    } else if m.from != self.code {
                        self.toast = Some((self.name_of(&with), m.text.clone()));
                    }
                    let c = self.chats.entry(with).or_default();
                    if !c.iter().any(|x| x.id == m.id) {
                        c.push(m);
                    }
                }
                FromServer::Invited { code, name, room } => {
                    crate::audio::ui(crate::audio::Cue::Chime);
                    self.invite = Some(Invite { code, name, room, at: now });
                }
                FromServer::Note { note } => self.notes.push(note),
                FromServer::Latest { build } => self.latest = build,
                FromServer::BugSaved { ok, msg } => {
                    self.bug_busy = false;
                    self.bug_res = Some((ok, msg));
                }
                FromServer::FriendStats { with, s } => {
                    self.stats.insert(with, s);
                }
                FromServer::Progress { rev, data } => got.push(Got::Progress { rev, data }),
                FromServer::Wallet { coins, owned, gain, why } => {
                    self.buying = false;
                    got.push(Got::Wallet { coins, owned, gain, why });
                }
                FromServer::Board { by, rows, me } => {
                    self.boards.insert(by, (rows, me));
                }
                FromServer::AuthErr { msg } => {
                    // whatever it was, this connection is no one: close it, and try again only when asked
                    let was_session = !self.busy;
                    self.auth_err = msg;
                    self.log_out_keep_err();
                    if was_session {
                        got.push(Got::LoggedOut);
                    }
                    return got;
                }
                FromServer::SocialErr { msg } => {
                    self.buying = false;
                    self.err = msg;
                }
                FromServer::Err { msg, fatal } => {
                    if self.busy {
                        self.busy = false;
                        self.auth_err = msg.clone();
                    }
                    self.err = msg;
                    if fatal {
                        self.stopped = true;
                    }
                }
                _ => {}
            }
        }
        if self.busy && self.auth_sent.map_or(false, |t| now - t > 15.0) {
            // a server too old to know logins never answers
            self.auth_err = "No answer from the server, try again".into();
            self.log_out_keep_err();
            return got;
        }
        if self.ready {
            if let Some(a) = self.auth.take() {
                // already connected as the device's own account: log in (or sign up) on the same connection
                if let (Ok(t), Some(s)) = (serde_json::to_string(&a), self.sock.as_ref()) {
                    s.send(&t);
                }
                self.auth_sent = Some(now);
            }
            if self.status != status {
                self.status = status;
                self.send(ToServer::SStatus { status });
            }
            if self.sent_name != name {
                self.sent_name = name.to_string();
                self.send(ToServer::SName { name: name.into() });
            }
            if now - self.ping_t > PING_EVERY {
                self.ping_t = now;
                self.send(ToServer::Hb { c: 0.0, r: 0.0, s: 0, d: 0 });
            }
        }
        got
    }
    fn log_out_keep_err(&mut self) {
        let e = std::mem::take(&mut self.auth_err);
        self.log_out();
        self.auth_err = e;
    }
}
