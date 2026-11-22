//! The sound device: AAudio on Android, cpal on the desktop. Both run `Mixer` in their callback.

use super::dsp::Clip;
use super::mixer::Mixer;
use super::Play;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "android")]
mod imp {
    use super::Mixer;
    use ndk::audio::{AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};

    /// Silent this long: let go of the audio device, also so it isn't held while the app is in the background.
    const IDLE: f32 = 3.0;

    /// Opening and starting the stream happens on a thread of its own: on a phone either can take
    /// tens or hundreds of milliseconds, and done in the game's frame that was a hitch.
    pub struct Dev {
        idle: Arc<AtomicBool>,
        broken: Arc<AtomicBool>,
        wake: Option<mpsc::Sender<()>>,
    }
    struct Device {
        mixer: Arc<Mutex<Mixer>>,
        stream: Option<AudioStream>,
        idle: Arc<AtomicBool>,
        broken: Arc<AtomicBool>,
    }
    impl Device {
        fn open(&mut self) -> Result<AudioStream, String> {
            let (mixer, idle, broken) = (self.mixer.clone(), self.idle.clone(), self.broken.clone());
            let mut quiet = 0.0f32;
            let builder = AudioStreamBuilder::new().map_err(|e| format!("{e:?}"))?;
            builder
                .direction(AudioDirection::Output)
                .format(AudioFormat::PCM_Float)
                .channel_count(2)
                .sharing_mode(AudioSharingMode::Shared)
                .performance_mode(AudioPerformanceMode::LowLatency)
                .data_callback(Box::new(move |st, data, frames| {
                    let ch = st.channel_count().max(1) as usize;
                    let out = unsafe { std::slice::from_raw_parts_mut(data as *mut f32, frames as usize * ch) };
                    out.fill(0.0);
                    let rate = st.sample_rate() as f32;
                    // never wait in the audio thread: a frame of silence beats a glitch
                    if let Ok(mut m) = mixer.try_lock() {
                        m.render(out, ch, rate, |x| x);
                        quiet = if m.busy() { 0.0 } else { quiet + frames as f32 / rate };
                    }
                    if quiet > IDLE {
                        quiet = 0.0;
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
        /// Have a stream ready (opening is the slow part), without starting it.
        fn ready(&mut self) {
            if self.broken.swap(false, Ordering::AcqRel) {
                self.stream = None;
                self.idle.store(true, Ordering::Release);
            }
            if self.stream.is_none() {
                let t = std::time::Instant::now();
                match self.open() {
                    Ok(s) => {
                        log::info!("audio: stream opened in {:.0} ms (on its own thread)", t.elapsed().as_secs_f64() * 1000.0);
                        self.stream = Some(s);
                    }
                    Err(e) => log::warn!("no audio: {e}"),
                }
            }
        }
        /// Make sure the stream is running.
        fn wake(&mut self) {
            self.ready();
            if !self.idle.load(Ordering::Acquire) {
                return;
            }
            if let Some(s) = self.stream.as_ref() {
                match s.request_start() {
                    Ok(()) => self.idle.store(false, Ordering::Release),
                    Err(e) => {
                        log::warn!("audio didn't start: {e:?}");
                        // try a fresh one, now rather than on the next sound
                        self.stream = None;
                        self.ready();
                        if let Some(s) = self.stream.as_ref() {
                            if s.request_start().is_ok() {
                                self.idle.store(false, Ordering::Release);
                            }
                        }
                    }
                }
            }
        }
    }
    impl Dev {
        pub fn new(mixer: Arc<Mutex<Mixer>>) -> Dev {
            let (idle, broken) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)));
            let (tx, rx) = mpsc::channel::<()>();
            let (id, br) = (idle.clone(), broken.clone());
            let ok = std::thread::Builder::new()
                .name("flunki-audio".into())
                .spawn(move || {
                    // the stream lives and dies on this thread
                    let mut dev = Device { mixer, stream: None, idle: id, broken: br };
                    dev.ready();
                    while rx.recv().is_ok() {
                        // a burst of sounds is one wake-up
                        while rx.try_recv().is_ok() {}
                        dev.wake();
                    }
                })
                .is_ok();
            Dev { idle, broken, wake: ok.then_some(tx) }
        }
        /// There's something to hear: make sure the stream runs.
        pub fn poke(&mut self) {
            // only the audio thread touches the device; tell it when there's something to do
            if self.idle.load(Ordering::Acquire) || self.broken.load(Ordering::Acquire) {
                if let Some(w) = self.wake.as_ref() {
                    let _ = w.send(());
                }
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::Mixer;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub struct Dev {
        mixer: Arc<Mutex<Mixer>>,
        stream: Option<cpal::Stream>,
        broken: Arc<AtomicBool>,
        /// no device, or it wouldn't open: don't try on every sound
        failed: bool,
    }
    fn fill<T: cpal::SizedSample>(data: &mut [T], channels: usize, rate: f32, mixer: &Mutex<Mixer>, conv: impl Fn(f32) -> T) {
        // never wait in the audio thread: a moment of silence beats a glitch
        match mixer.try_lock() {
            Ok(mut m) => m.render(data, channels, rate, conv),
            Err(_) => data.iter_mut().for_each(|x| *x = conv(0.0)),
        }
    }
    impl Dev {
        pub fn new(mixer: Arc<Mutex<Mixer>>) -> Dev {
            Dev { mixer, stream: None, broken: Arc::new(AtomicBool::new(false)), failed: false }
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
            let m = self.mixer.clone();
            let stream = match fmt {
                cpal::SampleFormat::F32 => dev.build_output_stream(&sc, move |d: &mut [f32], _: &_| fill(d, channels, rate, &m, |x| x), on_err, None),
                cpal::SampleFormat::I16 => dev.build_output_stream(&sc, move |d: &mut [i16], _: &_| fill(d, channels, rate, &m, |x| (x * 32767.0) as i16), on_err, None),
                cpal::SampleFormat::U16 => dev.build_output_stream(&sc, move |d: &mut [u16], _: &_| fill(d, channels, rate, &m, |x| ((x * 0.5 + 0.5) * 65535.0) as u16), on_err, None),
                other => return Err(format!("unsupported sample format {other:?}")),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            crate::app::log_str(&format!("audio: {channels} channels at {rate} Hz"));
            Ok(stream)
        }
        pub fn poke(&mut self) {
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
        }
    }
}

/// The platform side of `Audio`: the mixer and the device it plays on.
pub struct Out {
    mixer: Arc<Mutex<Mixer>>,
    dev: imp::Dev,
}
impl Out {
    pub fn new() -> Out {
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        Out { dev: imp::Dev::new(mixer.clone()), mixer }
    }
    pub fn unlock(&mut self) {}
    pub fn ready(&self) -> bool {
        true
    }
    fn with(&self, f: impl FnOnce(&mut Mixer)) {
        if let Ok(mut m) = self.mixer.lock() {
            f(&mut m);
        }
    }
    pub fn add(&mut self, id: usize, c: &Arc<Clip>) {
        self.with(|m| m.add(id, c.clone()));
    }
    pub fn play(&mut self, id: usize, p: Play) {
        self.with(|m| m.play(id, p));
        self.dev.poke();
    }
    pub fn bed(&mut self, slot: usize, id: Option<usize>, p: Play) {
        self.with(|m| m.bed(slot, id, p));
    }
    pub fn room(&mut self, t60: f32, damp: f32) {
        self.with(|m| m.room(t60, damp));
    }
    /// Once a frame: the game is still there, at this volume.
    pub fn frame(&mut self, volume: f32) {
        let mut busy = false;
        self.with(|m| {
            m.keep();
            m.volume = volume;
            busy = m.busy();
        });
        if busy {
            self.dev.poke();
        }
    }
}
