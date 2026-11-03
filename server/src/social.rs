//! Friends: accounts without sign-up (a friend code and a secret kept on the device), friend requests,
//! who's online, messages kept for when you come back, and invites to a room. All of it on one file
//! (FLUNKI_DATA, default `flunkiebal-data.json`), written atomically a few seconds after a change.
//! The web version logs in with a username and password instead, and keeps its progression here too.

use crate::{debug, info, warn};
use flunkiebal_core::proto::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const CODE_CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const MAX_FRIENDS: usize = 100;
const MAX_REQUESTS: usize = 50;
const CHAT_KEEP: usize = 200;
const CHAT_SEND: usize = 60;
/// Things an account may do per minute: enough to chat, not enough to pester.
const ACTIONS_PER_MIN: u32 = 60;
const NEW_ACCOUNTS_PER_MIN: u32 = 60;
const SAVE_AFTER: f32 = 3.0;
const PAIR_RECENT: usize = 20;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Account {
    secret: String,
    name: String,
    /// username and argon2 hash, for an account made with a password
    user: String,
    pass: String,
    /// the game's progression and its revision, kept for an account with a password
    progress: String,
    rev: u64,
    friends: Vec<String>,
    /// they asked me
    requests: Vec<String>,
    /// I asked them
    sent: Vec<String>,
    /// friend → the last message id I've seen
    read: HashMap<String, u64>,
}

#[derive(Serialize, Deserialize, Default)]
struct Chat {
    next: u64,
    msgs: Vec<ChatMsg>,
}

