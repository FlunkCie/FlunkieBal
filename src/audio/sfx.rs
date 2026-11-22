//! The match and the menus: ball, glass, cans, whistle, people, little tunes, buttons.

use super::dsp::*;
use std::f32::consts::{PI, TAU};

// ---------- ball, bottle, beers ----------

/// An arm swung hard: air torn past the hand, a sleeve.
pub fn throw(r: &mut Rng) -> Clip {
    let len = r.r(0.36, 0.46);
    let mut c = Clip::mono(len + 0.05);
    let n = ns(len);
    let (mut bp, mut lp) = (Bq::new(F::Bp, 400.0, 1.3), Bq::new(F::Lp, 380.0, 0.7));
    let peak = r.r(0.45, 0.6);
    let top = r.r(1500.0, 2100.0);
    for k in 0..n {
        let u = k as f32 / n as f32;
        // rising to a peak at release, then away
        let a = if u < peak { (u / peak).powf(2.2) } else { ((1.0 - u) / (1.0 - peak)).powf(1.4) };
        if k % 16 == 0 {
            bp.set(F::Bp, 300.0 + top * a, 1.3);
        }
        let w = r.n();
        c.l[k] += (bp.run(w) + lp.run(w) * 0.5) * a;
    }
    // the sleeve
    for _ in 0..r.i(5) + 4 {
        let at = ns(r.r(0.0, len * 0.6));
        noise(&mut c.l, at, r.fork(), F::Bp, 2200.0, 1800.0, 1.0, r.r(0.03, 0.06), 0.002, 0.012);
    }
    c.norm(0.9).trim()
}

/// The ball lands: on grass (0), a hard floor (1) or wood (2).
pub fn bounce(r: &mut Rng, floor: u8) -> Clip {
    let mut c = Clip::mono(0.5);
    let b = &mut c.l;
    let j = r.r(0.94, 1.06);
    match floor {
        0 => {
            tone(b, 0, 115.0 * j, 62.0 * j, 1.0, 0.002, 0.045, 0.06);
            noise(b, 0, r.fork(), F::Lp, 600.0, 300.0, 0.7, 0.7, 0.001, 0.022);
            // blades brushed aside
            for _ in 0..12 {
                let at = ns(r.r(0.0, 0.09));
                noise(b, at, r.fork(), F::Bp, 2800.0, 2400.0, 1.2, r.r(0.02, 0.05), 0.001, 0.008);
            }
        }
        1 => {
            // a hollow ball rings when it hits something hard
            modes(b, 0, &[(245.0 * j, 0.13, 1.0), (545.0 * j, 0.08, 0.5), (955.0 * j, 0.05, 0.3), (1480.0 * j, 0.03, 0.15)], 0.8);
            click(b, 0, r.fork(), 3200.0, 1.4);
            tone(b, 0, 130.0, 90.0, 0.6, 0.001, 0.02, 0.03);
        }
        _ => {
            modes(b, 0, &[(240.0 * j, 0.1, 0.8), (540.0 * j, 0.06, 0.35)], 0.8);
            // the boards answer
            modes(b, 0, &[(96.0 * j, 0.14, 0.9), (212.0 * j, 0.09, 0.5), (433.0, 0.05, 0.3), (870.0, 0.03, 0.15)], 0.9);
            click(b, 0, r.fork(), 2200.0, 1.0);
        }
    }
    c.norm(0.9).trim()
}

/// Glass bottle resonances, `f0` the lowest; `wet` damps them (there's liquid in it).
fn glass(f0: f32, t: f32) -> [(f32, f32, f32); 5] {
    [(f0, 0.5 * t, 1.0), (f0 * 2.32, 0.34 * t, 0.6), (f0 * 4.15, 0.22 * t, 0.45), (f0 * 6.4, 0.13 * t, 0.3), (f0 * 9.3, 0.08 * t, 0.2)]
}

