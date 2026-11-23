//! The real server binary, with bot players on real websockets running the real mirror.

use flunkiebal_core::game::{Foot, Game, State};
use flunkiebal_core::proto::*;
use futures_util::{SinkExt, StreamExt};
use glam::Vec3;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Server {
    child: Child,
    log: Arc<Mutex<Vec<String>>>,
    url: String,
}
impl Server {
    fn start(port: u16) -> Server {
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("flunkiebal-test-{port}.json")));
        Server::start_keeping_data(port)
    }
    fn start_keeping_data(port: u16) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_flunkiebal-server")).arg(port.to_string()).env("FLUNKI_LOG", "debug").env("FLUNKI_DATA", std::env::temp_dir().join(format!("flunkiebal-test-{port}.json"))).stdout(Stdio::piped()).spawn().unwrap();
        let out = child.stdout.take().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                l2.lock().unwrap().push(line);
            }
        });
        Server { child, log, url: format!("ws://127.0.0.1:{port}") }
    }
    fn logged(&self, what: &str) -> bool {
        self.log.lock().unwrap().iter().any(|l| l.contains(what))
    }
    fn dump(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct Bot {
    ws: Ws,
    g: Game,
    id: u32,
    code: String,
    token: String,
    members: Vec<Member>,
    phase: u8,
    notices: Vec<String>,
    rejects: Vec<String>,
    err: Option<String>,
    closed: Option<String>,
    hb: Instant,
}
async fn open(url: &str) -> Ws {
    for _ in 0..50 {
        if let Ok((ws, _)) = connect_async(url).await {
            return ws;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("server didn't come up");
}
impl Bot {
    async fn new(url: &str) -> Bot {
        Bot { ws: open(url).await, g: Game::new(1), id: 0, code: String::new(), token: String::new(), members: vec![], phase: 0, notices: vec![], rejects: vec![], err: None, closed: None, hb: Instant::now() }
    }
    async fn send(&mut self, m: ToServer) {
        let _ = self.ws.send(Message::Text(serde_json::to_string(&m).unwrap().into())).await;
    }
    /// Counted as the app counts: in the mirror's `Pred`, which carries on into the next match.
    async fn input(&mut self, i: Input) {
        self.g.pred.sent_n += 1;
        let n = self.g.pred.sent_n;
        self.send(ToServer::Input { n, i }).await;
    }
    /// Handle everything that arrives for this long, with heartbeats like a phone's.
    async fn run(&mut self, secs: f32) {
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f32() < secs {
            if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(20), self.ws.next()).await {
                match serde_json::from_str::<FromServer>(&t).expect("server message reads") {
                    FromServer::Welcome { code, id, token, .. } => {
                        self.code = code;
                        self.id = id;
                        self.token = token;
                    }
                    FromServer::Roster { members, phase, .. } => {
                        self.members = members;
                        self.phase = phase;
                        if self.g.mirror {
                            set_sides(&mut self.g, &self.members, Some(self.id));
                        }
                    }
                    FromServer::Snap { a, s } => {
                        if s.m != self.g.pred.match_id {
                            self.g.mirror_begin(s.m);
                            set_sides(&mut self.g, &self.members, Some(self.id));
                        }
                        self.g.apply_snapshot(&s, a);
                    }
                    FromServer::Notice { title, sub } => self.notices.push(format!("{title} | {sub}")),
                    FromServer::Reject { why, .. } => self.rejects.push(why),
                    FromServer::Err { msg, .. } => self.err = Some(msg),
                    FromServer::Closed { why } => self.closed = Some(why),
                    _ => {}
                }
            }
            let (s, d) = (self.g.pred.last_q, self.g.pred.digest);
            if self.hb.elapsed().as_secs_f32() > 0.9 {
                self.hb = Instant::now();
                self.send(ToServer::Hb { c: 0.0, r: 0.05, s, d }).await;
            }
        }
    }
    fn throw_spec(&self) -> Input {
        let t = self.g.turn;
        let o = self.g.teams[t].players[self.g.teams[t].idx].pos + Vec3::new(0.0, 1.2, -self.g.teams[t].side * 0.5);
        Input::Throw { o: o.to_array(), yaw: (-o.x).atan2(-o.z), dist: (o.x * o.x + o.z * o.z).sqrt() - 0.3, curve: 0.0, ring: 2 }
    }
}
/// An account to play online with: (friend code, secret).
async fn creds(url: &str, name: &str) -> (String, String) {
    let p = Pal::login(url, "", "", name).await;
    (p.code, p.secret)
}
async fn pair(s: &Server) -> (Bot, Bot) {
    let look = [0u8; 7];
    let mut a = Bot::new(&s.url).await;
    a.send(ToServer::Host { v: PROTOCOL, name: "Anna".into(), look, me: creds(&s.url, "Anna").await, wear: [0; 8] }).await;
    a.run(0.2).await;
    let mut b = Bot::new(&s.url).await;
    b.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Bram".into(), look, me: creds(&s.url, "Bram").await, wear: [0; 8] }).await;
    b.run(0.2).await;
    a.run(0.1).await;
    assert_eq!(a.members.len(), 2);
    assert_ne!(a.members[0].team, a.members[1].team, "they're put on different teams");
    (a, b)
}

/// Play until someone wins: always hit, always drink.
async fn play_out(s: &Server, a: &mut Bot, b: &mut Bot) {
    let t0 = Instant::now();
    let mut foot = 0;
    while a.g.state != State::Over {
        assert!(t0.elapsed().as_secs() < 150, "no winner after 150 s\n{}", s.dump());
        for bot in [&mut *a, &mut *b] {
            bot.run(0.03).await;
            let g = &bot.g;
            let me = g.my_team.unwrap();
            let d = &g.teams[me].dc[g.teams[me].lead(g.my_players[me]).unwrap_or(0)];
            if g.local_aiming() {
                let i = bot.throw_spec();
                bot.input(i).await;
            } else if g.can_drink(me) && !d.hold && d.cough <= 0.0 && d.p < 0.3 {
                bot.input(Input::Drink { hold: true, at: t0.elapsed().as_secs_f64() * 1000.0 }).await;
            } else if d.hold && d.p > 0.6 {
                bot.input(Input::Drink { hold: false, at: t0.elapsed().as_secs_f64() * 1000.0 }).await;
            } else if g.can_run(me) {
                foot = 1 - foot;
                bot.input(Input::Pad { foot, at: t0.elapsed().as_secs_f64() * 1000.0 }).await;
            }
        }
    }
}

#[tokio::test]
async fn whole_match_with_bots() {
    let s = Server::start(18791);
    let (mut a, mut b) = pair(&s).await;
    a.send(ToServer::Start).await;
    b.run(0.3).await;
    assert_eq!(b.phase, PHASE_PLAYING);
    assert!(b.g.mirror);
    let first = b.g.pred.last_q;
    b.run(1.0).await;
    let got = b.g.pred.last_q - first;
    assert!((17..=23).contains(&got), "{got} snapshots in a second");
    play_out(&s, &mut a, &mut b).await;
    a.run(0.3).await;
    b.run(0.3).await;
    assert_eq!(b.g.state, State::Over);
    assert_eq!(a.g.win, b.g.win);
    assert_eq!(a.phase, PHASE_OVER);
    assert_eq!(a.g.pred.missed_ev, 0);
    assert!(s.logged("started arena=vondel"), "{}", s.dump());
    assert!(s.logged("finished winner="), "{}", s.dump());
    assert!(!s.logged("DESYNC"), "{}", s.dump());
    let _ = Foot::L;
    // someone joining between matches gets the lobby, not the finished match
    let mut c = Bot::new(&s.url).await;
    c.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Cato".into(), look: [0; 7], me: creds(&s.url, "Cato").await, wear: [0; 8] }).await;
    c.run(0.4).await;
    assert!(c.err.is_none(), "{:?}", c.err);
    assert_eq!(c.g.pred.last_q, 0, "no snapshots for them yet");
    a.send(ToServer::Start).await;
    c.run(0.4).await;
    assert!(c.g.mirror, "they're in the next match");
    // …and the others followed
    b.run(0.4).await;
    assert_ne!(b.g.state, State::Over);
    // the next match takes their throws too
    let t0 = Instant::now();
    let mut thrown = false;
    while !thrown {
        assert!(t0.elapsed().as_secs() < 15, "nobody could throw in the next match\n{}", s.dump());
        for bot in [&mut a, &mut b, &mut c] {
            bot.run(0.03).await;
            if bot.g.local_aiming() && !thrown {
                let t = bot.g.turn;
                let before = bot.g.teams[t].throws;
                let i = bot.throw_spec();
                bot.input(i).await;
                bot.run(0.6).await;
                assert!(bot.rejects.is_empty(), "{:?}", bot.rejects);
                assert_eq!(bot.g.teams[t].throws, before + 1, "the server took the throw");
                assert_ne!(bot.g.state, State::Aim);
                thrown = true;
            }
        }
    }
}

#[tokio::test]
async fn dropped_player_pauses_and_comes_back() {
    let s = Server::start(18792);
    let (mut a, mut b) = pair(&s).await;
    a.send(ToServer::Start).await;
    a.run(0.3).await;
    b.run(0.3).await;
    // the connection breaks without a goodbye
    let (code, token, id) = (b.code.clone(), b.token.clone(), b.id);
    drop(b.ws);
    a.run(0.5).await;
    assert!(a.g.net_pause.is_some_and(|(w, left)| w == id && left > 10.0), "paused for them: {:?}\n{}", a.g.net_pause, s.dump());
    assert!(a.members.iter().any(|m| m.id == id && m.away));
    assert!(a.notices.iter().any(|n| n.contains("Waiting for Bram")), "{:?}", a.notices);
    // nothing moves meanwhile
    let tt = a.g.state;
    a.run(1.0).await;
    assert_eq!(a.g.state, tt);
    assert!(a.g.net_pause.is_some());
    // they're back in the same seat
    let mut b2 = Bot::new(&s.url).await;
    b2.send(ToServer::Resume { v: PROTOCOL, code, token }).await;
    b2.run(0.3).await;
    assert_eq!(b2.id, id);
    assert!(b2.g.pred.last_q > 0 || b2.members.iter().any(|m| m.id == id && !m.away));
    a.run(2.0).await;
    assert!(a.g.net_pause.is_none(), "goes on: {:?}", a.g.net_pause);
    assert!(a.notices.iter().any(|n| n.contains("Bram is back")), "{:?}", a.notices);
    assert!(a.members.iter().all(|m| !m.away));
    assert!(s.logged("reconnected after"), "{}", s.dump());
    assert!(s.logged("connection lost"), "{}", s.dump());
}

#[tokio::test]
async fn silent_player_is_noticed_and_host_moves_on() {
    let s = Server::start(18793);
    let (mut a, mut b) = pair(&s).await;
    a.send(ToServer::Start).await;
    b.run(0.3).await;
    // the host's app goes to sleep: socket open, not a word
    let (id_a, id_b) = (a.id, b.id);
    b.run(4.8).await;
    assert!(b.members.iter().any(|m| m.id == id_a && m.away), "{:?}\n{}", b.members, s.dump());
    assert!(b.g.net_pause.is_some());
    assert!(s.logged("silent for"), "{}", s.dump());
    // the sleeper wakes up, sees it's back and everything is repaired
    a.run(1.0).await;
    b.run(2.0).await;
    assert!(b.members.iter().all(|m| !m.away), "{:?}", b.members);
    assert!(b.g.net_pause.is_none());
    // the host really leaves: the match goes on, Bram gets the lobby
    a.send(ToServer::Leave).await;
    b.run(0.5).await;
    assert_eq!(b.members.len(), 1);
    assert!(b.notices.iter().any(|n| n.contains("Anna left") && n.contains("computer")), "{:?}", b.notices);
    assert!(s.logged(&format!("host is now player={id_b}")), "{}", s.dump());
    let q = b.g.pred.last_q;
    b.run(0.5).await;
    assert!(b.g.pred.last_q > q, "the match still runs");
}

#[tokio::test]
async fn bad_requests_are_told() {
    let s = Server::start(18794);
    let (mut a, mut b) = pair(&s).await;
    // an old game
    let mut old = Bot::new(&s.url).await;
    old.send(ToServer::Host { v: 1, name: "Old".into(), look: [0; 7], me: Default::default(), wear: [0; 8] }).await;
    old.run(0.2).await;
    assert!(old.err.as_deref().is_some_and(|e| e.contains("out of date")), "{:?}", old.err);
    assert!(s.logged("has protocol 1"));
    // a stale seat
    let mut gone = Bot::new(&s.url).await;
    gone.send(ToServer::Resume { v: PROTOCOL, code: "ZZZZ".into(), token: "nope".into() }).await;
    gone.run(0.2).await;
    assert!(gone.closed.is_some());
    // only the host starts; can't join a running match; can't throw someone else's throw
    b.send(ToServer::Start).await;
    b.run(0.2).await;
    assert!(b.err.as_deref().is_some_and(|e| e.contains("Only the host")));
    b.err = None;
    a.send(ToServer::Start).await;
    a.run(0.2).await;
    b.run(0.2).await;
    let mut late = Bot::new(&s.url).await;
    late.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Late".into(), look: [0; 7], me: creds(&s.url, "Late").await, wear: [0; 8] }).await;
    late.run(0.2).await;
    assert!(late.err.as_deref().is_some_and(|e| e.contains("already started")));
    let thrower = if a.g.local_aiming() { &mut a } else { &mut b };
    assert!(thrower.g.local_aiming());
    let i = thrower.throw_spec();
    let other = if a.g.local_aiming() { &mut b } else { &mut a };
    other.input(i).await;
    other.run(0.2).await;
    assert!(other.rejects.iter().any(|w| w.contains("someone else")), "{:?}", other.rejects);
    // garbage and a false digest are noticed, not fatal
    let _ = other.ws.send(Message::Text("not json".into())).await;
    let q = other.g.pred.last_q;
    other.send(ToServer::Hb { c: 0.0, r: 0.05, s: q, d: 12345 }).await;
    other.run(0.3).await;
    assert!(s.logged("unreadable message"), "{}", s.dump());
    assert!(s.logged("DESYNC"), "{}", s.dump());
}

struct Pal {
    ws: Ws,
    code: String,
    secret: String,
    friends: Vec<Friend>,
    requests: Vec<Friend>,
    sent: Vec<Friend>,
    said: Vec<(String, ChatMsg)>,
    history: Vec<ChatMsg>,
    invites: Vec<(String, String)>,
    err: Option<String>,
    user: String,
    name: String,
    progress: Option<(u64, String)>,
    auth_err: Option<String>,
    notes: Vec<Note>,
    latest: Option<u32>,
    coins: Option<u32>,
    owned: Vec<u8>,
    gains: Vec<(i64, String)>,
    board: Option<(u8, Vec<BoardRow>, Option<BoardRow>)>,
}
impl Pal {
    /// Log in with a kept session, or with no code, sign up as `name` (without its spaces).
    async fn login(url: &str, code: &str, secret: &str, name: &str) -> Pal {
        let mut p = Pal::open(url).await;
        if code.is_empty() {
            let user: String = name.chars().filter(|c| !c.is_whitespace()).collect();
            p.auth(ToServer::Register { v: PROTOCOL, user, pass: "longenough".into(), name: name.into() }).await;
            p.run().await;
        } else {
            p.send(ToServer::SHello { v: PROTOCOL, code: code.into(), secret: secret.into(), name: name.into(), only: true }).await;
            p.run().await;
        }
        assert!(!p.code.is_empty(), "no account: {:?}", p.err);
        p
    }
    async fn open(url: &str) -> Pal {
        Pal { ws: open(url).await, code: String::new(), secret: String::new(), friends: vec![], requests: vec![], sent: vec![], said: vec![], history: vec![], invites: vec![], err: None, user: String::new(), name: String::new(), progress: None, auth_err: None, notes: vec![], latest: None, coins: None, owned: vec![], gains: vec![], board: None }
    }
    /// Send a login or sign-up and wait for the answer (hashing takes a moment).
    async fn auth(&mut self, m: ToServer) {
        self.send(m).await;
        let t0 = Instant::now();
        while self.code.is_empty() && self.auth_err.is_none() && t0.elapsed().as_secs() < 5 {
            self.run().await;
        }
    }
    async fn send(&mut self, m: ToServer) {
        self.ws.send(Message::Text(serde_json::to_string(&m).unwrap().into())).await.unwrap();
    }
    /// Take in what the server has to say.
    async fn run(&mut self) {
        let t0 = Instant::now();
        while t0.elapsed().as_millis() < 250 {
            if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(50), self.ws.next()).await {
                match serde_json::from_str::<FromServer>(&t).unwrap() {
                    FromServer::Me { code, secret, user, name } => {
                        self.code = code;
                        self.secret = secret;
                        self.user = user;
                        self.name = name;
                    }
                    FromServer::Progress { rev, data } => self.progress = Some((rev, data)),
                    FromServer::AuthErr { msg } => self.auth_err = Some(msg),
                    FromServer::Friends { friends, requests, sent } => {
                        self.friends = friends;
                        self.requests = requests;
                        self.sent = sent;
                    }
                    FromServer::Said { with, m } => self.said.push((with, m)),
                    FromServer::History { msgs, .. } => self.history = msgs,
                    FromServer::Invited { name, room, .. } => self.invites.push((name, room)),
                    FromServer::Note { note } => self.notes.push(note),
                    FromServer::Latest { build } => self.latest = Some(build),
                    FromServer::Wallet { coins, owned, gain, why } => {
                        self.coins = Some(coins);
                        self.owned = owned;
                        if gain != 0 {
                            self.gains.push((gain, why));
                        }
                    }
                    FromServer::Board { by, rows, me } => self.board = Some((by, rows, me)),
                    FromServer::SocialErr { msg } => self.err = Some(msg),
                    _ => {}
                }
            }
        }
    }
}

/// A phone's background check, over plain HTTP like the app's: (status, body).
async fn notes_check(s: &Server, code: &str, secret: &str, after: u64) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = serde_json::to_string(&NotesReq { code: code.into(), secret: secret.into(), after }).unwrap();
    let mut t = TcpStream::connect(s.url.trim_start_matches("ws://")).await.unwrap();
    t.write_all(format!("POST {NOTES_PATH} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    let mut res = String::new();
    tokio::time::timeout(Duration::from_secs(5), t.read_to_string(&mut res)).await.unwrap().unwrap();
    let st = res.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    (st, res.split_once("\r\n\r\n").map_or(String::new(), |x| x.1.to_string()))
}

#[tokio::test]
async fn friends_from_request_to_invite() {
    let mut s = Server::start(18795);
    let mut a = Pal::login(&s.url, "", "", "Anna").await;
    let mut b = Pal::login(&s.url, "", "", "Bram").await;
    assert_eq!(a.code.len(), CODE_LEN);
    assert_ne!(a.code, b.code);
    assert!(a.secret.len() >= 32);
    // nonsense is turned down
    a.send(ToServer::FriendAdd { code: a.code.clone() }).await;
    a.run().await;
    assert!(a.err.take().unwrap().contains("own code"));
    a.send(ToServer::FriendAdd { code: "NOPE1234".into() }).await;
    a.run().await;
    assert!(a.err.take().unwrap().contains("No player"));
    // no messages or invites before you're friends
    a.send(ToServer::Say { to: b.code.clone(), text: "hi".into() }).await;
    a.run().await;
    assert!(a.err.take().unwrap().contains("not friends"));
    // ask (a sloppily typed code is fine) and accept
    a.send(ToServer::FriendAdd { code: format!(" {} ", b.code.to_lowercase()) }).await;
    a.run().await;
    b.run().await;
    assert_eq!(a.sent.len(), 1);
    assert_eq!(b.requests[0].name, "Anna");
    b.send(ToServer::FriendAdd { code: a.code.clone() }).await;
    b.run().await;
    a.run().await;
    assert!(a.sent.is_empty() && b.requests.is_empty());
    assert_eq!(a.friends[0].name, "Bram");
    assert!(a.friends[0].online);
    // what they're up to
    b.send(ToServer::SStatus { status: STATUS_MATCH }).await;
    a.run().await;
    assert_eq!(a.friends[0].status, STATUS_MATCH);
    // a live message
    a.send(ToServer::Say { to: b.code.clone(), text: "  hoi \u{7}Bram  ".into() }).await;
    b.run().await;
    a.run().await;
    assert_eq!(b.said[0].1.text, "hoi Bram");
    assert_eq!(a.said[0].1.from, a.code, "the sender gets it too");
    assert_eq!(b.friends[0].unread, 1);
    b.send(ToServer::ChatOpen { with: a.code.clone() }).await;
    b.run().await;
    assert_eq!(b.history.len(), 1);
    assert_eq!(b.friends[0].unread, 0, "opening reads it");
    // an invite
    let mut host = Bot::new(&s.url).await;
    host.send(ToServer::Host { v: PROTOCOL, name: "Anna".into(), look: [0; 7], me: (a.code.clone(), a.secret.clone()), wear: [0; 8] }).await;
    host.run(0.2).await;
    a.send(ToServer::Invite { to: b.code.clone(), room: "ZZZZ".into() }).await;
    a.run().await;
    assert!(a.err.take().is_some(), "not to a room that doesn't exist");
    a.send(ToServer::Invite { to: b.code.clone(), room: host.code.clone() }).await;
    b.run().await;
    assert_eq!(b.invites, vec![("Anna".to_string(), host.code.clone())]);
    assert_eq!(b.latest, Some(APP_BUILD), "every connection hears the newest build first");
    // …and as a notification, for a game in the background (the message before it was one too)
    let n = b.notes.last().unwrap();
    assert_eq!((n.kind, n.name.as_str(), n.room.as_str()), (NOTE_INVITE, "Anna", host.code.as_str()));
    assert_eq!(b.notes.iter().filter(|n| n.kind == NOTE_MSG).count(), 1);
    let seen = n.id;
    // Bram goes offline: Anna sees it, and her invite and message wait for him
    let (bc, bs) = (b.code.clone(), b.secret.clone());
    drop(b);
    a.run().await;
    a.run().await;
    assert!(!a.friends[0].online);
    a.send(ToServer::Invite { to: bc.clone(), room: host.code.clone() }).await;
    a.run().await;
    assert!(a.err.is_none(), "offline friends can be invited: {:?}", a.err);
    a.send(ToServer::Say { to: bc.clone(), text: "while you were out".into() }).await;
    a.run().await;
    // his phone's background check finds them, newer than what it has shown
    let (st, r) = notes_check(&s, &bc, &bs, seen).await;
    assert_eq!(st, 200);
    let r: NotesResp = serde_json::from_str(&r).unwrap();
    assert_eq!(r.build, APP_BUILD);
    assert_eq!(r.notes.iter().map(|n| (n.kind, n.text.as_str())).collect::<Vec<_>>(), vec![(NOTE_INVITE, ""), (NOTE_MSG, "while you were out")]);
    assert_eq!(notes_check(&s, &bc, "wrong", 0).await.0, 403, "only with the secret");
    // back in the game, the invite is waiting for him, once
    let b2 = Pal::login(&s.url, &bc, &bs, "Bram").await;
    assert_eq!(b2.invites, vec![("Anna".to_string(), host.code.clone())]);
    drop(b2);
    let b3 = Pal::login(&s.url, &bc, &bs, "Bram").await;
    assert!(b3.invites.is_empty(), "{:?}", b3.invites);
    drop(b3);
    // everything survives a server restart
    let (ac, asec) = (a.code.clone(), a.secret.clone());
    drop(a);
    drop(host);
    tokio::time::sleep(Duration::from_secs(4)).await;
    let _ = s.child.kill();
    let _ = s.child.wait();
    let s2 = Server::start_keeping_data(18795);
    let mut b = Pal::login(&s2.url, &bc, &bs, "Bram").await;
    assert_eq!(b.code, bc, "same account");
    assert_eq!(b.friends[0].code, ac);
    assert_eq!(b.friends[0].unread, 1, "the message waited");
    assert!(!b.friends[0].online);
    b.send(ToServer::ChatOpen { with: ac.clone() }).await;
    b.run().await;
    assert_eq!(b.history.last().unwrap().text, "while you were out");
    // someone else's code without the secret gets nothing
    let mut imp = Pal::open(&s2.url).await;
    imp.send(ToServer::SHello { v: PROTOCOL, code: ac.clone(), secret: "wrong".into(), name: "Eve".into(), only: false }).await;
    imp.run().await;
    assert!(imp.code.is_empty() && imp.auth_err.is_some());
    let a = Pal::login(&s2.url, &ac, &asec, "Anna").await;
    assert_eq!(a.code, ac);
    // unfriending ends it for both
    let mut a = a;
    a.send(ToServer::FriendRemove { code: bc.clone() }).await;
    a.run().await;
    b.run().await;
    assert!(a.friends.is_empty() && b.friends.is_empty());
}

#[tokio::test]
async fn emotes_reach_the_others_only() {
    let s = Server::start(18796);
    let (mut a, mut b) = pair(&s).await;
    a.send(ToServer::Start).await;
    a.run(0.2).await;
    b.run(0.2).await;
    let id = a.id;
    a.send(ToServer::Emote { i: 2 }).await;
    a.send(ToServer::Emote { i: 3 }).await; // too soon after: dropped
    a.send(ToServer::Emote { i: 200 }).await; // nonsense: dropped
    let mut got = vec![];
    let t0 = Instant::now();
    while t0.elapsed().as_millis() < 400 {
        if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(50), b.ws.next()).await {
            if let FromServer::Emote { i, from } = serde_json::from_str::<FromServer>(&t).unwrap() {
                got.push((i, from));
            }
        }
    }
    assert_eq!(got, vec![(2, id)]);
    let mut own = 0;
    let t0 = Instant::now();
    while t0.elapsed().as_millis() < 200 {
        if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(50), a.ws.next()).await {
            if matches!(serde_json::from_str::<FromServer>(&t).unwrap(), FromServer::Emote { .. }) {
                own += 1;
            }
        }
    }
    assert_eq!(own, 0, "the sender has already shown it");
}

