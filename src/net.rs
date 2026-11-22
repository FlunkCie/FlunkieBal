//! The connection to the server. The match itself runs there (see `flunkiebal_core::proto` and
//! `server/`); this keeps the socket alive, measures it, gets back in when it breaks and says how it's going.

pub use flunkiebal_core::proto::*;

// ---------- the socket ----------
#[cfg(target_arch = "wasm32")]
mod sock {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    pub struct Socket {
        ws: web_sys::WebSocket,
        inbox: Rc<RefCell<VecDeque<(String, f64)>>>,
        state: Rc<RefCell<u8>>, // 0 connecting, 1 open, 2 closed
        _cbs: Vec<Closure<dyn FnMut(JsValue)>>,
        /// when we last sent anything, for the keepalive
        sent: Rc<std::cell::Cell<f64>>,
        timer: Option<(i32, Closure<dyn FnMut()>)>,
    }
    impl Socket {
        pub fn connect(url: &str) -> Result<Socket, String> {
            let ws = web_sys::WebSocket::new(url).map_err(|e| format!("{e:?}"))?;
            let inbox = Rc::new(RefCell::new(VecDeque::new()));
            let state = Rc::new(RefCell::new(0u8));
            let (ib, st1, st2, st3) = (inbox.clone(), state.clone(), state.clone(), state.clone());
            let on_msg = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                if let Ok(e) = e.dyn_into::<web_sys::MessageEvent>() {
                    if let Some(s) = e.data().as_string() {
                        // stamped on arrival: a slow frame says nothing about the connection
                        ib.borrow_mut().push_back((s, js_sys::Date::now()));
                    }
                }
            });
            let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_| *st1.borrow_mut() = 1);
            let on_close = Closure::<dyn FnMut(JsValue)>::new(move |_| *st2.borrow_mut() = 2);
            let on_err = Closure::<dyn FnMut(JsValue)>::new(move |_| *st3.borrow_mut() = 2);
            ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
            ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
            ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
            Ok(Socket { ws, inbox, state, _cbs: vec![on_msg, on_open, on_close, on_err], sent: Rc::new(std::cell::Cell::new(js_sys::Date::now())), timer: None })
        }
        /// Say we're here once a second on a browser timer, whenever the game itself is quiet: a tab in
        /// the background draws no frames, and a long frame shouldn't look like a dead connection.
        pub fn keepalive(&mut self) {
            if self.timer.is_some() {
                return;
            }
            let (ws, st, sent) = (self.ws.clone(), self.state.clone(), self.sent.clone());
            let cb = Closure::<dyn FnMut()>::new(move || {
                let now = js_sys::Date::now();
                if *st.borrow() == 1 && now - sent.get() > super::KEEPALIVE_MS {
                    sent.set(now);
                    let _ = ws.send_with_str(super::KEEPALIVE);
                }
            });
            let id = web_sys::window().and_then(|w| w.set_interval_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), 1000).ok());
            if let Some(id) = id {
                self.timer = Some((id, cb));
            }
        }
        pub fn state(&self) -> u8 {
            *self.state.borrow()
        }
        pub fn send(&self, s: &str) {
            if self.state() == 1 {
                self.sent.set(js_sys::Date::now());
                let _ = self.ws.send_with_str(s);
            }
        }
        pub fn recv(&self) -> Option<(String, f64)> {
            self.inbox.borrow_mut().pop_front()
        }
        pub fn close(&self) {
            *self.state.borrow_mut() = 2;
            let _ = self.ws.close();
        }
    }
    impl Drop for Socket {
        fn drop(&mut self) {
            if let (Some((id, _)), Some(w)) = (self.timer.as_ref(), web_sys::window()) {
                w.clear_interval_with_handle(*id);
            }
            // the browser may still fire events after this; they must not reach the dropped closures
            self.ws.set_onmessage(None);
            self.ws.set_onopen(None);
            self.ws.set_onclose(None);
            self.ws.set_onerror(None);
            let _ = self.ws.close();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod sock {
    use std::collections::VecDeque;
    use std::io::ErrorKind;
    use std::net::{TcpStream, ToSocketAddrs};
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    use tungstenite::{client_tls_with_config, Message};

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    /// How long the socket thread waits for news before it looks at what there is to send.
    const POLL: Duration = Duration::from_millis(2);

    /// A websocket on its own thread, so a slow network never holds up a frame.
    pub struct Socket {
        out: mpsc::Sender<String>,
        inbox: Arc<Mutex<VecDeque<(String, f64)>>>,
        state: Arc<AtomicU8>, // 0 connecting, 1 open, 2 closed
        stop: Arc<AtomicBool>,
        keep: Arc<AtomicBool>,
    }
    fn now_ms() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
    }
    fn gone(e: &tungstenite::Error) -> bool {
        !matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
    }
    fn run(url: &str, rx: &mpsc::Receiver<String>, inbox: &Mutex<VecDeque<(String, f64)>>, state: &AtomicU8, stop: &AtomicBool, keep: &AtomicBool) -> Result<(), String> {
        let uri: tungstenite::http::Uri = url.parse().map_err(|e| format!("{e}"))?;
        let host = uri.host().ok_or("no host in the address")?;
        let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("wss") { 443 } else { 80 });
        let addrs = (host, port).to_socket_addrs().map_err(|e| format!("{e}"))?;
        let tcp = addrs.into_iter().find_map(|a| TcpStream::connect_timeout(&a, CONNECT_TIMEOUT).ok()).ok_or("can't reach the server")?;
        tcp.set_nodelay(true).map_err(|e| format!("{e}"))?;
        tcp.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(|e| format!("{e}"))?;
        tcp.set_write_timeout(Some(CONNECT_TIMEOUT)).map_err(|e| format!("{e}"))?;
        let probe = tcp.try_clone().map_err(|e| format!("{e}"))?;
        let (mut ws, _) = client_tls_with_config(url, tcp, None, None).map_err(|e| format!("{e}"))?;
        probe.set_read_timeout(Some(POLL)).map_err(|e| format!("{e}"))?;
        state.store(1, Ordering::Relaxed);
        let mut sent = std::time::Instant::now();
        while !stop.load(Ordering::Relaxed) {
            // the game is quiet (a long frame, a minimised window): say we're here
            if keep.load(Ordering::Relaxed) && sent.elapsed().as_secs_f64() * 1000.0 > super::KEEPALIVE_MS {
                sent = std::time::Instant::now();
                if let Err(e) = ws.send(Message::Text(super::KEEPALIVE.into())) {
                    if gone(&e) {
                        return Err(format!("{e}"));
                    }
                }
            }
            while let Ok(m) = rx.try_recv() {
                sent = std::time::Instant::now();
                // a full socket keeps the message queued; `flush` below carries on with it
                if let Err(e) = ws.send(Message::Text(m.into())) {
                    if gone(&e) {
                        return Err(format!("{e}"));
                    }
                }
            }
            if let Err(e) = ws.flush() {
                if gone(&e) {
                    return Err(format!("{e}"));
                }
            }
            match ws.read() {
                Ok(Message::Text(t)) => {
                    // a game in the background draws no frames, so nobody reads the inbox: a phone
                    // shows its notification from here, at once
                    #[cfg(target_os = "android")]
                    if t.starts_with(r#"{"t":"note""#) {
                        crate::android::note_json(&t);
                    }
                    // stamped on arrival: a slow frame says nothing about the connection
                    if let Ok(mut q) = inbox.lock() {
                        q.push_back((t.to_string(), now_ms()));
                    }
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(e) if gone(&e) => return Err(format!("{e}")),
                Err(_) => {}
            }
        }
        let _ = ws.close(None);
        let _ = ws.flush();
        Ok(())
    }
    impl Socket {
        pub fn connect(url: &str) -> Result<Socket, String> {
            let (out, rx) = mpsc::channel();
            let inbox = Arc::new(Mutex::new(VecDeque::new()));
            let state = Arc::new(AtomicU8::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let keep = Arc::new(AtomicBool::new(false));
            let (url, ib, st, sp, kp) = (url.to_string(), inbox.clone(), state.clone(), stop.clone(), keep.clone());
            std::thread::Builder::new()
                .name("flunki-socket".into())
                .spawn(move || {
                    if let Err(e) = run(&url, &rx, &ib, &st, &sp, &kp) {
                        if !sp.load(Ordering::Relaxed) {
                            crate::app::log_str(&format!("websocket {url}: {e}"));
                        }
                    }
                    st.store(2, Ordering::Relaxed);
                })
                .map_err(|e| format!("{e}"))?;
            Ok(Socket { out, inbox, state, stop, keep })
        }
        /// Say we're here once a second from the socket's thread whenever the game itself is quiet.
        pub fn keepalive(&mut self) {
            self.keep.store(true, Ordering::Relaxed);
        }
        pub fn state(&self) -> u8 {
            self.state.load(Ordering::Relaxed)
        }
        pub fn send(&self, s: &str) {
            if self.state() == 1 {
                let _ = self.out.send(s.to_string());
            }
        }
        pub fn recv(&self) -> Option<(String, f64)> {
            self.inbox.lock().ok()?.pop_front()
        }
        pub fn close(&self) {
            self.state.store(2, Ordering::Relaxed);
            self.stop.store(true, Ordering::Relaxed);
        }
    }
    impl Drop for Socket {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::net::TcpListener;
        use std::time::Instant;

        fn wait(what: &str, mut f: impl FnMut() -> bool) {
            let t = Instant::now();
            while !f() {
                assert!(t.elapsed().as_secs() < 5, "timed out: {what}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        /// An echo server that hangs up when told "bye".
        fn echo() -> u16 {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for s in l.incoming().flatten() {
                    let mut ws = tungstenite::accept(s).unwrap();
                    while let Ok(m) = ws.read() {
                        if m.to_text().unwrap_or("") == "bye" {
                            break;
                        }
                        if m.is_text() {
                            ws.send(m).unwrap();
                        }
                    }
                }
            });
            port
        }
        #[test]
        fn talks_and_notices_the_end() {
            let port = echo();
            let s = Socket::connect(&format!("ws://127.0.0.1:{port}")).unwrap();
            wait("open", || s.state() == 1);
            for i in 0..50 {
                s.send(&format!("m{i}"));
            }
            for i in 0..50 {
                let mut got = None;
                wait("echo", || {
                    got = s.recv();
                    got.is_some()
                });
                assert_eq!(got.unwrap().0, format!("m{i}"), "in order");
            }
            s.send("bye");
            wait("noticed the hang-up", || s.state() == 2);
        }
        #[test]
        fn a_bad_address_ends_in_closed() {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            drop(l);
            let s = Socket::connect(&format!("ws://127.0.0.1:{port}")).unwrap();
            wait("refused", || s.state() == 2);
            let bad = Socket::connect("not a url").unwrap();
            wait("unparseable", || bad.state() == 2);
        }
        /// Reaching the TLS code at all: this panicked when no crypto backend was selected.
        #[test]
        fn tls_is_set_up() {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            std::thread::spawn(move || {
                let _ = l.accept().map(|(s, _)| drop(s));
            });
            let s = Socket::connect(&format!("wss://127.0.0.1:{port}")).unwrap();
            wait("a failed handshake, not a dead thread", || s.state() == 2);
        }
        /// The real thing, over the internet: `cargo test --lib live_wss -- --ignored --nocapture`.
        #[test]
        #[ignore]
        fn live_wss() {
            let s = Socket::connect("wss://flunkiebal.flunkcie.nl").unwrap();
            wait("open", || s.state() == 1);
            s.send(r#"{"t":"list"}"#);
            let mut got = None;
            wait("an answer", || {
                got = s.recv();
                got.is_some()
            });
            println!("server said: {}", got.unwrap().0);
        }
        #[test]
        fn close_is_quick_and_final() {
            let port = echo();
            let s = Socket::connect(&format!("ws://127.0.0.1:{port}")).unwrap();
            wait("open", || s.state() == 1);
            s.close();
            assert_eq!(s.state(), 2);
            s.send("ignored");
        }
    }
}
pub use sock::Socket;

/// What the socket sends by itself when the game has been quiet this long: a heartbeat without a time
/// (its `Pong` measures nothing) or a snapshot number (nothing to check).
pub const KEEPALIVE: &str = r#"{"t":"hb","c":0,"r":0,"s":0,"d":0}"#;
pub const KEEPALIVE_MS: f64 = 1500.0;

/// Nothing at all from the server for this long while connected: the connection is dead, whatever the socket says.
pub const DEAD_AFTER: f64 = 5.0;
/// Trying to reconnect this long; after that the seat is given up.
pub const RECONNECT_FOR: f64 = 60.0;
const CONNECT_TIMEOUT: f64 = 6.0;

pub struct Reconnect {
    pub since: f64,
    pub next: f64,
    pub tries: u32,
    pub started: f64,
}

/// How the connection is doing, for the screen.
#[derive(Clone, PartialEq, Debug)]
pub enum Link {
    Good,
    Slow,
    /// snapshots have stopped coming for a moment
    Stalled,
    Reconnecting,
}

/// A connection to the server plus the room we are in.
pub struct Net {
    pub sock: Socket,
    pub url: String,
    pub id: u32,
    pub code: String,
    pub token: String,
    pub in_room: bool,
    pub host_id: u32,
    pub phase: u8,
    pub rooms: Vec<RoomInfo>,
    pub members: Vec<Member>,
    pub arena: String,
    pub diff: u8,
    pub open: bool,
    pub ping_t: f64,
    pub pending: Vec<ToServer>,
    /// round trip, seconds; 0 until measured
    pub rtt: f32,
    pub last_rx: f64,
    pub opened: f64,
    pub reconnect: Option<Reconnect>,
    /// everyone's round trip (ms) and connection state, from the server
    pub status: Vec<(u32, u16, u8)>,
}

impl Net {
    pub fn connect(url: &str, now: f64) -> Result<Net, String> {
        let url = url.trim();
        let mut sock = Socket::connect(url)?;
        sock.keepalive();
        Ok(Net { sock, url: url.into(), id: 0, code: String::new(), token: String::new(), in_room: false, host_id: 0, phase: PHASE_LOBBY, rooms: Vec::new(), members: Vec::new(), arena: "vondel".into(), diff: 1, open: false, ping_t: -10.0, pending: Vec::new(), rtt: 0.0, last_rx: now, opened: now, reconnect: None, status: Vec::new() })
    }
    /// Out of the room; the connection stays for the next one.
    pub fn leave_room(&mut self) {
        self.in_room = false;
        self.code.clear();
        self.token.clear();
        self.id = 0;
        self.host_id = 0;
        self.members.clear();
        self.status.clear();
        self.phase = PHASE_LOBBY;
        self.open = false;
    }
    pub fn host(&self) -> bool {
        self.in_room && self.host_id == self.id
    }
    pub fn send(&mut self, m: ToServer) {
        if self.sock.state() == 1 {
            if let Ok(s) = serde_json::to_string(&m) {
                self.sock.send(&s);
            }
        } else if self.pending.len() < 8 && !matches!(m, ToServer::Input { .. } | ToServer::Hb { .. }) {
            // inputs are only worth anything right now
            self.pending.push(m);
        }
    }
    pub fn flush(&mut self) {
        if self.sock.state() == 1 && !self.pending.is_empty() {
            for m in std::mem::take(&mut self.pending) {
                self.send(m);
            }
        }
    }
    pub fn me(&self) -> Option<&Member> {
        self.members.iter().find(|m| m.id == self.id)
    }
    pub fn humans(&self, t: u8) -> Vec<&Member> {
        team_of(&self.members, t)
    }
    pub fn owner(&self, t: usize, i: usize) -> Option<&Member> {
        owner_of(&self.members, t, i)
    }
    pub fn name_of(&self, id: u32) -> String {
        self.members.iter().find(|m| m.id == id).map_or("Someone".into(), |m| m.name.clone())
    }
    pub fn link(&self, snap_age: f32, playing: bool) -> Link {
        if self.reconnect.is_some() {
            Link::Reconnecting
        } else if playing && snap_age > 0.8 {
            Link::Stalled
        } else if self.rtt > 0.25 {
            Link::Slow
        } else {
            Link::Good
        }
    }
    /// Keep trying to get back in. Returns false when it's time to give up.
    pub fn try_reconnect(&mut self, now: f64) -> bool {
        let Some(r) = self.reconnect.as_mut() else { return true };
        if now - r.since > RECONNECT_FOR {
            return false;
        }
        let st = self.sock.state();
        if st == 1 {
            return true;
        }
        if st == 0 && now - r.started < CONNECT_TIMEOUT {
            return true;
        }
        if now < r.next {
            return true;
        }
        self.sock.close();
        r.tries += 1;
        r.started = now;
        r.next = now + (0.4 * 2f64.powi(r.tries.min(4) as i32)).min(5.0);
        if let Ok(s) = Socket::connect(&self.url) {
            self.sock = s;
            self.pending.clear();
            self.pending.push(ToServer::Resume { v: PROTOCOL, code: self.code.clone(), token: self.token.clone() });
        }
        true
    }
}