/// The ball strikes the bottle: a glassy "tonk".
pub fn ball_bottle(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.9);
    let b = &mut c.l;
    modes(b, 0, &glass(r.r(640.0, 760.0), 0.65), 0.7);
    modes(b, 0, &[(250.0, 0.07, 0.7)], 1.0);
    click(b, 0, r.fork(), 4500.0, 1.0);
    c.norm(0.9).trim()
}

/// Water sloshing inside the bottle.
fn slosh(b: &mut [f32], at: usize, r: &mut Rng, amp: f32, len: f32) {
    let n = ns(len).min(b.len().saturating_sub(at));
    let mut bq = Bq::new(F::Bp, 500.0, 2.5);
    let w = r.r(6.0, 9.0);
    for k in 0..n {
        let t = k as f32 / RATE;
        if k % 16 == 0 {
            bq.set(F::Bp, 480.0 + 220.0 * (TAU * w * t).sin(), 2.5);
        }
        b[at + k] += bq.run(r.n()) * amp * env(t, 0.03, len * 0.3);
    }
    for _ in 0..4 {
        bubble(b, at + ns(r.r(0.02, len * 0.6)), r.r(500.0, 1000.0), r.r(0.012, 0.025), amp * 0.4);
    }
}

/// The bottle goes over: dull on grass, a clattering bounce on a hard floor.
pub fn bottle_fall(r: &mut Rng, hard: bool) -> Clip {
    let mut c = Clip::mono(1.1);
    let b = &mut c.l;
    let f0 = r.r(660.0, 760.0);
    if hard {
        let (mut t, mut gap, mut a) = (0.0f32, r.r(0.1, 0.13), 1.0f32);
        for _ in 0..5 {
            let at = ns(t);
            modes(b, at, &glass(f0 * r.r(0.98, 1.02), 0.7), a * 0.6);
            click(b, at, r.fork(), 3800.0, a * 1.2);
            tone(b, at, 150.0, 100.0, a * 0.5, 0.001, 0.02, 0.03);
            t += gap;
            gap *= 0.62;
            a *= 0.55;
        }
        slosh(b, ns(0.02), r, 0.25, 0.5);
    } else {
        tone(b, 0, 170.0, 85.0, 1.0, 0.002, 0.06, 0.08);
        noise(b, 0, r.fork(), F::Lp, 800.0, 400.0, 0.7, 0.6, 0.001, 0.03);
        modes(b, 0, &glass(f0, 0.15), 0.25);
        slosh(b, ns(0.03), r, 0.35, 0.55);
    }
    c.norm(0.9).trim()
}

/// A full bottle of beer put down on the ground.
pub fn bottle_set(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.5);
    let b = &mut c.l;
    modes(b, 0, &glass(r.r(860.0, 960.0), 0.25), 0.5);
    tone(b, 0, 140.0, 90.0, 0.8, 0.002, 0.04, 0.05);
    click(b, 0, r.fork(), 3000.0, 0.6);
    c.norm(0.9).trim()
}

/// Aluminium can resonances (with beer in it).
fn can(f0: f32, t: f32) -> [(f32, f32, f32); 5] {
    [(280.0, 0.06 * t, 0.7), (f0, 0.18 * t, 1.0), (f0 * 2.03, 0.14 * t, 0.6), (f0 * 3.1, 0.1 * t, 0.45), (f0 * 4.4, 0.07 * t, 0.3)]
}