#[tokio::test]
async fn matches_count_for_stats_between_friends() {
    let mut s = Server::start(18797);
    // an old data file, from before there were stats, still loads
    let path = std::env::temp_dir().join("flunkiebal-test-18797.json");
    let _ = s.child.kill();
    let _ = s.child.wait();
    std::fs::write(&path, r#"{"accounts":{"OLDONE11":{"secret":"s3cret","name":"Old","user":"old","friends":[],"requests":[],"sent":[],"read":{}},"PHONE111":{"secret":"s3cret","name":"Phone"}},"chats":{}}"#).unwrap();
    s = Server::start_keeping_data(18797);
    let old = Pal::login(&s.url, "OLDONE11", "s3cret", "Old").await;
    assert_eq!(old.code, "OLDONE11", "the old account survives");
    let mut phone = Pal::open(&s.url).await;
    phone.send(ToServer::SHello { v: PROTOCOL, code: "PHONE111".into(), secret: "s3cret".into(), name: "Phone".into(), only: false }).await;
    phone.run().await;
    assert!(phone.code.is_empty() && phone.auth_err.is_some(), "an account without a password is gone");
    let mut pa = Pal::login(&s.url, "", "", "Anna").await;
    let mut pb = Pal::login(&s.url, "", "", "Bram").await;
    pa.send(ToServer::FriendAdd { code: pb.code.clone() }).await;
    pa.run().await;
    pb.send(ToServer::FriendAdd { code: pa.code.clone() }).await;
    pb.run().await;
    pa.run().await;
    // not played yet
    pa.send(ToServer::FriendStats { with: pb.code.clone() }).await;
    let stats = |t: &str| serde_json::from_str::<FromServer>(t).ok();
    let mut got = None;
    for _ in 0..20 {
        if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(100), pa.ws.next()).await {
            if let Some(FromServer::FriendStats { s, .. }) = stats(&t) {
                got = Some(s);
                break;
            }
        }
    }
    assert_eq!(got.unwrap(), PairStats::default());
    // a match against each other
    let mut a = Bot::new(&s.url).await;
    a.send(ToServer::Host { v: PROTOCOL, name: "Anna".into(), look: [0; 7], me: (pa.code.clone(), pa.secret.clone()), wear: [0; 8] }).await;
    a.run(0.2).await;
    let mut b = Bot::new(&s.url).await;
    b.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Bram".into(), look: [0; 7], me: (pb.code.clone(), pb.secret.clone()), wear: [0; 8] }).await;
    b.run(0.2).await;
    a.run(0.1).await;
    a.send(ToServer::Start).await;
    b.run(0.3).await;
    play_out(&s, &mut a, &mut b).await;
    let winner_is_a = a.g.win == a.g.my_team;
    let (at, bt) = (a.g.my_team.unwrap(), b.g.my_team.unwrap());
    let (ah, bh) = (a.g.teams[at].hits, b.g.teams[bt].hits);
    tokio::time::sleep(Duration::from_millis(200)).await;
    // a long match: their friends connections, quiet all along, may have been dropped; log in again
    let mut pa = Pal::login(&s.url, &pa.code, &pa.secret, "Anna").await;
    let mut pb = Pal::login(&s.url, &pb.code, &pb.secret, "Bram").await;
    // both sides see it, each from their own side
    let mut views = vec![];
    let (ca, cb) = (pa.code.clone(), pb.code.clone());
    for (p, other) in [(&mut pa, cb), (&mut pb, ca)] {
        p.send(ToServer::FriendStats { with: other }).await;
        for _ in 0..40 {
            if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(100), p.ws.next()).await {
                if let Some(FromServer::FriendStats { s, .. }) = stats(&t) {
                    views.push(s);
                    break;
                }
            }
        }
    }
    let (va, vb) = (&views[0], &views[1]);
    assert_eq!((va.against, va.together), (1, 0));
    assert_eq!(va.my_wins, winner_is_a as u32);
    assert_eq!(va.their_wins, !winner_is_a as u32);
    assert_eq!((vb.my_wins, vb.their_wins), (va.their_wins, va.my_wins), "mirrored");
    assert_eq!((va.my_hits, va.their_hits), (ah, bh), "hits belong to who threw");
    assert_eq!((vb.my_hits, vb.their_hits), (bh, ah));
    assert_eq!(va.recent[0].won, winner_is_a);
    assert_eq!(vb.recent[0].won, !winner_is_a);
    assert_eq!(va.recent[0].arena, "vondel");
    assert!(va.seconds > 0);
    // someone else can't look
    let mut eve = Pal::login(&s.url, "", "", "Eve").await;
    eve.send(ToServer::FriendStats { with: pa.code.clone() }).await;
    eve.run().await;
    assert!(eve.err.take().unwrap().contains("not friends"));
}

