//! Accounts (a username and password; the session is a friend code and a secret kept on the device),
//! friend requests, who's online, messages kept for when you come back, invites to a room, coins and
//! the leaderboards. All of it on one file (FLUNKI_DATA, default `flunkiebal-data.json`), written
//! atomically a few seconds after a change. Each account's progression is kept here too.
//! (Until build 13 a phone got an account without a password just by connecting; `migrate` removed them.)

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
/// Notifications kept per account, and for how long: a phone checks every quarter of an hour or so.
const NOTES_KEEP: usize = 30;
const NOTE_AGE: u64 = 24 * 3600;
/// The part of a message a notification shows.
const NOTE_TEXT: usize = 120;

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
    /// what to tell them about while they're not looking (invites, messages), and the last note's id
    notes: Vec<Note>,
    note_next: u64,
    /// invites up to this note have reached the game itself (the phone's check still gets them)
    invites_seen: u64,
    /// coins, and whether the starting coins have been given (accounts from before coins get them too)
    coins: u32,
    started: bool,
    /// the day (UTC, days since 1970) of the last daily bonus, and of `earned`: coins from matches that day
    daily: u64,
    earn_day: u64,
    earned: u32,
    /// hats bought in the shop
    owned: Vec<u8>,
    /// online matches against people, for the leaderboard; `elo` is the rating less `START_RATING`
    played: u32,
    wins: u32,
    hits: u32,
    throws: u32,
    elo: f32,
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
    /// the bets put in when it started: (friend code, team, coins, still there at the end)
    pub bets: Vec<(String, u8, u32, bool)>,
}
/// Rating points at stake in one match.
const ELO_K: f32 = 32.0;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Store {
    accounts: HashMap<String, Account>,
    chats: HashMap<String, Chat>,
    pairs: HashMap<String, Pair>,
    /// which of `migrate`'s steps this file has had
    version: u32,
}
const STORE_VERSION: u32 = 1;
/// The computer's rating by difficulty, as `elo` (less `START_RATING`): for rating past matches only.
const CPU_ELO: [f32; 3] = [-150.0, 0.0, 150.0];