/// The ball takes out a beer: struck, it falls on its side, rolls, and foams out.
pub fn can_knock(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.0);
    let b = &mut c.l;
    let f0 = r.r(1150.0, 1320.0);
    modes(b, 0, &can(f0, 1.0), 0.7);
    click(b, 0, r.fork(), 5000.0, 1.0);
    tone(b, 0, 220.0, 120.0, 0.4, 0.001, 0.03, 0.04);
    let down = r.r(0.27, 0.36);
    modes(b, ns(down), &can(f0 * 1.01, 0.7), 0.5);
    click(b, ns(down), r.fork(), 4000.0, 0.7);
    for k in 0..3 {
        let at = ns(down + 0.07 + k as f32 * r.r(0.04, 0.07));
        modes(b, at, &can(f0, 0.3), 0.12);
    }
    // glugging out, and the foam fizzing
    let s = down + 0.05;
    for _ in 0..9 {
        bubble(b, ns(s + r.r(0.0, 0.9)), r.r(380.0, 900.0), r.r(0.015, 0.035), r.r(0.1, 0.25));
    }
    let n = ns(1.5).min(b.len() - ns(s));
    let mut bp = Bq::new(F::Bp, 4200.0, 0.9);
    for k in 0..n {
        let t = k as f32 / RATE;
        let crackle = if r.u() < 0.02 { r.n() } else { 0.0 };
        b[ns(s) + k] += bp.run(r.n() * 0.3 + crackle) * 0.12 * env(t, 0.05, 0.45);
    }
    c.norm(0.9).trim()
}

// ---------- the referee ----------

/// A pea whistle: blasts at (start, end) seconds. The pea spinning in the chamber is the trill.
pub fn whistle(r: &mut Rng, blasts: &[(f32, f32)], force: f32) -> Clip {
    let end = blasts.iter().fold(0.0f32, |m, b| m.max(b.1));
    let mut c = Clip::mono(end + 0.1);
    let fc = r.r(2850.0, 3150.0) * (0.97 + 0.03 * force);
    let pea = r.r(32.0, 44.0) * force;
    let mut breath = Bq::new(F::Bp, fc, 6.0);
    let mut ph = 0.0f32;
    let mut pp = 0.0f32;
    for &(t0, t1) in blasts {
        let len = t1 - t0;
        let n = ns(len);
        let at = ns(t0);
        for k in 0..n.min(c.len() - at) {
            let t = k as f32 / RATE;
            let a = hump(t, len, 0.018, 0.035);
            // blown up to pitch, sagging a little at the end of a long one
            let bend = 0.96 + 0.04 * (t / 0.04).min(1.0) - if len > 0.5 { 0.03 * (t / len).powi(3) } else { 0.0 };
            pp = (pp + pea * (1.0 + 0.1 * r.n()) / RATE).fract();
            let trill = (pp * TAU).sin();
            let f = fc * bend * (1.0 + 0.045 * trill);
            ph = (ph + f / RATE).fract();
            let tone = (ph * TAU).sin() + 0.18 * (ph * 2.0 * TAU).sin() + 0.06 * (ph * 3.0 * TAU).sin();
            let am = 1.0 - 0.45 * (0.5 + 0.5 * trill);
            c.l[at + k] += (tone * am + breath.run(r.n()) * 0.35) * a;
        }
    }
    c.norm(0.85).trim()
}

// ---------- people ----------

/// One gulp from a can: the throat, and air bubbling into the can.
pub fn gulp(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.4);
    let b = &mut c.l;
    click(b, 0, r.fork(), 1400.0, 0.35);
    tone(b, ns(0.008), r.r(160.0, 190.0), r.r(85.0, 100.0), 0.9, 0.004, 0.04, 0.06);
    noise(b, ns(0.01), r.fork(), F::Bp, 900.0, 600.0, 2.0, 0.18, 0.005, 0.05);
    for _ in 0..r.i(3) + 2 {
        bubble(b, ns(r.r(0.04, 0.17)), r.r(420.0, 1100.0), r.r(0.014, 0.03), r.r(0.25, 0.45));
    }
    c.norm(0.85).trim()
}