#[tokio::test]
async fn accounts_with_a_password_keep_progress() {
    let mut s = Server::start(18799);
    let reg = |user: &str, pass: &str| ToServer::Register { v: PROTOCOL, user: user.into(), pass: pass.into(), name: "Anna".into() };
    let login = |user: &str, pass: &str| ToServer::Login { v: PROTOCOL, user: user.into(), pass: pass.into(), name: "Anna".into() };
    // what can't be an account
    for (u, p, why) in [("an", "longenough", "3 to 16"), ("anna boom", "longenough", "only letters"), ("anna", "short", "at least 8")] {
        let mut x = Pal::open(&s.url).await;
        x.auth(reg(u, p)).await;
        assert!(x.auth_err.as_deref().unwrap_or("").contains(why), "{u}/{p}: {:?}", x.auth_err);
    }
    let mut a = Pal::open(&s.url).await;
    a.auth(reg("Anna.B", "correct horse")).await;
    assert_eq!(a.user, "anna.b", "filed in lower case");
    assert_eq!(a.name, "Anna.B", "and the player's name, as typed");
    assert_eq!(a.code.len(), CODE_LEN);
    assert_eq!(a.progress, Some((0, String::new())), "nothing kept yet");
    let mut b = Pal::open(&s.url).await;
    b.auth(reg("anna.b", "another one")).await;
    assert!(b.auth_err.unwrap().contains("taken"));
    a.send(ToServer::SProgress { rev: 3, data: r#"{"xp":120}"#.into() }).await;
    a.run().await;
    assert!(a.err.is_none(), "{:?}", a.err);
    // no account by just connecting, whatever the game says
    for only in [false, true] {
        let mut dev = Pal::open(&s.url).await;
        dev.send(ToServer::SHello { v: PROTOCOL, code: String::new(), secret: String::new(), name: "Bram".into(), only }).await;
        dev.run().await;
        assert!(dev.code.is_empty() && dev.auth_err.is_some());
    }
    let mut phone = Pal::login(&s.url, "", "", "Bram").await;
    let phone_code = phone.code.clone();
    assert_eq!(phone.user, "bram");
    // logging in to another account on that connection switches to it
    phone.code.clear();
    phone.auth(ToServer::Login { v: PROTOCOL, user: "anna.b".into(), pass: "correct horse".into(), name: "x".into() }).await;
    assert_eq!(phone.user, "anna.b");
    assert_ne!(phone.code, phone_code);
    // wrong passwords and names nobody has look alike
    for (u, p) in [("anna.b", "wrong password"), ("nobody", "correct horse")] {
        let mut x = Pal::open(&s.url).await;
        x.auth(login(u, p)).await;
        assert_eq!(x.auth_err.as_deref(), Some("Wrong username or password"));
        assert!(x.code.is_empty());
    }
    // somewhere else, the same account and its progress; survives a restart
    let (code, secret) = (a.code.clone(), a.secret.clone());
    drop(a);
    tokio::time::sleep(Duration::from_secs(4)).await;
    let _ = s.child.kill();
    let _ = s.child.wait();
    let s2 = Server::start_keeping_data(18799);
    let mut a2 = Pal::open(&s2.url).await;
    a2.auth(login(" ANNA.B ", "correct horse")).await;
    assert_eq!(a2.code, code);
    assert_eq!(a2.progress, Some((3, r#"{"xp":120}"#.into())));
    // a kept session comes back with the secret, and only then
    let mut k = Pal::open(&s2.url).await;
    k.send(ToServer::SHello { v: PROTOCOL, code: code.clone(), secret: secret.clone(), name: "Anna".into(), only: true }).await;
    k.run().await;
    assert_eq!((k.code.as_str(), k.user.as_str()), (code.as_str(), "anna.b"));
    assert!(k.progress.is_some());
    let mut k2 = Pal::open(&s2.url).await;
    k2.send(ToServer::SHello { v: PROTOCOL, code: code.clone(), secret: "wrong".into(), name: "Anna".into(), only: true }).await;
    k2.run().await;
    assert!(k2.code.is_empty() && k2.auth_err.is_some(), "no new account for a session that's gone");
    // too much is too much
    k.send(ToServer::SProgress { rev: 4, data: "x".repeat(MAX_PROGRESS + 1) }).await;
    k.run().await;
    assert!(k.err.unwrap().contains("too big"));
    // guessing stops after a while
    let mut g = Pal::open(&s2.url).await;
    for _ in 0..AUTH_TRIES {
        g.auth_err = None;
        g.auth(login("anna.b", "guess")).await;
    }
    assert!(g.auth_err.unwrap().contains("wait"), "too many tries from one connection");
    assert!(s2.logged("logged in as user anna.b"), "{}", s2.dump());
}
/// More than a connection may try in a minute.
const AUTH_TRIES: usize = 11;

#[tokio::test]
async fn bug_reports_are_kept() {
    let s = Server::start(18800);
    let path = std::env::temp_dir().join("flunkiebal-bugs.jsonl");
    let mut p = Pal::open(&s.url).await;
    let said = |t: &str| serde_json::from_str::<FromServer>(t).ok();
    let mut answers = vec![];
    for text in ["", "the ball went sideways 18800", "two", "three", "four"] {
        p.send(ToServer::Bug { v: PROTOCOL, text: text.into(), info: "build 10, web".into() }).await;
        for _ in 0..20 {
            if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(100), p.ws.next()).await {
                if let Some(FromServer::BugSaved { ok, .. }) = said(&t) {
                    answers.push(ok);
                    break;
                }
            }
        }
    }
    assert_eq!(answers, vec![false, true, true, true, false], "nothing to say, then three, then wait a while");
    let kept = std::fs::read_to_string(&path).unwrap();
    let line = kept.lines().find(|l| l.contains("sideways 18800")).expect("kept");
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["info"], "build 10, web");
    assert!(s.logged("bug report from"), "{}", s.dump());
    let _ = std::fs::remove_file(&path);
}

/// Not part of the suite: a bot that joins a room on a running server and plays, to test a phone
/// against. `FLUNKI_URL=ws://127.0.0.1:8790 FLUNKI_JOIN=ABCD FLUNKI_SECS=300 cargo test --release
/// --test online bot_for_a_phone -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn bot_for_a_phone() {
    let url = std::env::var("FLUNKI_URL").unwrap_or_else(|_| "ws://127.0.0.1:8787".into());
    let code = std::env::var("FLUNKI_JOIN").expect("FLUNKI_JOIN=<room code>");
    let secs: u64 = std::env::var("FLUNKI_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let mut b = Bot::new(&url).await;
    let me = (std::env::var("FLUNKI_CODE").unwrap_or_default(), std::env::var("FLUNKI_SECRET").unwrap_or_default());
    b.send(ToServer::Join { v: PROTOCOL, code, name: "Bot".into(), look: [0; 7], me, wear: [0; 8] }).await;
    b.run(0.5).await;
    println!("joined as {} ({:?})", b.id, b.err);
    let t0 = Instant::now();
    let mut foot = 0;
    let mut last = (u32::MAX, State::Menu);
    while t0.elapsed().as_secs() < secs {
        b.run(0.05).await;
        let g = &b.g;
        if (g.pred.match_id, g.state) != last {
            last = (g.pred.match_id, g.state);
            println!("{:>6.1}s match={} state={:?} turn={} throws={:?} beers={:?}/{:?} rejects={:?}", t0.elapsed().as_secs_f32(), g.pred.match_id, g.state, g.turn, [g.teams[0].throws, g.teams[1].throws], g.teams[0].beers, g.teams[1].beers, b.rejects);
        }
        let Some(me) = g.my_team else { continue };
        let d = &g.teams[me].dc[g.teams[me].lead(g.my_players[me]).unwrap_or(0)];
        let at = t0.elapsed().as_secs_f64() * 1000.0;
        if g.local_aiming() {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let i = b.throw_spec();
            b.input(i).await;
        } else if g.can_drink(me) && !d.hold && d.cough <= 0.0 && d.p < 0.3 {
            b.input(Input::Drink { hold: true, at }).await;
        } else if d.hold && d.p > 0.6 {
            b.input(Input::Drink { hold: false, at }).await;
        } else if g.can_run(me) {
            foot = 1 - foot;
            b.input(Input::Pad { foot, at }).await;
        }
    }
}

#[tokio::test]
async fn coins_bets_shop_and_leaderboard() {
    let s = Server::start(18801);
    let mut pa = Pal::login(&s.url, "", "", "Anna").await;
    let mut pb = Pal::login(&s.url, "", "", "Bram").await;
    assert_eq!(pa.coins, Some(START_COINS), "coins to start with");
    // too dear, and not for sale
    pa.send(ToServer::Buy { item: 10 }).await;
    pa.run().await;
    assert!(pa.err.take().unwrap().contains("costs"));
    pa.send(ToServer::Buy { item: 2 }).await;
    pa.run().await;
    assert!(pa.err.take().unwrap().contains("not for sale"));
    pa.send(ToServer::Board { by: BOARD_RATING }).await;
    pa.run().await;
    let (_, rows, me) = pa.board.take().unwrap();
    assert!(rows.is_empty() && me.is_none(), "nobody has played yet");
    // a room with a bet
    let mut a = Bot::new(&s.url).await;
    a.send(ToServer::Host { v: PROTOCOL, name: "Anna".into(), look: [0; 7], me: (pa.code.clone(), pa.secret.clone()), wear: [0; 8] }).await;
    a.run(0.2).await;
    let mut b = Bot::new(&s.url).await;
    b.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Bram".into(), look: [0; 7], me: (pb.code.clone(), pb.secret.clone()), wear: [0; 8] }).await;
    b.run(0.2).await;
    // more than anyone has
    a.send(ToServer::Config { arena: "vondel".into(), diff: 1, stake: 250 }).await;
    a.run(0.1).await;
    a.send(ToServer::Start).await;
    a.run(0.3).await;
    assert_eq!(a.phase, PHASE_LOBBY);
    assert!(a.notices.iter().any(|n| n.contains("hasn't got 250 coins")), "{:?}", a.notices);
    // a stake that isn't on the list is ignored
    a.send(ToServer::Config { arena: "vondel".into(), diff: 1, stake: 33 }).await;
    a.send(ToServer::Config { arena: "vondel".into(), diff: 1, stake: 50 }).await;
    a.send(ToServer::Start).await;
    b.run(0.3).await;
    assert_eq!(b.phase, PHASE_PLAYING);
    pa.run().await;
    assert_eq!(pa.coins, Some(START_COINS - 50), "the bet is taken at kickoff");
    play_out(&s, &mut a, &mut b).await;
    let a_won = a.g.win == a.g.my_team;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut pa = Pal::login(&s.url, &pa.code, &pa.secret, "Anna").await;
    let mut pb = Pal::login(&s.url, &pb.code, &pb.secret, "Bram").await;
    let (w, l) = if a_won { (&mut pa, &mut pb) } else { (&mut pb, &mut pa) };
    assert_eq!(w.coins, Some(START_COINS - 50 + 100 + WIN_COINS), "the pot and the win\n{}", s.dump());
    assert_eq!(l.coins, Some(START_COINS - 50 + LOSS_COINS));
    // the leaderboards
    for by in [BOARD_RATING, BOARD_WINS, BOARD_COINS, BOARD_HITS] {
        w.send(ToServer::Board { by }).await;
        w.run().await;
        let (got, rows, me) = w.board.take().unwrap();
        assert_eq!(got, by);
        assert_eq!(rows.len(), 2);
        assert_eq!(me.as_ref().unwrap().code, w.code);
        if by != BOARD_HITS {
            assert_eq!(rows[0].code, w.code, "the winner leads board {by}");
        }
    }
    let (_, rows, _) = { l.send(ToServer::Board { by: BOARD_RATING }).await; l.run().await; l.board.take().unwrap() };
    assert!(rows[0].rating > START_RATING as i32 && rows[1].rating < START_RATING as i32, "{rows:?}");
    assert_eq!((rows[0].played, rows[0].wins, rows[1].wins), (1, 1, 0));
    // accuracy wants more throws first
    l.send(ToServer::Board { by: BOARD_ACCURACY }).await;
    l.run().await;
    assert!(l.board.take().unwrap().1.is_empty());
    // shopping with the winnings
    w.send(ToServer::Buy { item: 6 }).await;
    w.run().await;
    assert_eq!(w.owned, vec![6]);
    assert_eq!(w.coins, Some(START_COINS - 50 + 100 + WIN_COINS - 150));
    w.send(ToServer::Buy { item: 6 }).await;
    w.run().await;
    assert!(w.err.take().unwrap().contains("already"));
}

#[tokio::test]
async fn bets_need_accounts_and_are_refunded_when_called_off() {
    let s = Server::start(18802);
    let pa = Pal::login(&s.url, "", "", "Anna").await;
    let mut pb = Pal::login(&s.url, "", "", "Bram").await;
    let mut a = Bot::new(&s.url).await;
    a.send(ToServer::Host { v: PROTOCOL, name: "Anna".into(), look: [0; 7], me: (pa.code.clone(), pa.secret.clone()), wear: [0; 8] }).await;
    a.run(0.2).await;
    // nobody plays online without logging in
    let mut guest = Bot::new(&s.url).await;
    guest.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Guest".into(), look: [0; 7], me: Default::default(), wear: [0; 8] }).await;
    guest.run(0.2).await;
    assert_eq!(guest.err.as_deref(), Some("Log in to play online"));
    guest.err = None;
    guest.send(ToServer::Host { v: PROTOCOL, name: "Guest".into(), look: [0; 7], me: (pa.code.clone(), "wrong".into()), wear: [0; 8] }).await;
    guest.run(0.2).await;
    assert_eq!(guest.err.as_deref(), Some("Log in to play online"));
    // a bet needs both teams
    a.send(ToServer::Config { arena: "vondel".into(), diff: 1, stake: 10 }).await;
    a.send(ToServer::Start).await;
    a.run(0.3).await;
    assert_eq!(a.phase, PHASE_LOBBY);
    assert!(a.notices.iter().any(|n| n.contains("both teams")), "{:?}", a.notices);
    let mut b = Bot::new(&s.url).await;
    b.send(ToServer::Join { v: PROTOCOL, code: a.code.clone(), name: "Bram".into(), look: [0; 7], me: (pb.code.clone(), pb.secret.clone()), wear: [0; 8] }).await;
    b.run(0.2).await;
    a.send(ToServer::Start).await;
    b.run(0.3).await;
    assert_eq!(b.phase, PHASE_PLAYING);
    pb.run().await;
    assert_eq!(pb.coins, Some(START_COINS - 10));
    // everyone goes: the room closes after a while and the bets come back
    a.send(ToServer::Leave).await;
    b.send(ToServer::Leave).await;
    a.run(0.2).await;
    let t0 = Instant::now();
    while !s.logged("bets called off") {
        assert!(t0.elapsed().as_secs() < 5, "{}", s.dump());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pb = Pal::login(&s.url, &pb.code, &pb.secret, "Bram").await;
    assert_eq!(pb.coins, Some(START_COINS));
}