/// Who played what with whom, kept for any two accounts in a match, friends yet or not. `a` is the
/// code that sorts first in the key. Stays when a friendship ends, so it's there if they make up.
#[derive(Serialize, Deserialize, Default)]
struct Pair {
    together: u32,
    together_wins: u32,
    against: u32,
    a_wins: u32,
    b_wins: u32,
    a_throws: u32,
    a_hits: u32,
    b_throws: u32,
    b_hits: u32,
    seconds: u32,
    recent: Vec<PairRec>,
}
#[derive(Serialize, Deserialize)]
struct PairRec {
    at: u64,
    arena: String,
    together: bool,
    /// together: did they win. Against: did `a` win.
    won: bool,
    a_hits: u32,
    b_hits: u32,
    seconds: u32,
}
/// A finished match, as a room hands it over.
pub struct Finished {
    pub arena: String,
    pub seconds: u32,
    pub winner: u8,
    /// (friend code, team, throws, hits)
    pub players: Vec<(String, u8, u32, u32)>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Store {
    accounts: HashMap<String, Account>,
    chats: HashMap<String, Chat>,
    pairs: HashMap<String, Pair>,
}

struct Online {
    conn: u64,
    status: u8,
    window: Instant,
    actions: u32,
    progress_at: Option<Instant>,
}

pub struct Social {
    store: Store,
    path: String,
    online: HashMap<String, Online>,
    by_conn: HashMap<u64, String>,
    /// username → friend code
    users: HashMap<String, String>,
    changed: Option<Instant>,
    new_window: Instant,
    new_count: u32,
}

type Out = Vec<(String, FromServer)>;

fn pair(a: &str, b: &str) -> String {
    if a < b {
        format!("{a}:{b}")
    } else {
        format!("{b}:{a}")
    }
}
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
fn random_text(n: usize) -> Result<String, String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(b.iter().map(|x| CODE_CHARS[*x as usize % CODE_CHARS.len()] as char).collect())
}
fn clean_code(c: &str) -> String {
    c.trim().to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

impl Social {
    pub fn load() -> Social {
        let path = std::env::var("FLUNKI_DATA").unwrap_or_else(|_| "flunkiebal-data.json".into());
        let store = match std::fs::read_to_string(&path) {
            Ok(t) => match serde_json::from_str::<Store>(&t) {
                Ok(s) => {
                    info!("friends: loaded {} accounts from {path}", s.accounts.len());
                    s
                }
                Err(e) => {
                    // never overwrite what we couldn't read
                    let bad = format!("{path}.bad-{}", now());
                    let _ = std::fs::rename(&path, &bad);
                    warn!("friends: {path} is unreadable ({e}); kept as {bad}, starting empty");
                    Store::default()
                }
            },
            Err(_) => {
                info!("friends: no {path} yet, starting empty");
                Store::default()
            }
        };
        let mut store = store;
        for a in store.accounts.values_mut() {
            // with a password, the username is the name (accounts from before that was so)
            if !a.user.is_empty() && a.name.to_lowercase() != a.user {
                a.name = a.user.clone();
            }
        }
        let users = store.accounts.iter().filter(|(_, a)| !a.user.is_empty()).map(|(c, a)| (a.user.clone(), c.clone())).collect();
        Social { store, path, online: HashMap::new(), by_conn: HashMap::new(), users, changed: None, new_window: Instant::now(), new_count: 0 }
    }
    pub fn summary(&self) -> String {
        format!("{} accounts, {} online", self.store.accounts.len(), self.online.len())
    }
    pub fn conn_of(&self, code: &str) -> Option<u64> {
        self.online.get(code).map(|o| o.conn)
    }
    pub fn is_online_conn(&self, conn: u64) -> bool {
        self.by_conn.contains_key(&conn)
    }
    fn dirty(&mut self) {
        self.changed.get_or_insert_with(Instant::now);
    }
    pub fn save_if_due(&mut self) {
        if self.changed.map_or(false, |t| t.elapsed().as_secs_f32() >= SAVE_AFTER) {
            self.save();
        }
    }
    fn save(&mut self) {
        self.changed = None;
        let tmp = format!("{}.tmp", self.path);
        let res = serde_json::to_string(&self.store).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&tmp, t).map_err(|e| e.to_string())).and_then(|_| std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string()));
        if let Err(e) = res {
            warn!("friends: couldn't save to {}: {e}", self.path);
        }
    }

    fn friend_card(&self, me: &str, code: &str) -> Option<Friend> {
        let a = self.store.accounts.get(code)?;
        let o = self.online.get(code);
        let last_read = self.store.accounts.get(me).and_then(|m| m.read.get(code).copied()).unwrap_or(0);
        let unread = self.store.chats.get(&pair(me, code)).map_or(0, |c| c.msgs.iter().filter(|m| m.id > last_read && m.from != me).count() as u32);
        Some(Friend { code: code.into(), name: a.name.clone(), online: o.is_some(), status: o.map_or(STATUS_FREE, |o| o.status), unread })
    }
    /// The list as this person should see it.
    fn list(&self, me: &str) -> FromServer {
        let Some(a) = self.store.accounts.get(me) else { return FromServer::Friends { friends: vec![], requests: vec![], sent: vec![] } };
        let cards = |v: &Vec<String>| v.iter().filter_map(|c| self.friend_card(me, c)).collect::<Vec<_>>();
        FromServer::Friends { friends: cards(&a.friends), requests: cards(&a.requests), sent: cards(&a.sent) }
    }
    /// Everyone who has this person on a list needs to know something changed.
    fn lists_for_people_around(&self, code: &str, out: &mut Out) {
        out.push((code.into(), self.list(code)));
        if let Some(a) = self.store.accounts.get(code) {
            for c in a.friends.iter().chain(&a.requests).chain(&a.sent) {
                if self.online.contains_key(c) {
                    out.push((c.clone(), self.list(c)));
                }
            }
        }
    }

    /// Does this code and secret belong to an account?
    pub fn known(&self, code: &str, secret: &str) -> bool {
        self.store.accounts.get(code).map_or(false, |a| !secret.is_empty() && a.secret == secret)
    }
    pub fn hello(&mut self, conn: u64, code: &str, secret: &str, name: &str) -> Result<Out, String> {
        if self.known(code, secret) {
            return Ok(self.attach(conn, code, name));
        }
        // a device with no account, or one this server no longer knows (or a wrong secret): a new one
        if !code.is_empty() {
            warn!("friends: conn={conn} failed to log in as {code}; giving it a new account");
        }
        let code = self.new_account(name, Account::default())?;
        Ok(self.attach(conn, &code, name))
    }
    fn new_account(&mut self, name: &str, mut a: Account) -> Result<String, String> {
        if self.new_window.elapsed().as_secs() >= 60 {
            self.new_window = Instant::now();
            self.new_count = 0;
        }
        self.new_count += 1;
        if self.new_count > NEW_ACCOUNTS_PER_MIN {
            warn!("friends: too many new accounts this minute");
            return Err("The server is busy, try again in a minute".into());
        }
        let code = loop {
            let c = random_text(CODE_LEN)?;
            if !self.store.accounts.contains_key(&c) {
                break c;
            }
        };
        a.secret = random_text(32)?;
        a.name = name.into();
        if !a.user.is_empty() {
            self.users.insert(a.user.clone(), code.clone());
        }
        info!("friends: new account {code} ({name}){}", if a.user.is_empty() { String::new() } else { format!(" as user {}", a.user) });
        self.store.accounts.insert(code.clone(), a);
        self.dirty();
        Ok(code)
    }
    /// The friend code and password hash for a username.
    pub fn user(&self, user: &str) -> Option<(String, String)> {
        let code = self.users.get(user)?;
        self.store.accounts.get(code).map(|a| (code.clone(), a.pass.clone()))
    }
    /// An account with a password; `pass` is its hash. A phone already logged in with its own account
    /// (no password yet) keeps it, friends and all: it gets the username and password.
    pub fn register(&mut self, conn: u64, user: &str, pass: String, name: &str) -> Result<Out, String> {
        if self.users.contains_key(user) {
            return Err("That username is taken".into());
        }
        let mine = self.by_conn.get(&conn).cloned().filter(|c| self.store.accounts.get(c).map_or(false, |a| a.user.is_empty()));
        let code = match mine {
            Some(code) => {
                let a = self.store.accounts.get_mut(&code).unwrap();
                a.user = user.into();
                a.pass = pass;
                a.name = name.into();
                self.users.insert(user.into(), code.clone());
                info!("friends: account {code} is user {user} now");
                self.dirty();
                code
            }
            None => self.new_account(name, Account { user: user.into(), pass, ..Default::default() })?,
        };
        Ok(self.attach(conn, &code, name))
    }
    /// This connection is that account now.
    pub fn attach(&mut self, conn: u64, code: &str, name: &str) -> Out {
        // logged in as someone else on this connection until now: they've gone
        let mut out = if self.by_conn.get(&conn).map_or(false, |c| c != code) { self.disconnect(conn) } else { Vec::new() };
        let Some(a) = self.store.accounts.get_mut(code) else { return out };
        // with a password, the username is the name
        if a.user.is_empty() && a.name != name {
            a.name = name.into();
            self.dirty();
        }
        let a = &self.store.accounts[code];
        let me = FromServer::Me { code: code.into(), secret: a.secret.clone(), name: a.name.clone(), user: a.user.clone() };
        let progress = (!a.user.is_empty()).then(|| FromServer::Progress { rev: a.rev, data: a.progress.clone() });
        // logging in again from a new socket: that one counts now
        if let Some(old) = self.online.remove(code) {
            self.by_conn.remove(&old.conn);
        }
        self.by_conn.insert(conn, code.into());
        self.online.insert(code.into(), Online { conn, status: STATUS_FREE, window: Instant::now(), actions: 0, progress_at: None });
        out.push((code.to_string(), me));
        out.extend(progress.map(|p| (code.to_string(), p)));
        self.lists_for_people_around(code, &mut out);
        out
    }
    pub fn disconnect(&mut self, conn: u64) -> Out {
        let mut out = Vec::new();
        let Some(code) = self.by_conn.remove(&conn) else { return out };
        self.online.remove(&code);
        self.lists_for_people_around(&code, &mut out);
        out.retain(|(c, _)| *c != code);
        out
    }

    pub fn handle(&mut self, conn: u64, m: ToServer, can_invite: bool) -> Result<Out, String> {
        let me = self.by_conn.get(&conn).cloned().ok_or("Not connected yet")?;
        let o = self.online.get_mut(&me).ok_or("Not connected yet")?;
        // status changes and opening a chat don't count against you
        if !matches!(m, ToServer::SStatus { .. } | ToServer::ChatOpen { .. } | ToServer::FriendStats { .. } | ToServer::SProgress { .. }) {
            if o.window.elapsed().as_secs() >= 60 {
                o.window = Instant::now();
                o.actions = 0;
            }
            o.actions += 1;
            if o.actions > ACTIONS_PER_MIN {
                if o.actions == ACTIONS_PER_MIN + 1 {
                    warn!("friends: {me} is over the limit of {ACTIONS_PER_MIN} actions a minute");
                }
                return Err("Slow down a little".into());
            }
        }
        let mut out = Vec::new();
        match m {
            ToServer::SName { name } => {
                let n: String = name.chars().filter(|c| !c.is_control()).take(16).collect();
                let n = if n.trim().is_empty() { "Player".to_string() } else { n.trim().to_string() };
                if let Some(a) = self.store.accounts.get_mut(&me).filter(|a| a.user.is_empty()) {
                    a.name = n;
                }
                self.dirty();
                self.lists_for_people_around(&me, &mut out);
            }
            ToServer::SStatus { status } => {
                if o.status != status {
                    o.status = status.min(STATUS_MATCH);
                    self.lists_for_people_around(&me, &mut out);
                }
            }
            ToServer::SProgress { rev, data } => {
                // at most once a second, and only what fits
                if o.progress_at.map_or(false, |t| t.elapsed().as_secs_f32() < 1.0) {
                    return Err("Slow down a little".into());
                }
                o.progress_at = Some(Instant::now());
                if data.len() > MAX_PROGRESS {
                    warn!("friends: {me} sent {} bytes of progress", data.len());
                    return Err("Your progress is too big to keep".into());
                }
                let Some(a) = self.store.accounts.get_mut(&me).filter(|a| !a.user.is_empty()) else { return Err("Log in to keep your progress".into()) };
                a.progress = data;
                a.rev = rev;
                self.dirty();
            }
            ToServer::FriendAdd { code } => self.add(&me, &clean_code(&code), &mut out)?,
            ToServer::FriendRemove { code } => {
                let code = clean_code(&code);
                for (a, b) in [(me.as_str(), code.as_str()), (code.as_str(), me.as_str())] {
                    if let Some(acc) = self.store.accounts.get_mut(a) {
                        acc.friends.retain(|c| c != b);
                        acc.requests.retain(|c| c != b);
                        acc.sent.retain(|c| c != b);
                        acc.read.remove(b);
                    }
                }
                self.store.chats.remove(&pair(&me, &code));
                self.dirty();
                debug!("friends: {me} removed {code}");
                out.push((me.clone(), self.list(&me)));
                out.push((code.clone(), self.list(&code)));
            }
            ToServer::Say { to, text } => {
                let to = clean_code(&to);
                self.must_be_friends(&me, &to)?;
                let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_CHAT).collect();
                let text = text.trim().to_string();
                if text.is_empty() {
                    return Ok(out);
                }
                let chat = self.store.chats.entry(pair(&me, &to)).or_default();
                chat.next += 1;
                let msg = ChatMsg { id: chat.next, from: me.clone(), text, at: now() };
                chat.msgs.push(msg.clone());
                if chat.msgs.len() > CHAT_KEEP {
                    let extra = chat.msgs.len() - CHAT_KEEP;
                    chat.msgs.drain(..extra);
                }
                if let Some(a) = self.store.accounts.get_mut(&me) {
                    a.read.insert(to.clone(), msg.id);
                }
                self.dirty();
                out.push((me.clone(), FromServer::Said { with: to.clone(), m: msg.clone() }));
                out.push((to.clone(), FromServer::Said { with: me.clone(), m: msg }));
                out.push((to.clone(), self.list(&to)));
            }
            ToServer::ChatOpen { with } => {
                let with = clean_code(&with);
                self.must_be_friends(&me, &with)?;
                let (msgs, last) = self.store.chats.get(&pair(&me, &with)).map_or((Vec::new(), 0), |c| {
                    let from = c.msgs.len().saturating_sub(CHAT_SEND);
                    (c.msgs[from..].to_vec(), c.next)
                });
                if let Some(a) = self.store.accounts.get_mut(&me) {
                    if a.read.insert(with.clone(), last) != Some(last) {
                        self.dirty();
                    }
                }
                out.push((me.clone(), FromServer::History { with, msgs }));
                out.push((me.clone(), self.list(&me)));
            }
            ToServer::FriendStats { with } => {
                let with = clean_code(&with);
                self.must_be_friends(&me, &with)?;
                let s = self.pair_view(&me, &with);
                out.push((me.clone(), FromServer::FriendStats { with, s }));
            }
            ToServer::Invite { to, room } => {
                let to = clean_code(&to);
                self.must_be_friends(&me, &to)?;
                if !can_invite {
                    return Err("That match can't take more people".into());
                }
                if !self.online.contains_key(&to) {
                    return Err("They're offline".into());
                }
                let name = self.store.accounts.get(&me).map_or(String::new(), |a| a.name.clone());
                debug!("friends: {me} invites {to} to room {room}");
                out.push((to, FromServer::Invited { code: me, name, room }));
            }
            _ => {}
        }
        Ok(out)
    }
    /// Is this who they say they are? Then their matches count.
    pub fn verified(&self, me: &(String, String)) -> Option<String> {
        self.store.accounts.get(&me.0).filter(|a| !me.1.is_empty() && a.secret == me.1).map(|_| me.0.clone())
    }
    pub fn record(&mut self, f: &Finished) {
        for (i, x) in f.players.iter().enumerate() {
            for y in &f.players[i + 1..] {
                if x.0 == y.0 {
                    continue;
                }
                // the same order as the key
                let (a, b) = if x.0 < y.0 { (x, y) } else { (y, x) };
                let p = self.store.pairs.entry(pair(&a.0, &b.0)).or_default();
                let together = a.1 == b.1;
                let won = a.1 == f.winner;
                if together {
                    p.together += 1;
                    p.together_wins += won as u32;
                } else {
                    p.against += 1;
                    if won {
                        p.a_wins += 1;
                    } else {
                        p.b_wins += 1;
                    }
                }
                p.a_throws += a.2;
                p.a_hits += a.3;
                p.b_throws += b.2;
                p.b_hits += b.3;
                p.seconds += f.seconds;
                p.recent.push(PairRec { at: now(), arena: f.arena.clone(), together, won, a_hits: a.3, b_hits: b.3, seconds: f.seconds });
                if p.recent.len() > PAIR_RECENT {
                    p.recent.remove(0);
                }
            }
        }
        self.dirty();
    }
    fn pair_view(&self, me: &str, other: &str) -> PairStats {
        let Some(p) = self.store.pairs.get(&pair(me, other)) else { return PairStats::default() };
        let mine_a = me < other;
        let side = |a: u32, b: u32| if mine_a { (a, b) } else { (b, a) };
        let (my_wins, their_wins) = side(p.a_wins, p.b_wins);
        let (my_throws, their_throws) = side(p.a_throws, p.b_throws);
        let (my_hits, their_hits) = side(p.a_hits, p.b_hits);
        let recent = p
            .recent
            .iter()
            .rev()
            .map(|r| {
                let (mh, th) = side(r.a_hits, r.b_hits);
                PairMatch { at: r.at, arena: r.arena.clone(), together: r.together, won: if r.together || mine_a { r.won } else { !r.won }, my_hits: mh, their_hits: th, seconds: r.seconds }
            })
            .collect();
        PairStats { together: p.together, together_wins: p.together_wins, against: p.against, my_wins, their_wins, my_throws, my_hits, their_throws, their_hits, seconds: p.seconds, recent }
    }
    fn must_be_friends(&self, me: &str, other: &str) -> Result<(), String> {
        if self.store.accounts.get(me).map_or(false, |a| a.friends.iter().any(|c| c == other)) {
            Ok(())
        } else {
            Err("You're not friends".into())
        }
    }
    fn add(&mut self, me: &str, code: &str, out: &mut Out) -> Result<(), String> {
        if code == me {
            return Err("That's your own code".into());
        }
        let Some(them) = self.store.accounts.get(code) else { return Err(format!("No player with code {code}")) };
        let Some(mine) = self.store.accounts.get(me) else { return Err("Not connected yet".into()) };
        if mine.friends.iter().any(|c| c == code) {
            return Err("You're already friends".into());
        }
        if mine.sent.iter().any(|c| c == code) {
            return Err("You've already asked them".into());
        }
        if mine.friends.len() >= MAX_FRIENDS {
            return Err("Your friend list is full".into());
        }
        let they_asked = mine.requests.iter().any(|c| c == code);
        if they_asked {
            if them.friends.len() >= MAX_FRIENDS {
                return Err("Their friend list is full".into());
            }
            if let Some(a) = self.store.accounts.get_mut(me) {
                a.requests.retain(|c| c != code);
                a.friends.push(code.into());
            }
            if let Some(b) = self.store.accounts.get_mut(code) {
                b.sent.retain(|c| c != me);
                b.friends.push(me.into());
            }
            info!("friends: {me} and {code} are now friends");
        } else {
            if them.requests.len() >= MAX_REQUESTS {
                return Err("They have too many open requests".into());
            }
            if let Some(a) = self.store.accounts.get_mut(me) {
                a.sent.push(code.into());
            }
            if let Some(b) = self.store.accounts.get_mut(code) {
                b.requests.push(me.into());
            }
        }
        self.dirty();
        out.push((me.into(), self.list(me)));
        out.push((code.into(), self.list(code)));
        Ok(())
    }
}
