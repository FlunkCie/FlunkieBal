//! Our own mixer, for the desktop (cpal) and Android (AAudio): plays the bank's clips at a pitch
//! and a place in the stereo field, loops the beds, sends some of it all to a reverb, and keeps
//! the sum from clipping. Web Audio does the same in the browser (`web.rs`).

use super::dsp::{Clip, RATE};
use super::Play;
use std::sync::Arc;

use super::LOOPS;
/// No frame from the game this long (it's in the background): the beds fade out.
const STALE: f32 = 0.4;

struct Voice {
    clip: Arc<Clip>,
    pos: f64,
    step: f64,
    gl: f32,
    gr: f32,
    wet: f32,
    wait: usize,
}
#[derive(Default)]
struct Bed {
    clip: Option<Arc<Clip>>,
    pos: f64,
    gain: f32,
    want: f32,
    pan: f32,
    wet: f32,
}

/// Freeverb: eight combs and four all-passes a side.
struct Verb {
    combs: [Vec<(Vec<f32>, usize, f32)>; 2],
    alls: [Vec<(Vec<f32>, usize)>; 2],
    fb: Vec<f32>,
    damp: f32,
}
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLS: [usize; 4] = [556, 441, 341, 225];
impl Verb {
    fn new(rate: f32) -> Verb {
        let sc = rate / 44100.0;
        let side = |spread: usize| -> (Vec<(Vec<f32>, usize, f32)>, Vec<(Vec<f32>, usize)>) {
            (COMBS.iter().map(|d| (vec![0.0; ((d + spread) as f32 * sc) as usize + 1], 0, 0.0)).collect(), ALLS.iter().map(|d| (vec![0.0; ((d + spread) as f32 * sc) as usize + 1], 0)).collect())
        };
        let (c0, a0) = side(0);
        let (c1, a1) = side(23);
        let mut v = Verb { combs: [c0, c1], alls: [a0, a1], fb: vec![0.0; 8], damp: 0.3 };
        v.set(1.0, 0.3, rate);
        v
    }
    /// Reverberation time (s) and how much the highs die first (0 … 1).
    fn set(&mut self, t60: f32, damp: f32, rate: f32) {
        for (i, c) in self.combs[0].iter().enumerate() {
            let d = c.0.len() as f32 / rate;
            self.fb[i] = 10f32.powf(-3.0 * d / t60.max(0.1)).min(0.98);
        }
        self.damp = damp.clamp(0.0, 0.95);
    }
    #[inline]
    fn run(&mut self, x: f32) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let mut s = 0.0;
            for (i, (buf, at, lp)) in self.combs[ch].iter_mut().enumerate() {
                let y = buf[*at];
                *lp = y * (1.0 - self.damp) + *lp * self.damp;
                buf[*at] = x + *lp * self.fb[i];
                *at = (*at + 1) % buf.len();
                s += y;
            }
            for (buf, at) in self.alls[ch].iter_mut() {
                let b = buf[*at];
                buf[*at] = s + b * 0.5;
                *at = (*at + 1) % buf.len();
                s = b - s;
            }
            out[ch] = s;
        }
        (out[0], out[1])
    }
}

