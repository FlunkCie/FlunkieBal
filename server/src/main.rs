//! FlunkieBal server.
//!
//! Every online match runs here: the same `Game` as offline, at 60 Hz, as the one true state. Phones
//! send what their players do and draw the snapshots they get. It keeps rooms with a four-letter
//! code, keeps a seat when someone's connection drops, and logs what is going on.
//! Run it with `flunkiebal-server [port]` (default 8787). The game connects to wss://flunkiebal.flunkcie.nl,
//! a TLS proxy in front of this (the desktop build takes `FLUNKI_SERVER=ws://<machine>:<port>` for testing).
//! One plain HTTP request is served on the same port: a phone's background check for notifications
//! (`POST /api/notes`), made while the game and its sockets are closed.

mod hub;
mod log;
mod room;
mod social;

use futures_util::{SinkExt, StreamExt};
use hub::{Event, Hub};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);
/// A socket that takes this long to accept one write is dead.
const WRITE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_MSG: usize = 8 * 1024;

/// A websocket starts with `GET`; the background check is a `POST`.
async fn is_post(stream: &TcpStream) -> bool {
    let mut b = [0u8; 5];
    for _ in 0..50 {
        match stream.peek(&mut b).await {
            Ok(n) if n >= 5 => return &b == b"POST ",
            Ok(0) | Err(_) => return false,
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    false
}

/// `POST /api/notes` with a `NotesReq`: the notes for that account, as a `NotesResp`.
async fn http(mut stream: TcpStream, addr: SocketAddr, hub: mpsc::UnboundedSender<Event>) {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let read = async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                let len = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok())).unwrap_or(0);
                if len > MAX_MSG {
                    return None;
                }
                while buf.len() < end + 4 + len {
                    let n = stream.read(&mut chunk).await.ok()?;
                    if n == 0 {
                        return None;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                return Some((head, String::from_utf8_lossy(&buf[end + 4..end + 4 + len]).to_string()));
            }
            if buf.len() > MAX_MSG {
                return None;
            }
        }
    };
    let Ok(Some((head, body))) = tokio::time::timeout(Duration::from_secs(10), read).await else { return };
    let path = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
    let (status, answer) = match serde_json::from_str::<flunkiebal_core::proto::NotesReq>(&body) {
        Ok(req) if path == flunkiebal_core::proto::NOTES_PATH => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = hub.send(Event::Notes { req, reply: tx });
            match tokio::time::timeout(Duration::from_secs(5), rx).await {
                Ok(Ok(Some(json))) => ("200 OK", json),
                Ok(Ok(None)) => ("403 Forbidden", "{}".to_string()),
                _ => ("503 Service Unavailable", "{}".to_string()),
            }
        }
        _ => {
            debug!("{addr} unknown HTTP request: {}", head.lines().next().unwrap_or(""));
            ("404 Not Found", "{}".to_string())
        }
    };
    let res = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{answer}", answer.len());
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.write_all(res.as_bytes())).await;
    let _ = stream.shutdown().await;
}

async fn serve(stream: TcpStream, addr: SocketAddr, hub: mpsc::UnboundedSender<Event>) {
    if is_post(&stream).await {
        return http(stream, addr, hub).await;
    }
    let ws = match tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::accept_async(stream)).await {
        Ok(Ok(ws)) => ws,
        Ok(Err(e)) => {
            debug!("{addr} websocket handshake failed: {e}");
            return;
        }
        Err(_) => {
            debug!("{addr} websocket handshake timed out");
            return;
        }
    };
    let id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut src) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let (snap_tx, mut snap_rx) = watch::channel(String::new());
    let conn = room::Conn { tx, snap: snap_tx, id };
    let _ = hub.send(Event::Open { conn, addr });
    let hub2 = hub.clone();
    // Messages go out in order; of the snapshots only the newest, so a slow phone skips instead of lagging.
    let writer = tokio::spawn(async move {
        loop {
            let m = tokio::select! {
                biased;
                m = rx.recv() => match m { Some(m) => m, None => break },
                r = snap_rx.changed() => {
                    if r.is_err() { break }
                    snap_rx.borrow_and_update().clone()
                }
            };
            match tokio::time::timeout(WRITE_TIMEOUT, sink.send(Message::Text(m.into()))).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break,
                Err(_) => {
                    let _ = hub2.send(Event::Stuck { conn: id });
                    break;
                }
            }
        }
    });
    let mut why = "closed by the player";
    loop {
        match src.next().await {
            Some(Ok(Message::Text(t))) => {
                // only progression may be longer
                let max = if t.starts_with(r#"{"t":"sprogress""#) {
                    flunkiebal_core::proto::MAX_PROGRESS + 1024
                } else if t.starts_with(r#"{"t":"bug""#) {
                    // what the player wrote can be in any script, and the game's log lines are escaped
                    4 * (flunkiebal_core::proto::BUG_TEXT + flunkiebal_core::proto::BUG_INFO)
                } else {
                    MAX_MSG
                };
                if t.len() > max {
                    why = "sent an oversized message";
                    break;
                }
                let _ = hub.send(Event::Text { conn: id, text: t.to_string() });
            }
            Some(Ok(Message::Close(_))) | None => break,
            Some(Err(_)) => {
                why = "socket error";
                break;
            }
            _ => {}
        }
    }
    let _ = hub.send(Event::Close { conn: id, why });
    writer.abort();
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok())).unwrap_or(8787);
    let listener = TcpListener::bind(("0.0.0.0", port)).await.expect("bind");
    info!("FlunkieBal server listening on ws://0.0.0.0:{port} (protocol {})", flunkiebal_core::proto::PROTOCOL);
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(Hub::new(tx.clone()).run(rx));
    while let Ok((stream, addr)) = listener.accept().await {
        let _ = stream.set_nodelay(true);
        tokio::spawn(serve(stream, addr, tx.clone()));
    }
}
