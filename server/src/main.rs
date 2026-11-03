//! FlunkieBal server.
//!
//! Every online match runs here: the same `Game` as offline, at 60 Hz, as the one true state. Phones
//! send what their players do and draw the snapshots they get. It keeps rooms with a four-letter
//! code, keeps a seat when someone's connection drops, and logs what is going on.
//! Run it with `flunkiebal-server [port]` (default 8787). The game connects to wss://flunkiebal.flunkcie.nl,
//! a TLS proxy in front of this (the desktop build takes `FLUNKI_SERVER=ws://<machine>:<port>` for testing).

mod hub;
mod log;
mod room;
mod social;

use futures_util::{SinkExt, StreamExt};
use hub::{Event, Hub};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);
/// A socket that takes this long to accept one write is dead.
const WRITE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_MSG: usize = 8 * 1024;

async fn serve(stream: TcpStream, addr: SocketAddr, hub: mpsc::UnboundedSender<Event>) {
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
                let max = if t.starts_with(r#"{"t":"sprogress""#) { flunkiebal_core::proto::MAX_PROGRESS + 1024 } else { MAX_MSG };
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