/// A cough that went down the wrong way: a few bursts.
pub fn cough(r: &mut Rng, v: usize) -> Clip {
    let mut c = Clip::mono(1.0);
    let p0 = if v == 1 { r.r(200.0, 240.0) } else { r.r(105.0, 135.0) };
    let mut t = 0.0;
    let mut a = 1.0;
    for _ in 0..r.i(2) + 2 {
        let at = ns(t);
        noise(&mut c.l, at, r.fork(), F::Bp, 1800.0, 1500.0, 0.8, a * 0.12, 0.001, 0.008);
        let dur = r.r(0.17, 0.24);
        voice(&mut c.l, at, dur, r.fork(), 0.65, |u| (p0 * (1.1 - 0.25 * u), vmix(AH, UH, u), a * env(u * dur, 0.008, 0.06)));
        t += dur + r.r(0.04, 0.1);
        a *= r.r(0.6, 0.85);
    }
    c.norm(0.85).trim()
}

/// A running step: on grass (0), a hard floor (1) or wood (2). Heel, then toe.
pub fn step(r: &mut Rng, floor: u8) -> Clip {
    let mut c = Clip::mono(0.28);
    let b = &mut c.l;
    let toe = ns(r.r(0.03, 0.05));
    match floor {
        0 => {
            noise(b, 0, r.fork(), F::Lp, 450.0, 300.0, 0.7, 0.8, 0.002, 0.02);
            tone(b, 0, 85.0, 60.0, 0.4, 0.002, 0.02, 0.03);
            noise(b, toe, r.fork(), F::Lp, 750.0, 500.0, 0.7, 0.4, 0.002, 0.015);
            // grass crushed underfoot
            for _ in 0..r.i(14) + 14 {
                let at = ns(r.r(0.0, 0.13));
                noise(b, at, r.fork(), F::Bp, r.r(2000.0, 4500.0), 2500.0, 2.5, r.r(0.08, 0.2), 0.0005, 0.004);
            }
        }
        1 => {
            noise(b, 0, r.fork(), F::Bp, r.r(2200.0, 3000.0), 2000.0, 1.5, 1.0, 0.0005, 0.005);
            tone(b, 0, 115.0, 80.0, 0.6, 0.001, 0.015, 0.02);
            noise(b, toe, r.fork(), F::Bp, 3500.0, 3000.0, 1.5, 0.5, 0.0005, 0.004);
            noise(b, toe + ns(0.01), r.fork(), F::Bp, 1800.0, 1200.0, 0.8, 0.12, 0.005, 0.025);
        }
        _ => {
            tone(b, 0, 135.0, 88.0, 0.9, 0.002, 0.03, 0.04);
            modes(b, 0, &[(r.r(180.0, 200.0), 0.08, 0.4), (r.r(400.0, 430.0), 0.05, 0.25)], 1.0);
            click(b, 0, r.fork(), 2000.0, 0.5);
            tone(b, toe, 150.0, 100.0, 0.4, 0.002, 0.02, 0.03);
            click(b, toe, r.fork(), 2600.0, 0.3);
        }
    }
    c.norm(0.9).trim()
}

/// One person's shout: 0 "yeah!", 1 "awww", 2 "oooh!", 3 "wooo!".
fn shout1(b: &mut [f32], at: usize, mut r: Rng, kind: u8, f: bool) {
    let p = if f { r.r(210.0, 280.0) } else { r.r(115.0, 160.0) };
    match kind {
        0 => {
            let dur = r.r(0.5, 0.75);
            voice(b, at, dur, r.fork(), 0.25, |u| {
                let v = if u < 0.18 { vmix(EE, EH, u / 0.18) } else { vmix(EH, AH, (u - 0.18) / 0.25) };
                let pitch = p * 1.6 * (if u < 0.3 { 0.9 + 0.8 * u } else { 1.14 - 0.35 * (u - 0.3) });
                (pitch, v, hump(u, 1.0, 0.06, 0.35))
            });
        }
        1 => {
            let dur = r.r(0.9, 1.2);
            voice(b, at, dur, r.fork(), 0.3, |u| (p * (1.3 - 0.45 * u), vmix(AH, OH, u), hump(u, 1.0, 0.15, 0.4)));
        }
        2 => {
            let dur = r.r(0.65, 0.85);
            voice(b, at, dur, r.fork(), 0.3, |u| (p * (1.15 + 0.35 * (u * PI).sin()), vmix(OO, OH, u), hump(u, 1.0, 0.12, 0.35)));
        }
        _ => {
            let dur = r.r(0.6, 0.9);
            voice(b, at, dur, r.fork(), 0.25, |u| (p * 1.9 * (1.0 + 0.4 * u), vmix(OO, OO, u), hump(u, 1.0, 0.1, 0.3)));
        }
    }
}
/// A team (three of them, sometimes four) shouting together.
pub fn shout(r: &mut Rng, kind: u8) -> Clip {
    let mut c = Clip::stereo(1.6);
    for i in 0..3 + r.i(2) {
        let mut m = vec![0.0; ns(1.4)];
        let k = if kind == 0 && i == 2 && r.chance(0.5) { 3 } else { kind };
        shout1(&mut m, 0, r.fork(), k, r.chance(0.45));
        c.mix_in(&m, ns(r.r(0.0, 0.1)), r.r(0.7, 1.0), r.r(-0.5, 0.5));
    }
    c.norm(0.9).trim()
}