/// What the game keeps of a match (`save::MatchRec`), the parts the server reads from a progression.
#[derive(Deserialize, Default)]
#[serde(default)]
struct OldMatch {
    t: f64,
    mode: String,
    diff: usize,
    win: Option<bool>,
    throws: u32,
    dur: u32,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct OldProgress {
    matches: Vec<OldMatch>,
}

/// Bring an older file up to date.
/// 1 (build 13): every account gets a rating, from its past matches: the online ones between accounts
/// (put together again from the stats kept per pair, which also give the leaderboard's matches, wins
/// and hits) at full weight, those against the computer (from the progression) at half, all in the order
/// they were played. Then accounts without a password go, and everything of theirs.
fn migrate(st: &mut Store) {
    if st.version >= 1 {
        return;
    }
    // the online matches: pair records with the same end, arena and length are one match
    struct Online {
        at: u64,
        /// friend code, team, hits
        who: Vec<(String, u8, u32)>,
        winner: Option<u8>,
        seconds: u32,
    }
    let mut groups: HashMap<(u64, String, u32), Vec<(String, String, &PairRec)>> = HashMap::new();
    for (k, p) in &st.pairs {
        let Some((a, b)) = k.split_once(':') else { continue };
        for r in &p.recent {
            groups.entry((r.at, r.arena.clone(), r.seconds)).or_default().push((a.into(), b.into(), r));
        }
    }
    let mut online = Vec::new();
    for ((at, _, seconds), recs) in groups {
        // sides: whoever comes first is on team 0, the rest follow from together or against
        let mut team: HashMap<String, u8> = HashMap::new();
        team.insert(recs[0].0.clone(), 0);
        for _ in 0..recs.len() {
            for (a, b, r) in &recs {
                let flip = |t: u8| if r.together { t } else { 1 - t };
                match (team.get(a).copied(), team.get(b).copied()) {
                    (Some(t), None) => {
                        team.insert(b.clone(), flip(t));
                    }
                    (None, Some(t)) => {
                        team.insert(a.clone(), flip(t));
                    }
                    _ => {}
                }
            }
        }
        let mut winner = None;
        let mut hits: HashMap<String, u32> = HashMap::new();
        for (a, b, r) in &recs {
            hits.insert(a.clone(), r.a_hits);
            hits.insert(b.clone(), r.b_hits);
            if let Some(&ta) = team.get(a) {
                // together: did their team win; against: did `a` win. Either way, `a`'s team or not
                winner = Some(if r.won { ta } else { 1 - ta });
            }
        }
        let who: Vec<(String, u8, u32)> = team.iter().map(|(c, t)| (c.clone(), *t, hits.get(c).copied().unwrap_or(0))).collect();
        // against people only
        if who.iter().any(|w| w.1 == 0) && who.iter().any(|w| w.1 == 1) {
            online.push(Online { at, who, winner, seconds });
        }
    }
    // the matches against the computer, and each one's own online matches for their throws
    let mut cpu: Vec<(u64, String, usize, bool)> = Vec::new();
    let mut own: HashMap<String, Vec<(u64, u32)>> = HashMap::new();
    for (c, a) in &st.accounts {
        let Ok(p) = serde_json::from_str::<OldProgress>(&a.progress) else { continue };
        for m in p.matches {
            let start = if m.t > 1e11 { m.t / 1000.0 } else { m.t } as u64;
            match (m.mode.as_str(), m.win) {
                ("cpu", Some(w)) => cpu.push((start + m.dur as u64, c.clone(), m.diff.min(2), w)),
                ("online", _) => own.entry(c.clone()).or_default().push((start + m.dur as u64, m.throws)),
                _ => {}
            }
        }
    }
    enum Ev<'a> {
        On(&'a Online),
        Cpu(&'a (u64, String, usize, bool)),
    }
    let mut evs: Vec<(u64, Ev)> = online.iter().map(|o| (o.at, Ev::On(o))).chain(cpu.iter().map(|c| (c.0, Ev::Cpu(c)))).collect();
    evs.sort_by_key(|e| e.0);
    for a in st.accounts.values_mut() {
        (a.elo, a.played, a.wins, a.hits, a.throws) = (0.0, 0, 0, 0, 0);
    }
    for (_, e) in &evs {
        match e {
            Ev::On(o) => {
                let Some(w) = o.winner else { continue };
                let avg = |t: u8| {
                    let rs: Vec<f32> = o.who.iter().filter(|x| x.1 == t).filter_map(|x| st.accounts.get(&x.0)).map(|a| a.elo).collect();
                    rs.iter().sum::<f32>() / rs.len().max(1) as f32
                };
                let r = [avg(0), avg(1)];
                for (c, t, hits) in &o.who {
                    let throws = own.get(c).and_then(|v| v.iter().find(|m| m.0.abs_diff(o.at) <= 60 + o.seconds as u64 / 10)).map_or(0, |m| m.1);
                    let Some(a) = st.accounts.get_mut(c) else { continue };
                    let won = *t == w;
                    let expect = 1.0 / (1.0 + 10f32.powf((r[1 - *t as usize] - r[*t as usize]) / 400.0));
                    a.elo += ELO_K * (won as u32 as f32 - expect);
                    a.played += 1;
                    a.wins += won as u32;
                    a.hits += hits;
                    a.throws += throws.max(*hits);
                }
            }
            Ev::Cpu((_, c, diff, won)) => {
                let Some(a) = st.accounts.get_mut(c) else { continue };
                let expect = 1.0 / (1.0 + 10f32.powf((CPU_ELO[*diff] - a.elo) / 400.0));
                a.elo += ELO_K * 0.5 * (*won as u32 as f32 - expect);
            }
        }
    }
    // no more accounts without a password
    let gone: std::collections::HashSet<String> = st.accounts.iter().filter(|(_, a)| a.user.is_empty()).map(|(c, _)| c.clone()).collect();
    st.accounts.retain(|c, _| !gone.contains(c));
    for a in st.accounts.values_mut() {
        a.friends.retain(|c| !gone.contains(c));
        a.requests.retain(|c| !gone.contains(c));
        a.sent.retain(|c| !gone.contains(c));
        a.read.retain(|c, _| !gone.contains(c));
        a.notes.retain(|n| !gone.contains(&n.from));
    }
    let either = |k: &String| k.split(':').any(|c| gone.contains(c));
    st.chats.retain(|k, _| !either(k));
    st.pairs.retain(|k, _| !either(k));
    let rated: Vec<String> = st.accounts.values().filter(|a| a.elo != 0.0).map(|a| format!("{}={:.0}/{}", a.name, START_RATING + a.elo, a.played)).collect();
    info!("friends: file brought up to version 1: {} online and {} computer matches rated, {} accounts without a password removed; ratings {}", online.len(), cpu.len(), gone.len(), rated.join(" "));
    st.version = 1;
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
fn today() -> u64 {
    now() / 86400
}
fn wallet(a: &Account, gain: i64, why: &str) -> FromServer {
    FromServer::Wallet { coins: a.coins, owned: a.owned.clone(), gain, why: why.into() }
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
        let mut migrated = false;
        if store.version < STORE_VERSION {
            if store.accounts.is_empty() {
                store.version = STORE_VERSION;
            } else {
                // kept as it was, should anything go wrong
                let old = format!("{path}.before-v{STORE_VERSION}");
                match std::fs::copy(&path, &old) {
                    Ok(_) => {
                        migrate(&mut store);
                        migrated = true;
                    }
                    Err(e) => warn!("friends: couldn't keep {path} as {old} ({e}), so it's left as it was"),
                }
            }
        }
        for a in store.accounts.values_mut() {
            // with a password, the username is the name (accounts from before that was so)
            if !a.user.is_empty() && a.name.to_lowercase() != a.user {
                a.name = a.user.clone();
            }
        }
        let users = store.accounts.iter().filter(|(_, a)| !a.user.is_empty()).map(|(c, a)| (a.user.clone(), c.clone())).collect();
        let mut s = Social { store, path, online: HashMap::new(), by_conn: HashMap::new(), users, changed: None, new_window: Instant::now(), new_count: 0 };
        if migrated {
            s.save();
        }
        s
    }
    pub fn summary(&self) -> String {
        format!("{} accounts, {} online", self.store.accounts.len(), self.online.len())
    }
    pub fn conn_of(&self, code: &str) -> Option<u64> {
        self.online.get(code).map(|o| o.conn)
    }
    /// The friend code logged in on this connection.
    pub fn account_of(&self, conn: u64) -> Option<String> {
        self.by_conn.get(&conn).cloned()
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

    /// Does this code and secret belong to an account (with a password: there are no others)?
    pub fn known(&self, code: &str, secret: &str) -> bool {
        self.store.accounts.get(code).map_or(false, |a| !secret.is_empty() && a.secret == secret && !a.user.is_empty())
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
    /// An account with a password; `pass` is its hash.
    pub fn register(&mut self, conn: u64, user: &str, pass: String, name: &str) -> Result<Out, String> {
        if self.users.contains_key(user) {
            return Err("That username is taken".into());
        }
        let code = self.new_account(name, Account { user: user.into(), pass, ..Default::default() })?;
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
        // the starting coins, once, and the daily bonus
        let a = self.store.accounts.get_mut(code).unwrap();
        let mut gain = 0i64;
        let mut why = "";
        if !a.started {
            a.started = true;
            a.coins += START_COINS;
            gain += START_COINS as i64;
            why = "Welcome! Coins to start with";
            a.daily = today();
        } else if a.daily < today() {
            a.daily = today();
            a.coins += DAILY_COINS;
            gain += DAILY_COINS as i64;
            why = "Daily bonus";
        }
        if gain != 0 {
            info!("coins: {code} +{gain} ({why}), has {}", a.coins);
            self.dirty();
        }
        let a = &self.store.accounts[code];
        let purse = wallet(a, gain, why);
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
        out.push((code.to_string(), purse));
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
            ToServer::Buy { item } => {
                let day = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / 86_400) as u64;
                let a = self.store.accounts.get_mut(&me).ok_or("Not connected yet")?;
                // a costume: the pieces you don't have yet, for less
                let (price, items) = match bundle(item) {
                    Some(b) => bundle_price(&b, &a.owned),
                    None => (price_on(item, day).ok_or("That's not for sale")?, vec![item]),
                };
                if items.is_empty() || items.iter().all(|i| a.owned.contains(i)) {
                    return Err("You already have that".into());
                }
                if a.coins < price {
                    return Err(format!("That costs {price} coins, you have {}", a.coins));
                }
                a.coins -= price;
                a.owned.extend(items.iter().filter(|i| !a.owned.contains(i)).collect::<Vec<_>>());
                info!("coins: {me} bought item {item} for {price}, has {} left", a.coins);
                out.push((me.clone(), wallet(a, -(price as i64), "Bought in the shop")));
                self.dirty();
            }
            ToServer::Board { by } => {
                let (rows, mine) = self.board(&me, by);
                out.push((me.clone(), FromServer::Board { by, rows, me: mine }));
            }
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
                let name = self.store.accounts.get(&me).map_or(String::new(), |a| a.name.clone());
                let short: String = msg.text.chars().take(NOTE_TEXT).collect();
                out.push((to.clone(), FromServer::Said { with: me.clone(), m: msg }));
                out.push((to.clone(), self.list(&to)));
                self.note(&to, NOTE_MSG, &me, &name, &short, "", &mut out);
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
                let name = self.store.accounts.get(&me).map_or(String::new(), |a| a.name.clone());
                debug!("friends: {me} invites {to} to room {room}{}", if self.online.contains_key(&to) { "" } else { " (offline: a notification)" });
                // online the game asks at once; either way it's a notification, for a phone in a pocket
                if self.online.contains_key(&to) {
                    out.push((to.clone(), FromServer::Invited { code: me.clone(), name: name.clone(), room: room.clone() }));
                }
                self.note(&to, NOTE_INVITE, &me, &name, "", &room, &mut out);
            }
            _ => {}
        }
        Ok(out)
    }
    /// Something to tell `to` about: kept for their phone's background check, and sent at once if
    /// they're connected (a game in the background shows it). A new invite from someone replaces their last.
    fn note(&mut self, to: &str, kind: u8, from: &str, name: &str, text: &str, room: &str, out: &mut Out) {
        let Some(a) = self.store.accounts.get_mut(to) else { return };
        let t = now();
        a.note_next += 1;
        let note = Note { id: a.note_next, kind, from: from.into(), name: name.into(), text: text.into(), room: room.into(), at: t };
        a.notes.retain(|n| t.saturating_sub(n.at) < NOTE_AGE && !(kind == NOTE_INVITE && n.kind == NOTE_INVITE && n.from == from));
        a.notes.push(note.clone());
        if a.notes.len() > NOTES_KEEP {
            let extra = a.notes.len() - NOTES_KEEP;
            a.notes.drain(..extra);
        }
        let online = self.online.contains_key(to);
        if online && kind == NOTE_INVITE {
            a.invites_seen = a.note_next;
        }
        self.dirty();
        if online {
            out.push((to.into(), FromServer::Note { note }));
        }
    }
    /// The notes for a phone's background check: newer than `after`, still current. None: not who they say.
    /// `room_ok` says whether an invite's room can still be joined.
    pub fn notes_for(&self, code: &str, secret: &str, after: u64, room_ok: impl Fn(&str) -> bool) -> Option<Vec<Note>> {
        if !self.known(code, secret) {
            return None;
        }
        let t = now();
        let a = &self.store.accounts[code];
        Some(a.notes.iter().filter(|n| n.id > after && t.saturating_sub(n.at) < if n.kind == NOTE_INVITE { INVITE_KEEP } else { NOTE_AGE }).filter(|n| n.kind != NOTE_INVITE || room_ok(&n.room)).cloned().collect())
    }
    /// Invites that came while they were away and are still good, each handed over once: (from, name, room).
    pub fn waiting_invites(&mut self, conn: u64) -> Vec<(String, String, String)> {
        let Some(a) = self.by_conn.get(&conn).and_then(|c| self.store.accounts.get_mut(c)) else { return Vec::new() };
        let t = now();
        let got = a.notes.iter().filter(|n| n.kind == NOTE_INVITE && n.id > a.invites_seen && t.saturating_sub(n.at) < INVITE_KEEP).map(|n| (n.from.clone(), n.name.clone(), n.room.clone())).collect();
        if a.invites_seen != a.note_next {
            a.invites_seen = a.note_next;
            self.changed.get_or_insert_with(Instant::now);
        }
        got
    }
    /// Is this who they say they are? Then their matches count.
    pub fn verified(&self, me: &(String, String)) -> Option<String> {
        self.known(&me.0, &me.1).then(|| me.0.clone())
    }
    /// File a finished match: stats between each two accounts, the leaderboard, coins and bets.
    pub fn record(&mut self, f: &Finished) -> Out {
        let mut out = Vec::new();
        self.pairs(f);
        // each account once, should someone sit in two seats
        let mut ps: Vec<&(String, u8, u32, u32)> = Vec::new();
        for p in &f.players {
            if self.store.accounts.contains_key(&p.0) && !ps.iter().any(|q| q.0 == p.0) {
                ps.push(p);
            }
        }
        // the rating: each team as the average of its accounts, against people only
        let avg = |t: u8| {
            let rs: Vec<f32> = ps.iter().filter(|p| p.1 == t).map(|p| self.store.accounts[&p.0].elo).collect();
            (!rs.is_empty()).then(|| rs.iter().sum::<f32>() / rs.len() as f32)
        };
        let (r0, r1) = (avg(0), avg(1));
        for p in &ps {
            let won = p.1 == f.winner;
            let them = if p.1 == 0 { r1 } else { r0 };
            let mine = if p.1 == 0 { r0 } else { r1 };
            let a = self.store.accounts.get_mut(&p.0).unwrap();
            if let (Some(them), Some(mine)) = (them, mine) {
                let expect = 1.0 / (1.0 + 10f32.powf((them - mine) / 400.0));
                a.elo += ELO_K * (won as u32 as f32 - expect);
                a.played += 1;
                a.wins += won as u32;
                a.throws += p.2;
                a.hits += p.3;
            }
            let pay = match (them.is_some(), won) {
                (true, true) => WIN_COINS,
                (true, false) => LOSS_COINS,
                (false, true) => CPU_WIN_COINS,
                (false, false) => 0,
            };
            if a.earn_day != today() {
                a.earn_day = today();
                a.earned = 0;
            }
            let pay = pay.min(MATCH_COINS_DAY.saturating_sub(a.earned));
            a.earned += pay;
            a.coins += pay;
            let why = if pay > 0 { if won { "You won online" } else { "For playing online" } } else { "" };
            out.push((p.0.clone(), wallet(a, pay as i64, why)));
        }
        // the bets: the winners still there share the pot; none there, those still there get theirs back
        let pot: u32 = f.bets.iter().map(|b| b.2).sum();
        let winners: Vec<&(String, u8, u32, bool)> = f.bets.iter().filter(|b| b.1 == f.winner && b.3).collect();
        let take: Vec<(String, u32, &str)> = if !winners.is_empty() {
            let each = pot / winners.len() as u32;
            let rest = pot - each * winners.len() as u32;
            winners.iter().enumerate().map(|(i, b)| (b.0.clone(), each + if i == 0 { rest } else { 0 }, "You won the bet")).collect()
        } else {
            f.bets.iter().filter(|b| b.3).map(|b| (b.0.clone(), b.2, "Bet called off: nobody to win it")).collect()
        };
        if pot > 0 {
            info!("coins: a pot of {pot} goes to {:?}", take.iter().map(|t| (&t.0, t.1)).collect::<Vec<_>>());
        }
        for (code, n, why) in take {
            if let Some(a) = self.store.accounts.get_mut(&code) {
                a.coins += n;
                out.push((code, wallet(a, n as i64, why)));
            }
        }
        self.dirty();
        out
    }
    /// How many coins someone has.
    pub fn coins(&self, code: &str) -> u32 {
        self.store.accounts.get(code).map_or(0, |a| a.coins)
    }
    /// Put a bet in: the coins are taken now (checked beforehand with `coins`).
    pub fn stake(&mut self, code: &str, n: u32) -> Out {
        let Some(a) = self.store.accounts.get_mut(code) else { return Vec::new() };
        a.coins = a.coins.saturating_sub(n);
        let w = wallet(a, -(n as i64), "Bet placed");
        self.dirty();
        vec![(code.into(), w)]
    }
    /// A match with bets that never finished: everyone gets theirs back.
    pub fn refund(&mut self, bets: &[(String, u32)]) -> Out {
        let mut out = Vec::new();
        for (code, n) in bets {
            if let Some(a) = self.store.accounts.get_mut(code) {
                a.coins += n;
                out.push((code.clone(), wallet(a, *n as i64, "Bet called off")));
            }
        }
        if !bets.is_empty() {
            info!("coins: bets called off, refunded {bets:?}");
            self.dirty();
        }
        out
    }
    /// The top of the leaderboard and `me`'s line on it: everyone who's played someone online.
    fn board(&self, me: &str, by: u8) -> (Vec<BoardRow>, Option<BoardRow>) {
        // accuracy only once there are enough throws to tell
        // on it: who has played someone online, or has a rating from before (`migrate`)
        let mut all: Vec<(&String, &Account)> = self.store.accounts.iter().filter(|(_, a)| (a.played > 0 || a.elo != 0.0) && (by != BOARD_ACCURACY || a.throws >= ACCURACY_MIN)).collect();
        let key = |a: &Account| match by {
            BOARD_WINS => (a.wins as f32, a.elo),
            BOARD_COINS => (a.coins as f32, a.elo),
            BOARD_HITS => (a.hits as f32, a.elo),
            BOARD_ACCURACY => (a.hits as f32 / a.throws.max(1) as f32, a.throws as f32),
            _ => (a.elo, a.wins as f32),
        };
        all.sort_by(|x, y| key(y.1).partial_cmp(&key(x.1)).unwrap_or(std::cmp::Ordering::Equal).then_with(|| x.0.cmp(y.0)));
        let row = |i: usize, (c, a): (&String, &Account)| BoardRow { rank: i as u32 + 1, code: c.clone(), name: a.name.clone(), rating: (START_RATING + a.elo).round() as i32, played: a.played, wins: a.wins, hits: a.hits, throws: a.throws, coins: a.coins };
        let mine = all.iter().position(|(c, _)| c.as_str() == me).map(|i| row(i, all[i]));
        (all.iter().take(BOARD_TOP).enumerate().map(|(i, x)| row(i, *x)).collect(), mine)
    }
    fn pairs(&mut self, f: &Finished) {
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
