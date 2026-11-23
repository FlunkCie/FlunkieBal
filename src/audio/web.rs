//! The browser: the bank's clips become `AudioBuffer`s, played through Web Audio (gain, stereo
//! panner, a convolver for the room, a compressor before the speakers).

use super::dsp::{Clip, Rng, RATE};
use super::LOOPS;
use super::Play;
use std::sync::Arc;
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, ConvolverNode, GainNode, StereoPannerNode};

struct Bed {
    id: usize,
    src: AudioBufferSourceNode,
    gain: GainNode,
    pan: StereoPannerNode,
    send: GainNode,
    last: (f32, f32, f32),
}
struct Graph {
    ctx: AudioContext,
    master: GainNode,
    verb: ConvolverNode,
}
pub struct Out {
    g: Option<Graph>,
    clips: Vec<Option<Arc<Clip>>>,
    bufs: Vec<Option<AudioBuffer>>,
    beds: [Option<Bed>; LOOPS],
    room: (f32, f32),
    room_set: bool,
    volume: f32,
}

/// A room's echo, for the convolver: decaying noise, the highs dying first.
fn impulse(t60: f32, damp: f32) -> Clip {
    let mut c = Clip::stereo(t60 * 1.1 + 0.05);
    let mut r = Rng::new(4242);
    let n = c.len();
    let pre = (0.008 * RATE) as usize;
    for ch in 0..2 {
        let mut y = 0.0f32;
        for k in pre..n {
            let t = (k - pre) as f32 / RATE;
            let a = (-6.9078 * t / t60).exp();
            // the low-pass closes as it decays
            let lp = (1.0 - damp * (t / t60).min(1.0) * 0.95).max(0.03);
            y += lp * (r.n() - y);
            let s = y * a;
            if ch == 0 {
                c.l[k] = s;
            } else {
                c.r[k] = s;
            }
        }
    }
    c.norm(0.5)
}