pub struct Mixer {
    clips: Vec<Option<Arc<Clip>>>,
    voices: Vec<Voice>,
    beds: [Bed; LOOPS],
    verb: Option<Verb>,
    room: (f32, f32),
    rate: f32,
    master: f32,
    pub volume: f32,
    lim: f32,
    stale: f32,
    tail: f32,
    l: Vec<f32>,
    r: Vec<f32>,
    send: Vec<f32>,
}
impl Default for Mixer {
    fn default() -> Self {
        Mixer { clips: Vec::new(), voices: Vec::new(), beds: Default::default(), verb: None, room: (0.6, 0.4), rate: 0.0, master: 0.0, volume: 0.8, lim: 1.0, stale: 0.0, tail: 0.0, l: Vec::new(), r: Vec::new(), send: Vec::new() }
    }
}
fn gains(pan: f32, gain: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::PI / 4.0;
    (a.cos() * gain * std::f32::consts::SQRT_2, a.sin() * gain * std::f32::consts::SQRT_2)
}
impl Mixer {
    pub fn add(&mut self, id: usize, c: Arc<Clip>) {
        if self.clips.len() <= id {
            self.clips.resize(id + 1, None);
        }
        self.clips[id] = Some(c);
    }
    pub fn play(&mut self, id: usize, p: Play) {
        let Some(Some(c)) = self.clips.get(id) else { return };
        if self.voices.len() > 48 {
            // the quietest goes
            if let Some(i) = self.voices.iter().enumerate().min_by(|a, b| (a.1.gl + a.1.gr).total_cmp(&(b.1.gl + b.1.gr))).map(|v| v.0) {
                self.voices.swap_remove(i);
            }
        }
        let (gl, gr) = gains(p.pan, p.gain);
        let rate = if self.rate > 0.0 { self.rate } else { 48000.0 };
        self.voices.push(Voice { clip: c.clone(), pos: 0.0, step: (p.rate * RATE / rate) as f64, gl, gr, wet: p.wet, wait: (p.delay * rate) as usize });
    }
    pub fn bed(&mut self, slot: usize, id: Option<usize>, p: Play) {
        let b = &mut self.beds[slot];
        let c = id.and_then(|i| self.clips.get(i).cloned().flatten());
        let same = match (&b.clip, &c) {
            (Some(a), Some(n)) => Arc::ptr_eq(a, n),
            (None, None) => true,
            _ => false,
        };
        if !same {
            b.clip = c;
            b.gain = 0.0;
            b.pos = 0.0;
        }
        b.want = p.gain;
        b.pan = p.pan;
        b.wet = p.wet;
    }
    pub fn room(&mut self, t60: f32, damp: f32) {
        self.room = (t60, damp);
        if let Some(v) = self.verb.as_mut() {
            v.set(t60, damp, self.rate);
        }
    }
    /// The game drew a frame.
    pub fn keep(&mut self) {
        self.stale = 0.0;
    }
    pub fn busy(&self) -> bool {
        !self.voices.is_empty() || self.beds.iter().any(|b| b.clip.is_some() && (b.gain > 1e-3 || (b.want > 1e-3 && self.stale < STALE))) || self.tail > 0.0
    }
    /// The next `frames` of stereo into `l` and `r` (silent to start with).
    fn mix(&mut self, frames: usize, rate: f32) {
        if self.rate != rate || self.verb.is_none() {
            self.rate = rate;
            let mut v = Verb::new(rate);
            v.set(self.room.0, self.room.1, rate);
            self.verb = Some(v);
        }
        self.l.clear();
        self.l.resize(frames, 0.0);
        self.r.clear();
        self.r.resize(frames, 0.0);
        self.send.clear();
        self.send.resize(frames, 0.0);
        let (l, r, send) = (&mut self.l, &mut self.r, &mut self.send);
        for v in self.voices.iter_mut() {
            let n = v.clip.len();
            let st = v.clip.is_stereo();
            let skip = v.wait.min(frames);
            v.wait -= skip;
            for k in skip..frames {
                let i = v.pos as usize;
                if i + 1 >= n {
                    v.pos = n as f64;
                    break;
                }
                let f = (v.pos - i as f64) as f32;
                let a = v.clip.l[i] + (v.clip.l[i + 1] - v.clip.l[i]) * f;
                let b = if st { v.clip.r[i] + (v.clip.r[i + 1] - v.clip.r[i]) * f } else { a };
                l[k] += a * v.gl;
                r[k] += b * v.gr;
                send[k] += (a * v.gl + b * v.gr) * v.wet;
                v.pos += v.step;
            }
        }
        self.voices.retain(|v| v.wait > 0 || (v.pos as usize) + 1 < v.clip.len());
        // the beds: each slides toward the level it was asked for (over about a second)
        self.stale += frames as f32 / rate;
        let k_fade = 1.0 - (-(frames as f32) / rate / 0.6).exp();
        let step = (RATE / rate) as f64;
        for b in self.beds.iter_mut() {
            let Some(c) = b.clip.as_ref() else { continue };
            let want = if self.stale > STALE { 0.0 } else { b.want };
            let g0 = b.gain;
            b.gain += (want - b.gain) * k_fade;
            if b.gain < 1e-4 && want == 0.0 {
                b.gain = 0.0;
                continue;
            }
            let n = c.len();
            let st = c.is_stereo();
            let (pl, pr) = gains(b.pan, 1.0);
            for k in 0..frames {
                let g = g0 + (b.gain - g0) * k as f32 / frames as f32;
                let i = b.pos as usize % n;
                let j = (i + 1) % n;
                let f = (b.pos - b.pos.floor()) as f32;
                let a = c.l[i] + (c.l[j] - c.l[i]) * f;
                let x = if st { c.r[i] + (c.r[j] - c.r[i]) * f } else { a };
                l[k] += a * g * pl;
                r[k] += x * g * pr;
                send[k] += (a + x) * g * b.wet * 0.5;
                b.pos = (b.pos + step) % n as f64;
            }
        }
        let verb = self.verb.as_mut().unwrap();
        let mut e = 0.0f32;
        for k in 0..frames {
            let (a, b) = verb.run(send[k] * 0.05);
            l[k] += a;
            r[k] += b;
            e = e.max(a.abs());
        }
        self.tail = if send.iter().any(|x| *x != 0.0) || e > 1e-4 { 1.0 } else { 0.0 };
        // volume, smoothed, and a limiter so a pile-up ducks instead of clipping
        let rel = 1.0 - (-1.0 / (0.15 * rate)).exp();
        for k in 0..frames {
            self.master += (self.volume - self.master) * 0.001;
            let (a, b) = (l[k] * self.master, r[k] * self.master);
            let pk = a.abs().max(b.abs());
            if pk * self.lim > 0.9 {
                self.lim = 0.9 / pk;
            } else {
                self.lim += (1.0 - self.lim) * rel;
            }
            l[k] = (a * self.lim).clamp(-1.0, 1.0);
            r[k] = (b * self.lim).clamp(-1.0, 1.0);
        }
    }
    /// Fill an interleaved buffer of `channels` per frame.
    pub fn render<T>(&mut self, out: &mut [T], channels: usize, rate: f32, conv: impl Fn(f32) -> T) {
        let ch = channels.max(1);
        let frames = out.len() / ch;
        self.mix(frames, rate);
        for (k, f) in out.chunks_mut(ch).enumerate() {
            let (a, b) = (self.l[k], self.r[k]);
            match ch {
                1 => f[0] = conv((a + b) * 0.5),
                _ => {
                    f[0] = conv(a);
                    f[1] = conv(b);
                    for x in f[2..].iter_mut() {
                        *x = conv(0.0);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::bank::{Cue, Job};

    fn clip(c: Cue) -> Arc<Clip> {
        let mut j = Job::new(c, 0);
        Arc::new(loop {
            if let Some(c) = j.step() {
                break c;
            }
        })
    }
    fn run(m: &mut Mixer, secs: f32) -> (f32, f32) {
        let mut out = vec![0.0f32; 960];
        let (mut pk, mut e) = (0.0f32, 0.0f32);
        for _ in 0..(secs * 100.0) as usize {
            m.render(&mut out, 2, 48000.0, |x| x);
            for x in &out {
                assert!(x.is_finite());
                pk = pk.max(x.abs());
                e += x * x;
            }
        }
        (pk, (e / (secs * 96000.0)).sqrt())
    }
    #[test]
    fn plays_pans_loops_and_fades_when_the_game_goes_quiet() {
        let mut m = Mixer::default();
        m.add(0, clip(Cue::WhistleTwo));
        m.add(1, clip(Cue::Leaves));
        m.room(1.0, 0.4);
        // hard left: the right channel stays (nearly) silent but for the reverb
        m.play(0, Play { gain: 1.0, pan: -1.0, wet: 0.0, ..Default::default() });
        let mut out = vec![0.0f32; 9600];
        m.render(&mut out, 2, 48000.0, |x| x);
        let (l, r) = out.chunks(2).fold((0.0f32, 0.0f32), |a, f| (a.0 + f[0].abs(), a.1 + f[1].abs()));
        assert!(l > 50.0 * r.max(1e-3), "left {l} right {r}");
        let (pk, _) = run(&mut m, 1.0);
        assert!(pk <= 1.0);
        assert!(!m.busy() || m.tail > 0.0);
        // a bed fades in while the game keeps drawing frames …
        m.bed(0, Some(1), Play { gain: 0.5, wet: 0.1, ..Default::default() });
        for _ in 0..20 {
            m.keep();
            run(&mut m, 0.1);
        }
        let (_, on) = run(&mut m, 0.2);
        assert!(on > 0.01, "bed rms {on}");
        // … and out once it stops
        run(&mut m, 3.0);
        let (_, off) = run(&mut m, 0.5);
        assert!(off < on * 0.05, "still {off} of {on}");
        run(&mut m, 3.0);
        assert!(!m.busy(), "lets the device go");
    }
    #[test]
    fn a_pile_up_does_not_clip() {
        let mut m = Mixer::default();
        m.add(0, clip(Cue::Crowd));
        for _ in 0..40 {
            m.play(0, Play { gain: 2.0, ..Default::default() });
        }
        let (pk, _) = run(&mut m, 1.0);
        assert!(pk <= 0.91, "peak {pk}");
    }
}