/// One clap of the hands.
fn clap1(b: &mut [f32], at: usize, mut r: Rng, amp: f32) {
    noise(b, at, r.fork(), F::Bp, r.r(900.0, 1800.0), 1000.0, r.r(1.0, 2.0), amp, 0.0008, r.r(0.008, 0.016));
    noise(b, at, r.fork(), F::Bp, 420.0, 400.0, 1.5, amp * 0.4, 0.0008, 0.01);
}
pub fn clap(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.15);
    clap1(&mut c.l, 0, r.fork(), 1.0);
    c.norm(0.9).trim()
}
/// Someone clapping along, a couple of seconds.
pub fn claps(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(3.0);
    let per = r.r(0.3, 0.36);
    for k in 0..8 {
        clap1(&mut c.l, ns(k as f32 * per + r.r(0.0, 0.015)), r.fork(), r.r(0.8, 1.0));
    }
    c.norm(0.9).trim()
}
/// Everybody: cheering, whooping, whistling through their fingers, applause. Made in steps:
/// each of the first ones is a few voices, the last the applause.
pub fn crowd(c: &mut Clip, r: &mut Rng, step: u32) {
    if step == 0 {
        *c = Clip::stereo(3.6);
    }
    if step < 13 {
        let mut m = vec![0.0; ns(1.5)];
        let kind = if r.chance(0.3) { 3 } else { 0 };
        shout1(&mut m, 0, r.fork(), kind, r.chance(0.45));
        c.mix_in(&m, ns(r.r(0.0, 0.5)), r.r(0.5, 1.0), r.n() * 0.9);
        if step % 2 == 0 {
            // and again, a bit later
            let mut m = vec![0.0; ns(1.5)];
            shout1(&mut m, 0, r.fork(), 3, r.chance(0.5));
            c.mix_in(&m, ns(r.r(0.7, 1.8)), r.r(0.3, 0.6), r.n() * 0.9);
        }
    } else if step < 15 {
        // a whistle through the fingers
        let mut m = vec![0.0; ns(1.2)];
        let f0 = r.r(2000.0, 2600.0);
        let n = m.len();
        let mut ph = 0.0f32;
        for k in 0..n {
            let u = k as f32 / n as f32;
            let f = f0 * (0.8 + 0.5 * (u * 3.0).min(1.0) - 0.2 * u);
            ph = (ph + f / RATE).fract();
            m[k] = (ph * TAU).sin() * hump(u, 1.0, 0.1, 0.3) + r.n() * 0.04;
        }
        c.mix_in(&m, ns(r.r(0.2, 1.5)), 0.12, r.n() * 0.8);
    } else {
        for _ in 0..14 {
            let pan = r.n() * 0.9;
            let per = r.r(0.17, 0.24);
            let mut m = vec![0.0; ns(3.6)];
            let mut t = r.r(0.05, 0.3);
            while t < 3.3 {
                clap1(&mut m, ns(t), r.fork(), (1.0 - t / 3.4).max(0.0) * r.r(0.6, 1.0));
                t += per * r.r(0.9, 1.1);
            }
            c.mix_in(&m, 0, 0.2, pan);
        }
        *c = std::mem::take(c).norm(0.9).trim();
    }
}