impl Out {
    pub fn new() -> Out {
        Out { g: None, clips: Vec::new(), bufs: Vec::new(), beds: Default::default(), room: (0.6, 0.4), room_set: false, volume: 0.8 }
    }
    fn graph(&mut self) -> Option<&Graph> {
        if self.g.is_none() {
            let ctx = AudioContext::new().ok()?;
            let comp = ctx.create_dynamics_compressor().ok()?;
            comp.threshold().set_value(-8.0);
            comp.knee().set_value(6.0);
            comp.ratio().set_value(10.0);
            comp.attack().set_value(0.003);
            comp.release().set_value(0.2);
            let master = ctx.create_gain().ok()?;
            master.gain().set_value(self.volume);
            let verb = ctx.create_convolver().ok()?;
            let _ = master.connect_with_audio_node(&comp);
            let _ = verb.connect_with_audio_node(&master);
            let _ = comp.connect_with_audio_node(&ctx.destination());
            // a hidden tab goes quiet (its beds would play on otherwise)
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let (c2, d2) = (ctx.clone(), doc.clone());
                let f = Closure::<dyn FnMut()>::new(move || {
                    let _ = if d2.hidden() { c2.suspend() } else { c2.resume() };
                });
                let _ = doc.add_event_listener_with_callback("visibilitychange", f.as_ref().unchecked_ref());
                f.forget();
            }
            self.g = Some(Graph { ctx, master, verb });
            self.room_set = false;
        }
        let g = self.g.as_ref()?;
        if g.ctx.state() == web_sys::AudioContextState::Suspended && !web_sys::window().and_then(|w| w.document()).map_or(false, |d| d.hidden()) {
            let _ = g.ctx.resume();
        }
        self.g.as_ref()
    }
    /// Call from a user gesture so browsers let the sound start.
    pub fn unlock(&mut self) {
        let _ = self.graph();
    }
    pub fn ready(&self) -> bool {
        self.g.is_some()
    }
    pub fn add(&mut self, id: usize, c: &Arc<Clip>) {
        if self.clips.len() <= id {
            self.clips.resize(id + 1, None);
        }
        self.clips[id] = Some(c.clone());
    }
    fn buf(&mut self, id: usize) -> Option<AudioBuffer> {
        if let Some(Some(b)) = self.bufs.get(id) {
            return Some(b.clone());
        }
        let c = self.clips.get(id)?.clone()?;
        let ctx = self.g.as_ref()?.ctx.clone();
        let b = ctx.create_buffer(if c.is_stereo() { 2 } else { 1 }, c.len().max(1) as u32, RATE).ok()?;
        let _ = b.copy_to_channel(&c.l, 0);
        if c.is_stereo() {
            let _ = b.copy_to_channel(&c.r, 1);
        }
        if self.bufs.len() <= id {
            self.bufs.resize(id + 1, None);
        }
        self.bufs[id] = Some(b.clone());
        Some(b)
    }
    /// A source through gain → panner → master, and a send to the room.
    fn chain(&self, b: &AudioBuffer, p: &Play) -> Option<(AudioBufferSourceNode, GainNode, StereoPannerNode, GainNode)> {
        let g = self.g.as_ref()?;
        let (src, gain, pan, send) = (g.ctx.create_buffer_source().ok()?, g.ctx.create_gain().ok()?, g.ctx.create_stereo_panner().ok()?, g.ctx.create_gain().ok()?);
        src.set_buffer(Some(b));
        src.playback_rate().set_value(p.rate);
        gain.gain().set_value(p.gain);
        pan.pan().set_value(p.pan.clamp(-1.0, 1.0));
        send.gain().set_value(p.wet);
        let _ = src.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&pan);
        let _ = pan.connect_with_audio_node(&g.master);
        let _ = pan.connect_with_audio_node(&send);
        let _ = send.connect_with_audio_node(&g.verb);
        Some((src, gain, pan, send))
    }
    pub fn play(&mut self, id: usize, p: Play) {
        if self.graph().is_none() {
            return;
        }
        let Some(b) = self.buf(id) else { return };
        let Some((src, ..)) = self.chain(&b, &p) else { return };
        let t0 = self.g.as_ref().unwrap().ctx.current_time() + p.delay as f64;
        let _ = src.start_with_when(t0);
    }
    pub fn bed(&mut self, slot: usize, id: Option<usize>, p: Play) {
        if self.g.is_none() {
            return;
        }
        let now = self.g.as_ref().unwrap().ctx.current_time();
        if self.beds[slot].as_ref().map(|b| Some(b.id) != id).unwrap_or(false) {
            let b = self.beds[slot].take().unwrap();
            let _ = b.gain.gain().set_target_at_time(0.0, now, 0.3);
            let _ = b.src.stop_with_when(now + 2.0);
        }
        let Some(id) = id else { return };
        if self.beds[slot].is_none() {
            let Some(buf) = self.buf(id) else { return };
            let Some((src, gain, pan, send)) = self.chain(&buf, &Play { gain: 0.0, ..p }) else { return };
            src.set_loop(true);
            let _ = src.start();
            self.beds[slot] = Some(Bed { id, src, gain, pan, send, last: (-1.0, 0.0, 0.0) });
        }
        let b = self.beds[slot].as_mut().unwrap();
        let now_p = (p.gain, p.pan, p.wet);
        if b.last != now_p {
            let _ = b.gain.gain().set_target_at_time(p.gain, now, 0.4);
            let _ = b.pan.pan().set_target_at_time(p.pan, now, 0.2);
            let _ = b.send.gain().set_target_at_time(p.wet, now, 0.2);
            b.last = now_p;
        }
    }
    pub fn room(&mut self, t60: f32, damp: f32) {
        if self.room != (t60, damp) {
            self.room = (t60, damp);
            self.room_set = false;
        }
    }
    pub fn frame(&mut self, volume: f32) {
        let Some(g) = self.g.as_ref() else {
            self.volume = volume;
            return;
        };
        if volume != self.volume {
            self.volume = volume;
            let _ = g.master.gain().set_target_at_time(volume, g.ctx.current_time(), 0.05);
        }
        if !self.room_set {
            self.room_set = true;
            let ir = impulse(self.room.0, self.room.1);
            // a convolver takes only the context's own sample rate (Firefox throws otherwise, and a throwing
            // setter stops the game): the reverb's tail is resampled to it
            let rate = g.ctx.sample_rate();
            let resample = |x: &[f32]| -> Vec<f32> {
                if (rate - RATE).abs() < 1.0 {
                    return x.to_vec();
                }
                let n = (x.len() as f32 * rate / RATE) as usize;
                (0..n)
                    .map(|i| {
                        let t = i as f32 * RATE / rate;
                        let (k, f) = (t as usize, t.fract());
                        let a = x.get(k).copied().unwrap_or(0.0);
                        a + (x.get(k + 1).copied().unwrap_or(0.0) - a) * f
                    })
                    .collect()
            };
            let (l, r) = (resample(&ir.l), resample(&ir.r));
            if let Ok(b) = g.ctx.create_buffer(2, l.len().max(1) as u32, rate) {
                let _ = b.copy_to_channel(&l, 0);
                let _ = b.copy_to_channel(&r, 1);
                g.verb.set_buffer(Some(&b));
            }
        }
    }
}
