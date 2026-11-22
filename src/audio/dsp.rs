//! The building blocks every sound is made of: noise, filters, resonators, a voice.

use std::f32::consts::{PI, TAU};

/// The rate the bank is rendered at; playback resamples to the device.
pub const RATE: f32 = 44100.0;

pub fn ns(s: f32) -> usize {
    (s.max(0.0) * RATE) as usize
}

/// A rendered sound: mono (`r` empty) or stereo.
#[derive(Default, Clone)]
pub struct Clip {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}
impl Clip {
    pub fn mono(secs: f32) -> Clip {
        Clip { l: vec![0.0; ns(secs)], r: Vec::new() }
    }
    pub fn stereo(secs: f32) -> Clip {
        Clip { l: vec![0.0; ns(secs)], r: vec![0.0; ns(secs)] }
    }
    pub fn len(&self) -> usize {
        self.l.len()
    }
    pub fn is_stereo(&self) -> bool {
        !self.r.is_empty()
    }
    /// Scale so the loudest sample is `peak`.
    pub fn norm(mut self, peak: f32) -> Clip {
        let m = self.l.iter().chain(self.r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        if m > 1e-6 {
            let k = peak / m;
            self.l.iter_mut().chain(self.r.iter_mut()).for_each(|x| *x *= k);
        }
        self
    }
    /// Cut the silent end off, with a short fade.
    pub fn trim(mut self) -> Clip {
        let last = |v: &[f32]| v.iter().rposition(|x| x.abs() > 2e-4).unwrap_or(0);
        let n = (last(&self.l).max(if self.is_stereo() { last(&self.r) } else { 0 }) + 64).min(self.len());
        self.l.truncate(n);
        if self.is_stereo() {
            self.r.truncate(n);
        }
        let f = 64.min(n);
        for k in 0..f {
            let g = k as f32 / f as f32;
            self.l[n - 1 - k] *= g;
            if self.is_stereo() {
                self.r[n - 1 - k] *= g;
            }
        }
        self
    }
    /// Make it loop: the last `x` seconds are cross-faded into the start, and cut off.
    pub fn looped(mut self, x: f32) -> Clip {
        let n = ns(x).min(self.len() / 2);
        let len = self.len() - n;
        for ch in [&mut self.l, &mut self.r] {
            if ch.is_empty() {
                continue;
            }
            for k in 0..n {
                let u = k as f32 / n as f32;
                let (a, b) = ((u * PI / 2.0).sin(), (u * PI / 2.0).cos());
                ch[k] = ch[k] * a + ch[len + k] * b;
            }
            ch.truncate(len);
        }
        self
    }
    /// A mono sound into a stereo clip at `pan` (-1 left … 1 right).
    pub fn mix_in(&mut self, m: &[f32], at: usize, gain: f32, pan: f32) {
        let a = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
        let (gl, gr) = (a.cos() * gain, a.sin() * gain);
        let len = self.len();
        for (k, x) in m.iter().enumerate() {
            let i = at + k;
            if i >= len {
                break;
            }
            self.l[i] += x * gl;
            if self.is_stereo() {
                self.r[i] += x * gr;
            }
        }
    }
    /// The same, but wrapping round the end (for loops made of events).
    pub fn mix_wrap(&mut self, m: &[f32], at: usize, gain: f32, pan: f32) {
        let a = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
        let (gl, gr) = (a.cos() * gain, a.sin() * gain);
        let len = self.len();
        for (k, x) in m.iter().enumerate() {
            let i = (at + k) % len;
            self.l[i] += x * gl;
            if self.is_stereo() {
                self.r[i] += x * gr;
            }
        }
    }
}

#[derive(Clone)]
pub struct Rng(u32);
impl Rng {
    pub fn new(s: u32) -> Rng {
        Rng(s.wrapping_mul(2654435761).wrapping_add(0x9e37) | 1)
    }
    pub fn u(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16777216.0
    }
    /// -1 … 1
    pub fn n(&mut self) -> f32 {
        self.u() * 2.0 - 1.0
    }
    pub fn r(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.u()
    }
    pub fn i(&mut self, n: usize) -> usize {
        ((self.u() * n as f32) as usize).min(n.max(1) - 1)
    }
    /// A generator of its own, for a callee (so `r` can still be used for the other arguments).
    pub fn fork(&mut self) -> Rng {
        Rng::new((self.u() * 16777216.0) as u32 ^ 0x5bd1e995)
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.u() < p
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum F {
    Lp,
    Hp,
    Bp,
}
/// A biquad (audio EQ cookbook), transposed direct form II.
#[derive(Clone, Copy, Default)]
pub struct Bq {
    c: [f32; 5],
    z: [f32; 2],
}
impl Bq {
    pub fn new(f: F, hz: f32, q: f32) -> Bq {
        let mut b = Bq::default();
        b.set(f, hz, q);
        b
    }
    pub fn set(&mut self, f: F, hz: f32, q: f32) {
        let w = TAU * (hz / RATE).clamp(1e-4, 0.49);
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q.max(0.05));
        let (b0, b1, b2) = match f {
            F::Lp => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0),
            F::Hp => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0),
            F::Bp => (al, 0.0, -al),
        };
        let a0 = 1.0 + al;
        self.c = [b0 / a0, b1 / a0, b2 / a0, -2.0 * c / a0, (1.0 - al) / a0];
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let c = self.c;
        let y = c[0] * x + self.z[0];
        self.z[0] = c[1] * x - c[3] * y + self.z[1];
        self.z[1] = c[2] * x - c[4] * y;
        y
    }
}
/// One-pole low-pass.
#[derive(Clone, Copy, Default)]
pub struct Lp1 {
    a: f32,
    pub y: f32,
}
impl Lp1 {
    pub fn new(hz: f32) -> Lp1 {
        Lp1 { a: 1.0 - (-TAU * hz / RATE).exp(), y: 0.0 }
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}
/// Pink-ish noise (Paul Kellet's economy filter).
#[derive(Clone, Copy, Default)]
pub struct Pink([f32; 3]);
impl Pink {
    #[inline]
    pub fn run(&mut self, w: f32) -> f32 {
        let b = &mut self.0;
        b[0] = 0.99765 * b[0] + w * 0.0990460;
        b[1] = 0.96300 * b[1] + w * 0.2965164;
        b[2] = 0.57000 * b[2] + w * 1.0526913;
        (b[0] + b[1] + b[2] + w * 0.1848) * 0.3
    }
}
/// Brown noise.
#[derive(Clone, Copy, Default)]
pub struct Brown(f32);
impl Brown {
    #[inline]
    pub fn run(&mut self, w: f32) -> f32 {
        self.0 = (self.0 + 0.04 * w) / 1.02;
        self.0 * 3.0
    }
}
/// A slow, smooth wander between -1 and 1 (three sines with odd periods): gusts, swells.
pub struct Wander([(f32, f32); 3]);
impl Wander {
    pub fn new(r: &mut Rng, hz: f32) -> Wander {
        Wander([(hz * r.r(0.8, 1.2), r.r(0.0, TAU)), (hz * r.r(1.7, 2.3), r.r(0.0, TAU)), (hz * r.r(3.1, 3.9), r.r(0.0, TAU))])
    }
    pub fn at(&self, t: f32) -> f32 {
        let s = &self.0;
        ((TAU * s[0].0 * t + s[0].1).sin() * 0.55 + (TAU * s[1].0 * t + s[1].1).sin() * 0.3 + (TAU * s[2].0 * t + s[2].1).sin() * 0.15).clamp(-1.0, 1.0)
    }
}

/// Attack then an exponential fall with time constant `tau`.
#[inline]
pub fn env(t: f32, att: f32, tau: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < att {
        t / att
    } else {
        (-(t - att) / tau).exp()
    }
}
/// A smooth hump: up over `a`, held, down over `d`, `len` long in all.
#[inline]
pub fn hump(t: f32, len: f32, a: f32, d: f32) -> f32 {
    if t < 0.0 || t > len {
        0.0
    } else if t < a {
        (t / a * PI / 2.0).sin().powi(2)
    } else if t > len - d {
        ((len - t) / d * PI / 2.0).sin().powi(2)
    } else {
        1.0
    }
}

/// A struck resonance: a decaying sine (`t60`: seconds to fall 60 dB), added from sample `at`.
pub fn mode(b: &mut [f32], at: usize, hz: f32, t60: f32, amp: f32) {
    if hz >= RATE * 0.47 || at >= b.len() || amp.abs() < 1e-6 {
        return;
    }
    let (s, c) = (TAU * hz / RATE).sin_cos();
    let k = (-6.9078 / (t60.max(0.002) * RATE)).exp();
    let n = ((t60 * (amp.abs() / 1e-5).ln() / 6.9078 * RATE) as usize).min(b.len() - at);
    let (mut re, mut im) = (amp, 0.0f32);
    for x in b[at..at + n].iter_mut() {
        let r2 = (re * c - im * s) * k;
        im = (re * s + im * c) * k;
        re = r2;
        *x += im;
    }
}
/// Several resonances: (frequency, t60, amplitude).
pub fn modes(b: &mut [f32], at: usize, m: &[(f32, f32, f32)], gain: f32) {
    for &(f, t, a) in m {
        mode(b, at, f, t, a * gain);
    }
}
/// Filtered noise with an attack and an exponential fall; `f` may sweep from `hz0` to `hz1`.
#[allow(clippy::too_many_arguments)]
pub fn noise(b: &mut [f32], at: usize, mut r: Rng, f: F, hz0: f32, hz1: f32, q: f32, amp: f32, att: f32, tau: f32) {
    let len = ((att + tau * 7.0) * RATE) as usize;
    let n = len.min(b.len().saturating_sub(at));
    let mut bq = Bq::new(f, hz0, q);
    for k in 0..n {
        let t = k as f32 / RATE;
        if k % 16 == 0 && hz0 != hz1 {
            bq.set(f, hz0 * (hz1 / hz0).powf((k as f32 / len as f32).min(1.0)), q);
        }
        b[at + k] += bq.run(r.n()) * amp * env(t, att, tau);
    }
}
/// A sine sweeping from `f0` to `f1`, with an attack and an exponential fall.
#[allow(clippy::too_many_arguments)]
pub fn tone(b: &mut [f32], at: usize, f0: f32, f1: f32, amp: f32, att: f32, tau: f32, sweep: f32) {
    let n = (((att + tau * 7.0) * RATE) as usize).min(b.len().saturating_sub(at));
    let mut ph = 0.0f32;
    for k in 0..n {
        let t = k as f32 / RATE;
        let f = f0 * (f1 / f0).powf((t / sweep.max(1e-3)).min(1.0));
        ph = (ph + f / RATE).fract();
        b[at + k] += (ph * TAU).sin() * amp * env(t, att, tau);
    }
}
/// An air bubble in a liquid (Minnaert): a sine that rises as it dies away.
pub fn bubble(b: &mut [f32], at: usize, hz: f32, tau: f32, amp: f32) {
    let n = ((tau * 7.0 * RATE) as usize).min(b.len().saturating_sub(at));
    let mut ph = 0.0f32;
    for k in 0..n {
        let t = k as f32 / RATE;
        ph = (ph + hz * (1.0 + 0.6 * t / tau) / RATE).fract();
        b[at + k] += (ph * TAU).sin() * amp * env(t, 0.001, tau);
    }
}
/// A tiny click: a few samples of bright noise.
pub fn click(b: &mut [f32], at: usize, r: Rng, hz: f32, amp: f32) {
    noise(b, at, r, F::Bp, hz, hz, 0.9, amp, 0.0005, 0.0025);
}

/// Vowel formants (F1, F2, F3).
pub const AH: [f32; 3] = [760.0, 1150.0, 2500.0];
pub const EH: [f32; 3] = [560.0, 1800.0, 2550.0];
pub const EE: [f32; 3] = [320.0, 2250.0, 3000.0];
pub const OH: [f32; 3] = [560.0, 880.0, 2450.0];
pub const OO: [f32; 3] = [340.0, 850.0, 2300.0];
pub const UH: [f32; 3] = [620.0, 1200.0, 2550.0];
pub fn vmix(a: [f32; 3], b: [f32; 3], u: f32) -> [f32; 3] {
    let u = u.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u]
}
/// A human voice: a buzzing (and breathy) source through three formant filters. `shape(u)` gives
/// the pitch (Hz), formants and loudness at `u` = 0 … 1 through `dur` seconds.
pub fn voice(b: &mut [f32], at: usize, dur: f32, mut r: Rng, breath: f32, shape: impl Fn(f32) -> (f32, [f32; 3], f32)) {
    let n = ns(dur).min(b.len().saturating_sub(at));
    let mut fs = [Bq::default(); 3];
    let mut ph = 0.0f32;
    let mut jit = 0.0f32;
    let vib = r.r(4.5, 6.5);
    let vph = r.r(0.0, TAU);
    let mut hp = Bq::new(F::Hp, 90.0, 0.7);
    for k in 0..n {
        let u = k as f32 / n as f32;
        let t = k as f32 / RATE;
        let (p, fm, a) = shape(u);
        if k % 32 == 0 {
            for (i, f) in fs.iter_mut().enumerate() {
                let bw = [90.0, 120.0, 180.0][i];
                f.set(F::Bp, fm[i], fm[i] / bw);
            }
            jit = jit * 0.7 + r.n() * 0.3;
        }
        let f0 = p * (1.0 + 0.012 * (TAU * vib * t + vph).sin() + 0.01 * jit);
        ph += f0 / RATE;
        if ph >= 1.0 {
            ph -= 1.0;
        }
        // a sawtooth tamed at the jump (polyBLEP), close enough to a glottal pulse train
        let dt = f0 / RATE;
        let mut s = 2.0 * ph - 1.0;
        if ph < dt {
            let x = ph / dt;
            s -= x + x - x * x - 1.0;
        } else if ph > 1.0 - dt {
            let x = (ph - 1.0) / dt;
            s -= x * x + x + x + 1.0;
        }
        let src = s * (1.0 - breath) + r.n() * breath * 1.6;
        let y = fs[0].run(src) + fs[1].run(src) * 0.55 + fs[2].run(src) * 0.3;
        b[at + k] += hp.run(y) * a;
    }
}