/// A lighter: the flint, then the flame catching.
pub fn lighter(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.7);
    let b = &mut c.l;
    click(b, 0, r.fork(), 4000.0, 0.4);
    modes(b, 0, &[(4200.0, 0.05, 0.25), (6100.0, 0.03, 0.15)], 1.0);
    noise(b, ns(0.03), r.fork(), F::Bp, 700.0, 900.0, 0.7, 0.35, 0.04, 0.12);
    c.norm(0.8).trim()
}
/// A can opened: the tab cracks, then the "pssht" and fizz.
pub fn can_open(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(1.2);
    let b = &mut c.l;
    click(b, 0, r.fork(), 3000.0, 1.0);
    modes(b, 0, &[(r.r(2600.0, 3000.0), 0.03, 0.5), (5300.0, 0.02, 0.3)], 1.0);
    let at = ns(0.012);
    let n = ns(1.0);
    let mut hp = Bq::new(F::Bp, 3000.0, 0.8);
    let mut bp = Bq::new(F::Bp, 5500.0, 1.2);
    let mut hp2 = Bq::new(F::Bp, 4500.0, 1.0);
    for k in 0..n {
        let t = k as f32 / RATE;
        let w = r.n();
        let fizz = if r.u() < 0.01 * env(t, 0.0, 0.4) { r.n() * 2.0 } else { 0.0 };
        b[at + k] += (hp.run(w) * 0.5 + bp.run(w)) * 0.9 * env(t, 0.004, 0.07) + hp2.run(fizz) * 0.15;
    }
    c.norm(0.85).trim()
}
pub fn slap(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.2);
    noise(&mut c.l, 0, r.fork(), F::Bp, 1400.0, 1100.0, 0.7, 1.0, 0.0006, 0.009);
    noise(&mut c.l, 0, r.fork(), F::Lp, 260.0, 200.0, 0.7, 0.5, 0.001, 0.02);
    c.norm(0.85).trim()
}

// ---------- little tunes ----------

