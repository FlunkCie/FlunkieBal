//! The places: birds, bells, traffic, wind in the leaves, a café's chatter, the DJ upstairs.
//! Beds are loops; the rest are played now and then, somewhere round the listener.

use super::dsp::*;
use std::f32::consts::TAU;

/// A bird's note: a sine gliding through `f(u)`, with a touch of its octave.
fn chirp(b: &mut [f32], at: usize, dur: f32, amp: f32, f: impl Fn(f32) -> f32) {
    let n = ns(dur).min(b.len().saturating_sub(at));
    let mut ph = 0.0f32;
    for k in 0..n {
        let u = k as f32 / n as f32;
        ph = (ph + f(u) / RATE).fract();
        let e = (u * std::f32::consts::PI).sin().powf(0.7);
        b[at + k] += ((ph * TAU).sin() + 0.12 * (ph * 2.0 * TAU).sin()) * e * amp;
    }
}

/// A house sparrow: a handful of short "chirp"s.
pub fn sparrow(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(1.5);
    let mut t = 0.0;
    for _ in 0..r.i(5) + 3 {
        let (f0, f1) = (r.r(4800.0, 6200.0), r.r(3200.0, 4200.0));
        let d = r.r(0.04, 0.08);
        chirp(&mut c.l, ns(t), d, r.r(0.6, 1.0), |u| f0 + (f1 - f0) * u.powf(0.6));
        t += d + r.r(0.07, 0.16);
    }
    c.norm(0.8).trim()
}
/// A blackbird: a fluted phrase that ends in a twitter.
pub fn blackbird(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.6);
    let mut t = 0.0;
    for _ in 0..r.i(4) + 4 {
        let d = r.r(0.08, 0.24);
        let (f0, f1) = (r.r(1600.0, 3000.0), r.r(1600.0, 3200.0));
        let w = r.r(0.0, 0.15);
        chirp(&mut c.l, ns(t), d, r.r(0.6, 1.0), |u| (f0 + (f1 - f0) * u) * (1.0 + w * (u * 9.0).sin() * 0.2));
        t += d + r.r(0.02, 0.07);
    }
    for _ in 0..r.i(5) + 3 {
        let f0 = r.r(4500.0, 6500.0);
        chirp(&mut c.l, ns(t), 0.03, 0.4, |u| f0 * (1.0 - 0.3 * u));
        t += 0.045;
    }
    c.norm(0.8).trim()
}
/// A great tit: "tee-cher, tee-cher, tee-cher".
pub fn tit(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.0);
    let (hi, lo) = (r.r(5400.0, 6400.0), r.r(3500.0, 4200.0));
    for k in 0..r.i(3) + 3 {
        let t = k as f32 * 0.3;
        chirp(&mut c.l, ns(t), 0.08, 1.0, |u| hi * (1.0 - 0.05 * u));
        chirp(&mut c.l, ns(t + 0.11), 0.1, 0.8, |u| lo * (1.0 + 0.04 * u));
    }
    c.norm(0.8).trim()
}
/// A wood pigeon: "roo-COO-coo, coo-coo".
pub fn pigeon(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.8);
    let f0 = r.r(460.0, 520.0);
    for (t, d, p, a) in [(0.0, 0.28, 1.0, 0.7), (0.36, 0.55, 1.08, 1.0), (1.0, 0.3, 1.0, 0.8), (1.48, 0.3, 1.0, 0.75), (1.9, 0.26, 0.96, 0.6)] {
        let at = ns(t);
        let n = ns(d);
        let mut ph = 0.0f32;
        let mut lp = Lp1::new(900.0);
        for k in 0..n {
            let u = k as f32 / n as f32;
            let f = f0 * p * (1.0 + 0.06 * (u * 3.0).min(1.0) - 0.08 * u);
            ph = (ph + f / RATE).fract();
            // the throat's rasp
            let rough = 1.0 - 0.25 * (0.5 + 0.5 * (TAU * 28.0 * u * d).sin());
            let s = (ph * TAU).sin() + 0.3 * (ph * 2.0 * TAU).sin() + lp.run(r.n()) * 0.3;
            c.l[at + k] += s * rough * hump(u, 1.0, 0.25, 0.35) * a;
        }
    }
    c.norm(0.8).trim()
}
/// A bicycle bell: tring-tring.
pub fn bike_bell(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.0);
    let f0 = r.r(2000.0, 2400.0);
    let m = [(f0, 0.9, 1.0), (f0 * 1.013, 0.9, 0.7), (f0 * 2.62, 0.4, 0.35), (f0 * 4.1, 0.2, 0.2)];
    for burst in 0..2 {
        for k in 0..7 {
            let at = ns(burst as f32 * 0.42 + k as f32 / 19.0);
            modes(&mut c.l, at, &m, r.r(0.15, 0.3));
            click(&mut c.l, at, r.fork(), 6000.0, 0.1);
        }
    }
    c.norm(0.8).trim()
}
/// A herring gull, crying over the rooftops.
pub fn gull(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.4);
    let mut t = 0.0;
    for _ in 0..r.i(3) + 2 {
        let d = r.r(0.28, 0.4);
        let at = ns(t);
        let n = ns(d);
        let (mut f1, mut f2) = (Bq::new(F::Bp, 1900.0, 2.0), Bq::new(F::Bp, 3400.0, 3.0));
        let mut ph = 0.0f32;
        let base = r.r(850.0, 1000.0);
        for k in 0..n {
            let u = k as f32 / n as f32;
            let f = base * (1.0 + 0.6 * (u * 2.5).min(1.0) - 0.7 * (u - 0.4).max(0.0));
            ph = (ph + f / RATE).fract();
            let s = (2.0 * ph - 1.0) * (1.0 - 0.3 * (TAU * 70.0 * u * d).sin().abs());
            c.l[at + k] += (f1.run(s) + f2.run(s) * 0.6) * hump(u, 1.0, 0.08, 0.3);
        }
        t += d + r.r(0.08, 0.2);
    }
    c.norm(0.8).trim()
}
/// A car going past, left to right (or the other way): the tyres, the engine, Doppler.
pub fn car_pass(r: &mut Rng, v: usize) -> Clip {
    let len = 5.0;
    let mut c = Clip::stereo(len);
    let n = c.len();
    let dir = if v % 2 == 0 { 1.0 } else { -1.0 };
    let (mut tyre, mut lp) = (Bq::new(F::Bp, 900.0, 0.6), Bq::new(F::Lp, 300.0, 0.7));
    let mut ph = 0.0f32;
    let e0 = r.r(65.0, 85.0);
    for k in 0..n {
        let t = k as f32 / RATE;
        let x = (t - len * 0.5) / 1.1;
        let a = 1.0 / (1.0 + x * x * 2.5);
        let dop = 1.0 - 0.06 * (x * 1.5).tanh();
        ph = (ph + e0 * dop / RATE).fract();
        let s = tyre.run(r.n()) * 0.7 + lp.run(2.0 * ph - 1.0 + r.n() * 0.3) * 0.6;
        let pan = (x * 0.7).tanh() * dir;
        let g = (pan + 1.0) * std::f32::consts::PI / 4.0;
        c.l[k] += s * a * g.cos();
        c.r[k] += s * a * g.sin();
    }
    c.norm(0.8)
}
/// A train in the distance: the rumble rising and falling, the joints in the rails.
pub fn train(r: &mut Rng) -> Clip {
    let len = 9.0;
    let mut c = Clip::stereo(len);
    let n = c.len();
    let mut br = [Brown::default(), Brown::default()];
    let mut lp = [Bq::new(F::Lp, 160.0, 0.7), Bq::new(F::Lp, 160.0, 0.7)];
    for k in 0..n {
        let t = k as f32 / RATE;
        let a = hump(t, len, 3.0, 3.5);
        c.l[k] += lp[0].run(br[0].run(r.n())) * a;
        c.r[k] += lp[1].run(br[1].run(r.n())) * a;
    }
    let mut t = 0.8;
    while t < len - 1.0 {
        let a = hump(t, len, 3.0, 3.5) * 0.5;
        let mut m = vec![0.0; ns(0.2)];
        noise(&mut m, 0, r.fork(), F::Lp, 350.0, 300.0, 0.8, a, 0.002, 0.03);
        noise(&mut m, ns(0.11), r.fork(), F::Lp, 350.0, 300.0, 0.8, a, 0.002, 0.03);
        c.mix_in(&m, ns(t), 1.0, 0.0);
        t += r.r(0.8, 1.0);
    }
    c.norm(0.8)
}
/// Glasses touching: "proost".
pub fn clink(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(1.2);
    for k in 0..r.i(2) + 1 {
        let at = ns(k as f32 * r.r(0.12, 0.2));
        for _ in 0..2 {
            let f0 = r.r(2200.0, 3300.0);
            modes(&mut c.l, at, &[(f0, 0.35, 1.0), (f0 * 2.2, 0.2, 0.5), (f0 * 3.9, 0.1, 0.25)], 0.5);
        }
        click(&mut c.l, at, r.fork(), 6000.0, 0.4);
    }
    c.norm(0.8).trim()
}
/// A fork on a plate.
pub fn cutlery(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.8);
    for k in 0..r.i(3) + 1 {
        let at = ns(k as f32 * r.r(0.1, 0.25));
        modes(&mut c.l, at, &[(r.r(3500.0, 4500.0), 0.12, 1.0), (r.r(6500.0, 7500.0), 0.06, 0.6), (r.r(1200.0, 1500.0), 0.05, 0.3)], 0.6);
        click(&mut c.l, at, r.fork(), 5000.0, 0.5);
    }
    c.norm(0.8).trim()
}
/// Old wood giving a little.
pub fn creak(r: &mut Rng) -> Clip {
    let len = r.r(0.5, 0.9);
    let mut c = Clip::mono(len + 0.1);
    let n = ns(len);
    let (mut b1, mut b2) = (Bq::new(F::Bp, r.r(450.0, 600.0), 6.0), Bq::new(F::Bp, r.r(1000.0, 1300.0), 4.0));
    let mut ph = 0.0f32;
    let (f0, f1) = (r.r(30.0, 45.0), r.r(70.0, 100.0));
    for k in 0..n {
        let u = k as f32 / n as f32;
        let f = f0 + (f1 - f0) * (u * 3.0).sin().abs();
        ph += f / RATE;
        let pulse = if ph >= 1.0 {
            ph -= 1.0;
            1.0
        } else {
            0.0
        };
        c.l[k] += (b1.run(pulse) + b2.run(pulse) * 0.6) * hump(u, 1.0, 0.2, 0.3);
    }
    c.norm(0.8).trim()
}

