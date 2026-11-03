//! Spoken quick messages: the phone's own text-to-speech on Android (see `android.rs`), the browser's
//! speechSynthesis on the web. Silent on desktop builds.

/// What each quick message says out loud: (text, language).
pub const QM: [&str; 9] = ["Proost! \u{eb62}", "Nice throw!", "Drink up!", "Ref, are you blind?", "Oops…", "Hurry up!", crate::icons::LAUGH, crate::icons::CLAP, "GG"];
pub const QM_SAY: [(&str, &str); 9] = [("Proost!", "nl-NL"), ("Nice throw!", "en-GB"), ("Drink up!", "en-GB"), ("Ref! Are you blind?", "en-GB"), ("Oops!", "en-GB"), ("Hurry up!", "en-GB"), ("Ha ha ha ha!", "en-GB"), ("Bravo! Bravo!", "en-GB"), ("Good game!", "en-GB")];

#[cfg(target_arch = "wasm32")]
pub fn speak(text: &str, lang: &str, pitch: f32, rate: f32, vol: f32) {
    use wasm_bindgen::JsCast;
    let Some(win) = web_sys::window() else { return };
    let Ok(synth) = win.speech_synthesis() else { return };
    let Ok(u) = web_sys::SpeechSynthesisUtterance::new_with_text(text) else { return };
    u.set_lang(lang);
    u.set_pitch(pitch.clamp(0.1, 2.0));
    u.set_rate(rate.clamp(0.5, 2.0));
    u.set_volume(vol.clamp(0.0, 1.0));
    // prefer a real voice in that language
    let voices = synth.get_voices();
    let pre = &lang[..2];
    for v in voices.iter() {
        if let Ok(v) = v.dyn_into::<web_sys::SpeechSynthesisVoice>() {
            if v.lang().starts_with(pre) {
                u.set_voice(Some(&v));
                break;
            }
        }
    }
    synth.speak(&u);
}

#[cfg(target_os = "android")]
pub use crate::android::{speak, vibrate};
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn speak(_text: &str, _lang: &str, _pitch: f32, _rate: f32, _vol: f32) {}

#[cfg(target_arch = "wasm32")]
pub fn vibrate(ms: u32) {
    // Firefox on desktop has no navigator.vibrate: calling it directly throws straight through the
    // game loop, so look it up and call it as an ordinary JS function that may fail
    use wasm_bindgen::JsCast;
    let Some(w) = web_sys::window() else { return };
    let nav = w.navigator();
    if let Ok(f) = js_sys::Reflect::get(&nav, &"vibrate".into()) {
        if let Some(f) = f.dyn_ref::<js_sys::Function>() {
            let _ = f.call1(&nav, &wasm_bindgen::JsValue::from_f64(ms as f64));
        }
    }
}
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn vibrate(_ms: u32) {}