/// A marimba note: the bar's own overtones and the mallet.
fn marimba(b: &mut [f32], at: usize, f: f32, a: f32) {
    modes(b, at, &[(f, 0.6, 1.0), (f * 3.93, 0.16, 0.22), (f * 9.2, 0.05, 0.06)], a);
    mode(b, at, f * 0.5, 0.25, a * 0.15);
}
/// A bell, or a glass struck softly.
fn bell(b: &mut [f32], at: usize, f: f32, a: f32, t: f32) {
    modes(b, at, &[(f, 1.2 * t, 1.0), (f * 2.0, 0.8 * t, 0.4), (f * 3.01, 0.5 * t, 0.25), (f * 4.2, 0.3 * t, 0.12), (f * 5.43, 0.2 * t, 0.08)], a);
}
pub fn hit_sting(r: &mut Rng, v: usize) -> Clip {
    let mut c = Clip::mono(1.2);
    let arps = [[523.25, 659.25, 783.99, 1046.5], [587.33, 739.99, 880.0, 1174.66]];
    for (k, f) in arps[v % 2].iter().enumerate() {
        marimba(&mut c.l, ns(k as f32 * 0.065), *f, 0.8);
    }
    marimba(&mut c.l, 0, arps[v % 2][0] / 2.0, 0.6);
    let _ = r;
    c.norm(0.8).trim()
}
pub fn miss_sting(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(1.0);
    marimba(&mut c.l, 0, 392.0, 0.8);
    marimba(&mut c.l, ns(0.14), 311.13, 0.8);
    let _ = r;
    c.norm(0.6).trim()
}
/// A brass section: detuned saws, the filter opening with each note.
fn brass(b: &mut [f32], at: usize, f: f32, dur: f32, a: f32, r: &mut Rng) {
    let n = ns(dur + 0.25).min(b.len().saturating_sub(at));
    let mut lp = Bq::new(F::Lp, 800.0, 1.2);
    let det = [1.0, 1.006, 0.995];
    let mut ph = [r.u(), r.u(), r.u()];
    for k in 0..n {
        let t = k as f32 / RATE;
        let e = if t < dur { (t / 0.03).min(1.0) * (0.85 + 0.15 * (-t / 0.1).exp()) } else { (-(t - dur) / 0.06).exp() };
        if k % 16 == 0 {
            lp.set(F::Lp, 500.0 + 3500.0 * e * (0.6 + 0.4 * (-t / 0.15).exp()), 1.2);
        }
        let vib = 1.0 + if t > 0.3 { 0.006 * (TAU * 5.5 * t).sin() } else { 0.0 };
        let mut s = 0.0;
        for i in 0..3 {
            ph[i] = (ph[i] + f * det[i] * vib / RATE).fract();
            s += 2.0 * ph[i] - 1.0;
        }
        b[at + k] += lp.run(s) * e * a;
    }
}
pub fn win_sting(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.2);
    for k in 0..3 {
        for f in [392.0, 493.88] {
            brass(&mut c.l, ns(k as f32 * 0.15), f, 0.1, 0.5, r);
        }
    }
    for f in [523.25, 659.25, 783.99, 261.63] {
        brass(&mut c.l, ns(0.45), f, 1.1, 0.5, r);
    }
    // a timpani under the last chord
    tone(&mut c.l, ns(0.45), 98.0, 88.0, 1.5, 0.002, 0.35, 0.3);
    noise(&mut c.l, ns(0.45), r.fork(), F::Lp, 300.0, 200.0, 0.7, 0.6, 0.001, 0.05);
    c.norm(0.8).trim()
}
/// The sad trombone: wah, wah, wah, waaah.
pub fn lose_sting(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.6);
    let notes = [(0.0, 0.32, 293.66), (0.4, 0.32, 277.18), (0.8, 0.32, 261.63), (1.2, 1.0, 246.94)];
    for (t, d, f) in notes {
        let at = ns(t);
        let n = ns(d + 0.1);
        let mut lp = Bq::new(F::Lp, 600.0, 3.0);
        let mut ph = 0.0f32;
        for k in 0..n.min(c.len() - at) {
            let tt = k as f32 / RATE;
            let e = hump(tt, d + 0.08, 0.04, 0.12);
            let wah = (tt / 0.18).min(1.0);
            if k % 16 == 0 {
                lp.set(F::Lp, 350.0 + 1300.0 * wah * e, 3.0);
            }
            let vib = if d > 0.5 { 1.0 + 0.02 * (TAU * 6.0 * tt).sin() * (tt / 0.3).min(1.0) } else { 1.0 };
            ph = (ph + f * 0.5 * vib / RATE).fract();
            c.l[at + k] += lp.run(2.0 * ph - 1.0) * e;
        }
    }
    let _ = r;
    c.norm(0.75).trim()
}
/// The curve armed: a short rising zip.
pub fn spin(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.35);
    let n = ns(0.24);
    let mut bp = Bq::new(F::Bp, 400.0, 4.0);
    let mut ph = 0.0f32;
    for k in 0..n {
        let u = k as f32 / n as f32;
        let f = 300.0 * (5.0f32).powf(u);
        ph = (ph + f / RATE).fract();
        if k % 16 == 0 {
            bp.set(F::Bp, f * 2.0, 4.0);
        }
        c.l[k] += bp.run(2.0 * ph - 1.0 + r.n() * 0.3) * hump(u, 1.0, 0.2, 0.3);
    }
    click(&mut c.l, n, r.fork(), 5000.0, 0.4);
    c.norm(0.6).trim()
}

// ---------- the menus ----------