// ---------- beds ----------

/// Stereo noise shaped per sample by `f(channel, t, white)`, made into a loop.
fn bed(len: f32, mut f: impl FnMut(usize, f32, f32) -> f32, r: &mut Rng) -> Clip {
    let mut c = Clip::stereo(len + 1.0);
    for k in 0..c.len() {
        let t = k as f32 / RATE;
        c.l[k] = f(0, t, r.n());
        c.r[k] = f(1, t, r.n());
    }
    c.looped(1.0).norm(0.7)
}
/// Wind through the leaves, in gusts.
pub fn leaves(r: &mut Rng) -> Clip {
    let g = Wander::new(r, 0.09);
    let mut p = [Pink::default(); 2];
    let mut bp = [Bq::new(F::Bp, 1600.0, 0.5), Bq::new(F::Bp, 1800.0, 0.5)];
    let mut hp = [Bq::new(F::Hp, 500.0, 0.7), Bq::new(F::Hp, 500.0, 0.7)];
    bed(12.0, |ch, t, w| hp[ch].run(bp[ch].run(p[ch].run(w))) * (0.45 + 0.55 * (0.5 + 0.5 * g.at(t + ch as f32 * 0.3))).powi(2), r)
}
/// A city a few streets away.
pub fn city(r: &mut Rng) -> Clip {
    let g = Wander::new(r, 0.05);
    let mut b = [Brown::default(); 2];
    let mut lp = [Bq::new(F::Lp, 280.0, 0.7), Bq::new(F::Lp, 280.0, 0.7)];
    let mut p = [Pink::default(); 2];
    let mut lp2 = [Bq::new(F::Lp, 900.0, 0.7), Bq::new(F::Lp, 900.0, 0.7)];
    bed(11.0, |ch, t, w| lp[ch].run(b[ch].run(w)) + lp2[ch].run(p[ch].run(w)) * 0.25 * (0.6 + 0.4 * g.at(t + ch as f32)), r)
}
/// Wind on a roof: lower, stronger gusts, a whistle in the railings.
pub fn wind(r: &mut Rng) -> Clip {
    let g = Wander::new(r, 0.07);
    let mut p = [Pink::default(); 2];
    let mut lp = [Bq::new(F::Lp, 500.0, 0.8), Bq::new(F::Lp, 500.0, 0.8)];
    let mut wh = [Bq::new(F::Bp, 540.0, 10.0), Bq::new(F::Bp, 610.0, 10.0)];
    let mut k = 0u32;
    bed(
        10.0,
        |ch, t, w| {
            let a = 0.5 + 0.5 * g.at(t + ch as f32 * 0.2);
            k += 1;
            if k % 32 < 2 {
                lp[ch].set(F::Lp, 300.0 + 700.0 * a, 0.8);
            }
            let x = p[ch].run(w);
            lp[ch].run(x) * (0.3 + 0.7 * a) + wh[ch].run(x) * a * a * 1.5
        },
        r,
    )
}
/// A room's own hush.
pub fn room(r: &mut Rng) -> Clip {
    let mut b = [Brown::default(); 2];
    let mut lp = [Bq::new(F::Lp, 220.0, 0.7), Bq::new(F::Lp, 220.0, 0.7)];
    let mut p = [Pink::default(); 2];
    let mut lp2 = [Bq::new(F::Lp, 1500.0, 0.7), Bq::new(F::Lp, 1500.0, 0.7)];
    bed(7.0, |ch, _, w| lp[ch].run(b[ch].run(w)) + lp2[ch].run(p[ch].run(w)) * 0.08, r)
}
/// The clubhouse fridge humming in the corner, in a quiet room.
pub fn fridge(r: &mut Rng) -> Clip {
    let g = Wander::new(r, 0.3);
    let mut b = [Brown::default(); 2];
    let mut lp = [Bq::new(F::Lp, 200.0, 0.7), Bq::new(F::Lp, 200.0, 0.7)];
    // a whole number of hum cycles, so the loop joins cleanly
    bed(
        6.0,
        |ch, t, w| {
            let h = (TAU * 100.0 * t).sin() * 0.5 + (TAU * 200.0 * t).sin() * 0.25 + (TAU * 300.0 * t).sin() * 0.1;
            lp[ch].run(b[ch].run(w)) * 0.7 + h * 0.12 * (0.9 + 0.1 * g.at(t))
        },
        r,
    )
}
/// A sports hall's ventilation.
pub fn vent(r: &mut Rng) -> Clip {
    let mut p = [Pink::default(); 2];
    let mut bp = [Bq::new(F::Bp, 420.0, 0.8), Bq::new(F::Bp, 460.0, 0.8)];
    let mut b = [Brown::default(); 2];
    let mut lp = [Bq::new(F::Lp, 150.0, 0.7), Bq::new(F::Lp, 150.0, 0.7)];
    bed(8.0, |ch, t, w| bp[ch].run(p[ch].run(w)) * 0.8 + lp[ch].run(b[ch].run(w)) * 0.6 + (TAU * 120.0 * t).sin() * 0.03, r)
}
/// A road nearby: the hiss of tyres, coming and going.
pub fn traffic(r: &mut Rng) -> Clip {
    let g = Wander::new(r, 0.12);
    let mut b = [Brown::default(); 2];
    let mut lp = [Bq::new(F::Lp, 300.0, 0.7), Bq::new(F::Lp, 300.0, 0.7)];
    let mut p = [Pink::default(); 2];
    let mut bp = [Bq::new(F::Bp, 1000.0, 0.6), Bq::new(F::Bp, 1100.0, 0.6)];
    bed(13.0, |ch, t, w| lp[ch].run(b[ch].run(w)) + bp[ch].run(p[ch].run(w)) * 0.5 * (0.5 + 0.5 * g.at(t + ch as f32 * 0.7)).powi(2), r)
}
/// A few crickets, each at its own pace.
pub fn crickets(r: &mut Rng) -> Clip {
    let mut c = Clip::stereo(6.0);
    let len = c.len();
    for _ in 0..4 {
        let (pan, f, per) = (r.n() * 0.8, r.r(4200.0, 4900.0), r.r(0.45, 0.75));
        let a = r.r(0.4, 1.0);
        let mut m = vec![0.0; ns(0.12)];
        for p in 0..3 {
            chirp(&mut m, ns(p as f32 / 30.0), 0.018, 1.0, |_| f);
        }
        let mut t = r.r(0.0, per);
        while ns(t) < len {
            c.mix_wrap(&m, ns(t), a, pan);
            t += per * r.r(0.97, 1.03);
        }
    }
    c.norm(0.7)
}
/// A packed café: one talker per step, each in phrases of syllables, all of it behind a low-pass.
pub fn babble(c: &mut Clip, r: &mut Rng, step: u32) {
    let len = 9.0;
    if step == 0 {
        *c = Clip::stereo(len);
    }
    let f = r.chance(0.45);
    let p = if f { r.r(180.0, 240.0) } else { r.r(100.0, 140.0) };
    let mut m = vec![0.0; ns(len + 1.0)];
    let vowels = [AH, EH, EE, OH, OO, UH];
    let mut t = r.r(0.0, 1.0);
    while t < len {
        // a phrase
        let syl = r.i(10) + 4;
        let mut v = vowels[r.i(6)];
        for s in 0..syl {
            let d = r.r(0.09, 0.22);
            let v2 = vowels[r.i(6)];
            let fall = 1.0 - 0.15 * s as f32 / syl as f32;
            let up = r.r(0.95, 1.12);
            let a = r.r(0.5, 1.0);
            voice(&mut m, ns(t), d, r.fork(), 0.2, |u| (p * fall * up, vmix(v, v2, u), a * hump(u, 1.0, 0.25, 0.35)));
            v = v2;
            t += d + r.r(0.0, 0.05);
        }
        t += r.r(0.25, 1.2);
    }
    // the far end of the room is muffled
    let mut lp = Bq::new(F::Lp, r.r(1600.0, 2600.0), 0.7);
    m.iter_mut().for_each(|x| *x = lp.run(*x));
    let n = c.len();
    let (tail, head) = m.split_at(n);
    let mut wrapped = tail.to_vec();
    for (k, x) in head.iter().enumerate() {
        wrapped[k] += x;
    }
    c.mix_in(&wrapped, 0, r.r(0.5, 1.0), r.n() * 0.85);
    if step + 1 == super::bank::Cue::Babble.steps() {
        *c = std::mem::take(c).norm(0.7);
    }
}
/// The DJ upstairs at the Duplex: four bars of house at 124.
pub fn music(r: &mut Rng) -> Clip {
    let beat = 60.0 / 124.0;
    let len = beat * 16.0;
    let mut c = Clip::stereo(len);
    let mut kick = vec![0.0; ns(0.4)];
    tone(&mut kick, 0, 150.0, 48.0, 1.0, 0.001, 0.12, 0.06);
    click(&mut kick, 0, r.fork(), 3000.0, 0.15);
    let mut hat = vec![0.0; ns(0.08)];
    noise(&mut hat, 0, r.fork(), F::Hp, 8000.0, 8000.0, 0.7, 0.5, 0.0005, 0.012);
    let mut clap = vec![0.0; ns(0.2)];
    for k in 0..3 {
        noise(&mut clap, ns(k as f32 * 0.009), r.fork(), F::Bp, 1300.0, 1200.0, 1.2, 0.6, 0.0005, 0.008);
    }
    noise(&mut clap, ns(0.02), r.fork(), F::Bp, 1200.0, 1000.0, 1.0, 0.4, 0.001, 0.05);
    for b in 0..16 {
        let t = b as f32 * beat;
        c.mix_wrap(&kick, ns(t), 1.0, 0.0);
        c.mix_wrap(&hat, ns(t + beat * 0.5), 0.35, 0.3);
        c.mix_wrap(&hat, ns(t + beat * 0.75), 0.12, -0.3);
        if b % 2 == 1 {
            c.mix_wrap(&clap, ns(t), 0.45, 0.0);
        }
    }
    // the bass, on the off-beats: A minor, then F, C, G
    let roots = [55.0, 43.65, 65.41, 49.0];
    for b in 0..16 {
        let f = roots[b / 4];
        let mut m = vec![0.0; ns(beat * 0.45)];
        let mut lp = Bq::new(F::Lp, 260.0, 2.0);
        let mut ph = 0.0f32;
        let n = m.len();
        for k in 0..n {
            ph = (ph + f / RATE).fract();
            m[k] = lp.run(2.0 * ph - 1.0) * hump(k as f32 / n as f32, 1.0, 0.05, 0.3);
        }
        c.mix_wrap(&m, ns(b as f32 * beat + beat * 0.5), 0.55, 0.0);
    }
    // a chord stab every bar
    let chords = [[220.0, 261.63, 329.63], [174.61, 220.0, 261.63], [196.0, 261.63, 329.63], [196.0, 246.94, 293.66]];
    for bar in 0..4 {
        for (i, f) in chords[bar].iter().enumerate() {
            let mut m = vec![0.0; ns(0.5)];
            let mut lp = Bq::new(F::Lp, 1800.0, 1.5);
            let mut ph = [0.0f32; 2];
            let n = m.len();
            for k in 0..n {
                let t = k as f32 / RATE;
                ph[0] = (ph[0] + f * 1.004 / RATE).fract();
                ph[1] = (ph[1] + f * 0.996 / RATE).fract();
                m[k] = lp.run(ph[0] + ph[1] - 1.0) * env(t, 0.004, 0.09);
            }
            c.mix_wrap(&m, ns((bar * 4) as f32 * beat + beat * 1.5), 0.2, (i as f32 - 1.0) * 0.5);
        }
    }
    c.norm(0.7)
}
