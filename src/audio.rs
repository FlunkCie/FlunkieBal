//! Synthesised sound effects (no audio files): through Web Audio in the browser, through our own
//! little synth on the desktop (cpal) and on Android (AAudio).

use crate::game::Sfx;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Wave {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Filter {
    Lowpass,
    Highpass,
    Bandpass,
}
/// What can play the score below.
pub trait Sink {
    fn tone(&mut self, f0: f32, f1: f32, dur: f32, w: Wave, vol: f32, delay: f32);
    fn noise(&mut self, dur: f32, vol: f32, f0: f32, f1: f32, f: Filter, q: f32, delay: f32);
}
/// What every sound is made of.
pub fn score(s: Sfx, o: &mut impl Sink) {
    match s {
        Sfx::Throw => o.noise(0.28, 0.35, 500.0, 1800.0, Filter::Bandpass, 1.2, 0.0),
        Sfx::Bounce(i) => {
            let v = i.clamp(0.05, 1.0);
            o.tone(140.0, 60.0, 0.14, Wave::Sine, 0.6 * v, 0.0);
            o.noise(0.06, 0.25 * v, 900.0, 300.0, Filter::Lowpass, 0.7, 0.0);
        }
        Sfx::Touch(i) => {
            let v = i.clamp(0.2, 1.0);
            o.tone(880.0, 520.0, 0.07, Wave::Triangle, 0.5 * v, 0.0);
            o.noise(0.05, 0.3 * v, 3000.0, 1200.0, Filter::Bandpass, 2.0, 0.0);
        }
        Sfx::Bottle(i) => {
            let v = i.clamp(0.2, 1.0);
            o.tone(320.0, 180.0, 0.12, Wave::Triangle, 0.45 * v, 0.0);
            o.noise(0.08, 0.25 * v, 1500.0, 500.0, Filter::Bandpass, 1.5, 0.0);
        }
        Sfx::CanHit => {
            o.tone(1900.0, 1400.0, 0.12, Wave::Square, 0.12, 0.0);
            o.noise(0.25, 0.35, 4000.0, 1500.0, Filter::Highpass, 0.8, 0.0);
        }
        Sfx::Whistle => {
            o.tone(2600.0, 2550.0, 0.22, Wave::Sine, 0.28, 0.0);
            o.tone(2650.0, 2500.0, 0.35, Wave::Sine, 0.28, 0.25);
        }
        Sfx::Stop => {
            o.tone(2500.0, 2450.0, 0.55, Wave::Sine, 0.3, 0.0);
            o.tone(2560.0, 2520.0, 0.55, Wave::Sine, 0.12, 0.0);
        }
        Sfx::Glug => o.tone(190.0, 85.0, 0.12, Wave::Sine, 0.4, 0.0),
        Sfx::Cough => {
            for k in 0..3 {
                o.noise(0.13, 0.4, 900.0, 400.0, Filter::Bandpass, 1.4, k as f32 * 0.2);
            }
        }
        Sfx::Step => o.noise(0.05, 0.18, 700.0, 250.0, Filter::Lowpass, 0.7, 0.0),
        Sfx::Hit => {
            for (k, f) in [523.0f32, 659.0, 784.0, 1047.0].iter().enumerate() {
                o.tone(*f, *f, 0.18, Wave::Triangle, 0.25, k as f32 * 0.07);
            }
        }
        Sfx::Straf => {
            o.tone(2600.0, 2500.0, 0.6, Wave::Sine, 0.3, 0.0);
            o.tone(150.0, 90.0, 0.45, Wave::Sawtooth, 0.12, 0.05);
        }
        Sfx::Spin => o.tone(400.0, 1600.0, 0.25, Wave::Triangle, 0.2, 0.0),
        Sfx::Miss => {
            o.tone(392.0, 370.0, 0.18, Wave::Triangle, 0.18, 0.0);
            o.tone(311.0, 290.0, 0.3, Wave::Triangle, 0.18, 0.16);
        }
        Sfx::Win => {
            for (k, f) in [523.0f32, 659.0, 784.0, 1047.0, 784.0, 1047.0].iter().enumerate() {
                o.tone(*f, *f, 0.22, Wave::Square, 0.1, k as f32 * 0.11);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{score, Filter, Sfx, Sink, Wave};
    use web_sys::{AudioContext, BiquadFilterType, OscillatorType};

    pub struct Audio {
        ctx: Option<AudioContext>,
        noise: Option<web_sys::AudioBuffer>,
        pub volume: f32,
    }
    impl Audio {
        pub fn new() -> Self {
            Audio { ctx: None, noise: None, volume: 0.8 }
        }
        fn ctx(&mut self) -> Option<AudioContext> {
            if self.ctx.is_none() {
                let c = AudioContext::new().ok()?;
                let sr = c.sample_rate();
                let len = (sr * 1.0) as u32;
                if let Ok(buf) = c.create_buffer(1, len, sr) {
                    let mut seed = 22222u32;
                    let data: Vec<f32> = (0..len)
                        .map(|_| {
                            seed ^= seed << 13;
                            seed ^= seed >> 17;
                            seed ^= seed << 5;
                            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
                        })
                        .collect();
                    let _ = buf.copy_to_channel(&data, 0);
                    self.noise = Some(buf);
                }
                self.ctx = Some(c);
            }
            let c = self.ctx.clone()?;
            if c.state() == web_sys::AudioContextState::Suspended {
                let _ = c.resume();
            }
            Some(c)
        }
        /// Call from a user gesture so browsers let the sound start.
        pub fn unlock(&mut self) {
            let _ = self.ctx();
        }
        fn tone(&mut self, f0: f32, f1: f32, dur: f32, ty: OscillatorType, vol: f32, delay: f32) {
            let Some(c) = self.ctx() else { return };
            let v = vol * self.volume;
            if v <= 0.001 {
                return;
            }
            let t0 = c.current_time() + delay as f64;
            let (Ok(o), Ok(gn)) = (c.create_oscillator(), c.create_gain()) else { return };
            o.set_type(ty);
            let _ = o.frequency().set_value_at_time(f0, t0);
            let _ = o.frequency().exponential_ramp_to_value_at_time(f1.max(20.0), t0 + dur as f64);
            let _ = gn.gain().set_value_at_time(0.0001, t0);
            let _ = gn.gain().linear_ramp_to_value_at_time(v, t0 + 0.008);
            let _ = gn.gain().exponential_ramp_to_value_at_time(0.0001, t0 + dur as f64);
            let _ = o.connect_with_audio_node(&gn);
            let _ = gn.connect_with_audio_node(&c.destination());
            let _ = o.start_with_when(t0);
            let _ = o.stop_with_when(t0 + dur as f64 + 0.05);
        }
        fn noise(&mut self, dur: f32, vol: f32, f0: f32, f1: f32, ty: BiquadFilterType, q: f32, delay: f32) {
            let Some(c) = self.ctx() else { return };
            let Some(buf) = self.noise.clone() else { return };
            let v = vol * self.volume;
            if v <= 0.001 {
                return;
            }
            let t0 = c.current_time() + delay as f64;
            let (Ok(src), Ok(fl), Ok(gn)) = (c.create_buffer_source(), c.create_biquad_filter(), c.create_gain()) else { return };
            src.set_buffer(Some(&buf));
            fl.set_type(ty);
            fl.q().set_value(q);
            let _ = fl.frequency().set_value_at_time(f0, t0);
            let _ = fl.frequency().exponential_ramp_to_value_at_time(f1.max(20.0), t0 + dur as f64);
            let _ = gn.gain().set_value_at_time(0.0001, t0);
            let _ = gn.gain().linear_ramp_to_value_at_time(v, t0 + 0.01);
            let _ = gn.gain().exponential_ramp_to_value_at_time(0.0001, t0 + dur as f64);
            let _ = src.connect_with_audio_node(&fl);
            let _ = fl.connect_with_audio_node(&gn);
            let _ = gn.connect_with_audio_node(&c.destination());
            let _ = src.start_with_when_and_grain_offset(t0, (t0 * 7.3) % 0.5);
            let _ = src.stop_with_when(t0 + dur as f64 + 0.05);
        }
        pub fn play(&mut self, s: Sfx) {
            score(s, self);
        }
    }
    impl Sink for Audio {
        fn tone(&mut self, f0: f32, f1: f32, dur: f32, w: Wave, vol: f32, delay: f32) {
            let ty = match w {
                Wave::Sine => OscillatorType::Sine,
                Wave::Triangle => OscillatorType::Triangle,
                Wave::Square => OscillatorType::Square,
                Wave::Sawtooth => OscillatorType::Sawtooth,
            };
            Audio::tone(self, f0, f1, dur, ty, vol, delay);
        }
        fn noise(&mut self, dur: f32, vol: f32, f0: f32, f1: f32, f: Filter, q: f32, delay: f32) {
            let ty = match f {
                Filter::Lowpass => BiquadFilterType::Lowpass,
                Filter::Highpass => BiquadFilterType::Highpass,
                Filter::Bandpass => BiquadFilterType::Bandpass,
            };
            Audio::noise(self, dur, vol, f0, f1, ty, q, delay);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod synth {
    //! The same tones and filtered noise Web Audio makes, rendered by hand.
    use super::{Filter, Sink, Wave};
    use std::f32::consts::TAU;

    const FLOOR: f32 = 0.0001;
    const CHUNK: u32 = 16;

    enum Kind {
        Tone { wave: Wave, phase: f32 },
        Noise { filter: Filter, q: f32, seed: u32, z: [f32; 2], c: [f32; 5] },
    }
    struct Voice {
        kind: Kind,
        f0: f32,
        f1: f32,
        dur: f32,
        vol: f32,
        attack: f32,
        wait: f32,
        t: f32,
        n: u32,
    }
    #[derive(Default)]
    pub struct Synth {
        voices: Vec<Voice>,
        pub volume: f32,
        seed: u32,
    }
    /// Filter coefficients (audio EQ cookbook).
    fn biquad(f: Filter, freq: f32, q: f32, rate: f32) -> [f32; 5] {
        let w = TAU * (freq / rate).min(0.45);
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q.max(0.1));
        let (b0, b1, b2) = match f {
            Filter::Lowpass => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0),
            Filter::Highpass => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0),
            Filter::Bandpass => (al, 0.0, -al),
        };
        let a0 = 1.0 + al;
        [b0 / a0, b1 / a0, b2 / a0, -2.0 * c / a0, (1.0 - al) / a0]
    }
    impl Synth {
        pub fn busy(&self) -> bool {
            !self.voices.is_empty()
        }
        /// Add this synth's sound for the next samples to `out` (which starts out silent).
        pub fn render(&mut self, out: &mut [f32], rate: f32) {
            let h = 1.0 / rate;
            for v in self.voices.iter_mut() {
                let mut skip = 0;
                if v.wait > 0.0 {
                    skip = ((v.wait * rate) as usize).min(out.len());
                    v.wait -= skip as f32 * h;
                }
                for x in out[skip..].iter_mut() {
                    if v.t >= v.dur {
                        break;
                    }
                    let u = v.t / v.dur;
                    let freq = v.f0 * (v.f1.max(20.0) / v.f0).powf(u);
                    // up to full volume quickly, then an exponential fade, as `exponentialRamp` does
                    let env = if v.t < v.attack { v.vol * v.t / v.attack } else { v.vol * (FLOOR / v.vol).powf((v.t - v.attack) / (v.dur - v.attack).max(1e-4)) };
                    let y = match &mut v.kind {
                        Kind::Tone { wave, phase } => {
                            *phase = (*phase + freq * h).fract();
                            let p = *phase;
                            match wave {
                                Wave::Sine => (p * TAU).sin(),
                                Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
                                Wave::Square => if p < 0.5 { 1.0 } else { -1.0 },
                                Wave::Sawtooth => 2.0 * p - 1.0,
                            }
                        }
                        Kind::Noise { filter, q, seed, z, c } => {
                            if v.n % CHUNK == 0 {
                                *c = biquad(*filter, freq, *q, rate);
                            }
                            *seed ^= *seed << 13;
                            *seed ^= *seed >> 17;
                            *seed ^= *seed << 5;
                            let i = (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
                            let o = c[0] * i + z[0];
                            z[0] = c[1] * i - c[3] * o + z[1];
                            z[1] = c[2] * i - c[4] * o;
                            o
                        }
                    };
                    *x += y * env;
                    v.t += h;
                    v.n += 1;
                }
            }
            self.voices.retain(|v| v.t < v.dur);
            for x in out.iter_mut() {
                // several sounds at once bend instead of clipping
                *x = x.tanh();
            }
        }
    }
    impl Sink for Synth {
        fn tone(&mut self, f0: f32, f1: f32, dur: f32, wave: Wave, vol: f32, delay: f32) {
            let vol = vol * self.volume;
            if vol > 0.001 {
                self.voices.push(Voice { kind: Kind::Tone { wave, phase: 0.0 }, f0, f1, dur, vol, attack: 0.008, wait: delay, t: 0.0, n: 0 });
            }
        }
        fn noise(&mut self, dur: f32, vol: f32, f0: f32, f1: f32, filter: Filter, q: f32, delay: f32) {
            let vol = vol * self.volume;
            if vol > 0.001 {
                self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223) | 1;
                self.voices.push(Voice { kind: Kind::Noise { filter, q, seed: self.seed, z: [0.0; 2], c: [0.0; 5] }, f0, f1, dur, vol, attack: 0.01, wait: delay, t: 0.0, n: 0 });
            }
        }
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::{score, synth::Synth, Sfx};
    use ndk::audio::{AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// Silent this long: let go of the audio device, also so it isn't held while the app is in the background.
    const IDLE: f32 = 3.0;

    pub struct Audio {
        synth: Arc<Mutex<Synth>>,
        stream: Option<AudioStream>,
        /// the callback stopped itself, or the device went away (headphones out)
        idle: Arc<AtomicBool>,
        broken: Arc<AtomicBool>,
        pub volume: f32,
    }
    impl Audio {
        pub fn new() -> Self {
            Audio { synth: Arc::new(Mutex::new(Synth::default())), stream: None, idle: Arc::new(AtomicBool::new(true)), broken: Arc::new(AtomicBool::new(false)), volume: 0.8 }
        }
        fn open(&mut self) -> Result<AudioStream, String> {
            let (synth, idle, broken) = (self.synth.clone(), self.idle.clone(), self.broken.clone());
            let mut quiet = 0.0f32;
            let builder = AudioStreamBuilder::new().map_err(|e| format!("{e:?}"))?;
            builder
                .direction(AudioDirection::Output)
                .format(AudioFormat::PCM_Float)
                .channel_count(1)
                .sharing_mode(AudioSharingMode::Shared)
                .performance_mode(AudioPerformanceMode::LowLatency)
                .data_callback(Box::new(move |st, data, frames| {
                    let out = unsafe { std::slice::from_raw_parts_mut(data as *mut f32, frames as usize) };
                    out.fill(0.0);
                    let rate = st.sample_rate() as f32;
                    // never wait in the audio thread: a frame of silence beats a glitch
                    if let Ok(mut s) = synth.try_lock() {
                        s.render(out, rate);
                        quiet = if s.busy() { 0.0 } else { quiet + frames as f32 / rate };
                    }
                    if quiet > IDLE {
                        idle.store(true, Ordering::Release);
                        AudioCallbackResult::Stop
                    } else {
                        AudioCallbackResult::Continue
                    }
                }))
                .error_callback(Box::new(move |_st, e| {
                    log::warn!("audio device error: {e:?}");
                    broken.store(true, Ordering::Release);
                }))
                .open_stream()
                .map_err(|e| format!("{e:?}"))
        }
        /// Make sure the stream is running.
        fn wake(&mut self) {
            if self.broken.swap(false, Ordering::AcqRel) {
                self.stream = None;
                self.idle.store(true, Ordering::Release);
            }
            if !self.idle.load(Ordering::Acquire) {
                return;
            }
            if self.stream.is_none() {
                match self.open() {
                    Ok(s) => self.stream = Some(s),
                    Err(e) => {
                        log::warn!("no audio: {e}");
                        return;
                    }
                }
            }
            if let Some(s) = self.stream.as_ref() {
                match s.request_start() {
                    Ok(()) => self.idle.store(false, Ordering::Release),
                    Err(e) => {
                        log::warn!("audio didn't start: {e:?}");
                        self.stream = None;
                    }
                }
            }
        }
        pub fn unlock(&mut self) {}
        pub fn play(&mut self, s: Sfx) {
            if let Ok(mut y) = self.synth.lock() {
                y.volume = self.volume;
                score(s, &mut *y);
            }
            self.wake();
        }
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod imp {
    use super::{score, synth::Synth, Sfx};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub struct Audio {
        synth: Arc<Mutex<Synth>>,
        stream: Option<cpal::Stream>,
        broken: Arc<AtomicBool>,
        /// no device, or it wouldn't open: don't try on every sound
        failed: bool,
        pub volume: f32,
    }
    /// The synth is mono; every channel gets the same.
    fn fill<T: cpal::SizedSample>(data: &mut [T], channels: usize, rate: f32, mono: &mut Vec<f32>, synth: &Mutex<Synth>, conv: impl Fn(f32) -> T) {
        mono.clear();
        mono.resize(data.len() / channels.max(1), 0.0);
        // never wait in the audio thread: a moment of silence beats a glitch
        if let Ok(mut s) = synth.try_lock() {
            s.render(mono, rate);
        }
        for (frame, x) in data.chunks_mut(channels.max(1)).zip(mono.iter()) {
            frame.fill(conv(*x));
        }
    }
    impl Audio {
        pub fn new() -> Self {
            Audio { synth: Arc::new(Mutex::new(Synth::default())), stream: None, broken: Arc::new(AtomicBool::new(false)), failed: false, volume: 0.8 }
        }
        fn open(&mut self) -> Result<cpal::Stream, String> {
            let dev = cpal::default_host().default_output_device().ok_or("no audio output device")?;
            let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
            let (rate, channels) = (cfg.sample_rate().0 as f32, cfg.channels() as usize);
            let (sc, fmt) = (cfg.config(), cfg.sample_format());
            let broken = self.broken.clone();
            let on_err = move |e: cpal::StreamError| {
                crate::app::log_str(&format!("audio device error: {e}"));
                broken.store(true, Ordering::Release);
            };
            let synth = self.synth.clone();
            let mut mono = Vec::new();
            let stream = match fmt {
                cpal::SampleFormat::F32 => dev.build_output_stream(&sc, move |d: &mut [f32], _: &_| fill(d, channels, rate, &mut mono, &synth, |x| x), on_err, None),
                cpal::SampleFormat::I16 => dev.build_output_stream(&sc, move |d: &mut [i16], _: &_| fill(d, channels, rate, &mut mono, &synth, |x| (x * 32767.0) as i16), on_err, None),
                cpal::SampleFormat::U16 => dev.build_output_stream(&sc, move |d: &mut [u16], _: &_| fill(d, channels, rate, &mut mono, &synth, |x| ((x * 0.5 + 0.5) * 65535.0) as u16), on_err, None),
                other => return Err(format!("unsupported sample format {other:?}")),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            crate::app::log_str(&format!("audio: {channels} channels at {rate} Hz"));
            Ok(stream)
        }
        pub fn unlock(&mut self) {}
        pub fn play(&mut self, s: Sfx) {
            if self.broken.swap(false, Ordering::AcqRel) {
                // headphones out and the like: open whatever is the default now
                self.stream = None;
                self.failed = false;
            }
            if self.stream.is_none() && !self.failed {
                match self.open() {
                    Ok(st) => self.stream = Some(st),
                    Err(e) => {
                        crate::app::log_str(&format!("no sound: {e}"));
                        self.failed = true;
                    }
                }
            }
            if let Ok(mut y) = self.synth.lock() {
                y.volume = self.volume;
                score(s, &mut *y);
            }
        }
    }
}

pub use imp::Audio;

#[cfg(test)]
mod tests {
    use super::synth::Synth;
    use super::*;
    fn sound(s: Sfx) -> Vec<f32> {
        let mut y = Synth::default();
        y.volume = 1.0;
        score(s, &mut y);
        let mut all = vec![];
        // in small bursts, like an audio callback
        for _ in 0..200 {
            let mut b = [0.0f32; 480];
            y.render(&mut b, 48000.0);
            all.extend_from_slice(&b);
        }
        assert!(!y.busy(), "{s:?} rings on");
        all
    }
    #[test]
    fn every_sound_is_audible_and_clean() {
        for s in [Sfx::Throw, Sfx::Bounce(0.5), Sfx::Touch(0.5), Sfx::Bottle(0.5), Sfx::CanHit, Sfx::Whistle, Sfx::Stop, Sfx::Glug, Sfx::Cough, Sfx::Step, Sfx::Hit, Sfx::Straf, Sfx::Spin, Sfx::Miss, Sfx::Win] {
            let a = sound(s);
            let peak = a.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(a.iter().all(|x| x.is_finite()), "{s:?} not finite");
            assert!(peak > 0.02 && peak <= 1.0, "{s:?} peak {peak}");
        }
    }
    #[test]
    fn delays_are_kept() {
        // the second note of the miss starts 0.16 s in, the first has faded a lot by 0.17
        let a = sound(Sfx::Miss);
        let rms = |r: std::ops::Range<usize>| (a[r.clone()].iter().map(|x| x * x).sum::<f32>() / r.len() as f32).sqrt();
        assert!(rms(9000..9600) > rms(7000..7600) * 1.5, "second note arrives");
    }
    #[test]
    fn a_tone_has_its_pitch() {
        // STOP is a 2.5 kHz whistle, plus a quieter one close by
        let a = sound(Sfx::Stop);
        let z = a[4800..9600].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((230..=270).contains(&z), "{z} crossings in 0.1 s of a 2.5 kHz whistle");
    }
}