/// A soft wooden tap (`p` scales the pitch).
pub fn tap(r: &mut Rng, p: f32) -> Clip {
    let mut c = Clip::mono(0.12);
    let f = r.r(1050.0, 1150.0) * p;
    modes(&mut c.l, 0, &[(f, 0.05, 1.0), (f * 2.7, 0.02, 0.3)], 1.0);
    click(&mut c.l, 0, r.fork(), 4000.0, 0.5);
    c.norm(0.8).trim()
}
/// The big buttons: a rounder "thock".
pub fn tap_big(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.2);
    tone(&mut c.l, 0, 520.0, 360.0, 1.0, 0.002, 0.03, 0.04);
    modes(&mut c.l, 0, &[(780.0, 0.07, 0.45), (1560.0, 0.03, 0.15)], 1.0);
    click(&mut c.l, 0, r.fork(), 3000.0, 0.4);
    c.norm(0.8).trim()
}
pub fn tick(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.06);
    mode(&mut c.l, 0, r.r(2500.0, 2700.0), 0.015, 1.0);
    click(&mut c.l, 0, r.fork(), 6000.0, 0.4);
    c.norm(0.6).trim()
}
pub fn toggle(r: &mut Rng, on: bool) -> Clip {
    let mut c = Clip::mono(0.2);
    let (a, b) = if on { (900.0, 1350.0) } else { (1350.0, 900.0) };
    modes(&mut c.l, 0, &[(a, 0.04, 1.0)], 1.0);
    modes(&mut c.l, ns(0.06), &[(b, 0.06, 1.0)], 1.0);
    click(&mut c.l, 0, r.fork(), 5000.0, 0.3);
    c.norm(0.7).trim()
}
/// A sheet sliding up (or away).
pub fn swish(r: &mut Rng, up: bool) -> Clip {
    let mut c = Clip::mono(0.25);
    let (f0, f1) = if up { (500.0, 2600.0) } else { (2600.0, 500.0) };
    let n = ns(0.2);
    let mut bp = Bq::new(F::Bp, f0, 1.2);
    for k in 0..n {
        let u = k as f32 / n as f32;
        if k % 16 == 0 {
            bp.set(F::Bp, f0 * (f1 / f0).powf(u), 1.2);
        }
        c.l[k] += bp.run(r.n()) * (u * PI).sin().powi(2);
    }
    c.norm(0.4).trim()
}
/// Something's come in: two bell notes.
pub fn chime(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(1.8);
    bell(&mut c.l, 0, 1318.5, 0.8, 0.8);
    bell(&mut c.l, ns(0.13), 1975.5, 0.6, 0.8);
    let _ = r;
    c.norm(0.6).trim()
}
/// A message bubble: a pop, and a smaller one.
pub fn pop(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.3);
    tone(&mut c.l, 0, 520.0, 1250.0, 1.0, 0.002, 0.025, 0.04);
    tone(&mut c.l, ns(0.07), 760.0, 1700.0, 0.6, 0.002, 0.02, 0.035);
    let _ = r;
    c.norm(0.6).trim()
}
/// Up a level: rising bells and a sparkle.
pub fn level_up(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(2.0);
    for (k, f) in [1046.5, 1318.5, 1568.0, 2093.0].iter().enumerate() {
        bell(&mut c.l, ns(k as f32 * 0.08), *f, 0.6, 0.7);
    }
    for _ in 0..30 {
        let at = ns(r.r(0.25, 1.2));
        mode(&mut c.l, at, r.r(4000.0, 8000.0), 0.08, r.r(0.03, 0.08));
    }
    c.norm(0.6).trim()
}
pub fn denied(r: &mut Rng) -> Clip {
    let mut c = Clip::mono(0.35);
    for k in 0..2 {
        modes(&mut c.l, ns(k as f32 * 0.11), &[(220.0, 0.08, 1.0), (440.0, 0.04, 0.3)], 1.0);
        click(&mut c.l, ns(k as f32 * 0.11), r.fork(), 1500.0, 0.3);
    }
    c.norm(0.6).trim()
}
