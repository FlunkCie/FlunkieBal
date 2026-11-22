//! What only Android can do, by asking `MainActivity` over JNI: speech, vibration, the notch, the
//! text prompt, the clipboard, leaving the app. Plus where the app's files live.

use android_activity::AndroidApp;
use jni::objects::{JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct Ctx {
    vm: JavaVM,
    activity: jni::objects::GlobalRef,
    dir: Option<PathBuf>,
}
static CTX: OnceLock<Ctx> = OnceLock::new();
/// The text field waiting for the prompt's answer.
static ASKED: Mutex<Option<String>> = Mutex::new(None);

pub fn init(app: &AndroidApp) {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) }.expect("java vm");
    let activity = {
        let mut env = vm.attach_current_thread().expect("attach");
        let a = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };
        env.new_global_ref(a).expect("activity")
    };
    let _ = CTX.set(Ctx { vm, activity, dir: app.internal_data_path() });
}

/// Run something against the activity. A Java exception is logged and costs that one call, not the game.
fn with<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let c = CTX.get()?;
    let mut env = c.vm.attach_current_thread_permanently().ok()?;
    match f(&mut env, c.activity.as_obj()) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("java call failed: {e}");
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            None
        }
    }
}
fn text(env: &mut JNIEnv, o: JObject) -> jni::errors::Result<Option<String>> {
    if o.is_null() {
        return Ok(None);
    }
    let s = JString::from(o);
    let t: String = env.get_string(&s)?.into();
    Ok(Some(t))
}

pub fn data_dir() -> Option<PathBuf> {
    CTX.get()?.dir.clone()
}

pub fn speak(t: &str, lang: &str, pitch: f32, rate: f32, vol: f32) {
    with(|env, a| {
        let (t, lang) = (JObject::from(env.new_string(t)?), JObject::from(env.new_string(lang)?));
        env.call_method(a, "speak", "(Ljava/lang/String;Ljava/lang/String;FFF)V", &[JValue::Object(&t), JValue::Object(&lang), JValue::Float(pitch), JValue::Float(rate), JValue::Float(vol)])?;
        Ok(())
    });
}
pub fn copy_text(t: &str) {
    with(|env, a| {
        let t = JObject::from(env.new_string(t)?);
        env.call_method(a, "copyText", "(Ljava/lang/String;)V", &[JValue::Object(&t)])?;
        Ok(())
    });
}
pub fn vibrate(ms: u32) {
    with(|env, a| {
        env.call_method(a, "vibrate", "(I)V", &[JValue::Int(ms as i32)])?;
        Ok(())
    });
}
/// Draw at the screen's highest refresh rate, or at 60 Hz (cooler, and plenty for this game).
pub fn max_fps(on: bool) {
    with(|env, a| {
        env.call_method(a, "maxFps", "(Z)V", &[JValue::Bool(on as u8)])?;
        Ok(())
    });
}
/// The display cutout: top, right, bottom, left, in egui points.
pub fn insets() -> [f32; 4] {
    with(|env, a| {
        let o = env.call_method(a, "insets", "()Ljava/lang/String;", &[])?.l()?;
        text(env, o)
    })
    .flatten()
    .map_or([0.0; 4], |s| {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() == 4 {
            [v[0].clamp(0.0, 80.0), v[1].clamp(0.0, 80.0), v[2].clamp(0.0, 60.0), v[3].clamp(0.0, 80.0)]
        } else {
            [0.0; 4]
        }
    })
}
/// Back on the main menu: go to the launcher, as back does in every app.
pub fn leave() {
    with(|env, a| {
        env.call_method(a, "leave", "()V", &[])?;
        Ok(())
    });
}

/// Open the phone's text prompt for this field.
pub fn ask_text(field: &str, current: &str) {
    if let Ok(mut q) = ASKED.lock() {
        *q = Some(field.to_string());
    }
    with(|env, a| {
        let (f, c) = (JObject::from(env.new_string(field)?), JObject::from(env.new_string(current)?));
        env.call_method(a, "askText", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&f), JValue::Object(&c)])?;
        Ok(())
    });
}
/// What was typed in the prompt, if this field asked and the answer has come in.
pub fn text_answer(field: &str) -> Option<String> {
    let mut q = ASKED.lock().ok()?;
    if q.as_deref() != Some(field) {
        return None;
    }
    let got = with(|env, a| {
        let o = env.call_method(a, "pollText", "()Ljava/lang/String;", &[])?.l()?;
        text(env, o)
    })
    .flatten()?;
    *q = None;
    // "=text" for OK, "!" for cancel
    got.strip_prefix('=').map(str::to_string)
}

/// Vulkan if this phone has a working driver for it, GLES otherwise. A driver can take its time
/// waking up, or never come back at all; everything hangs on this thread, and Android calls an app
/// "not responding" after 5 seconds, so it gets a thread of its own and a limit.
pub fn backends() -> egui_wgpu::wgpu::Backends {
    use egui_wgpu::wgpu;
    const PROBE_LIMIT: std::time::Duration = std::time::Duration::from_secs(3);
    let t0 = std::time::Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("vulkan-probe".into()).spawn(move || {
        let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
        d.backends = wgpu::Backends::VULKAN;
        let n = pollster::block_on(wgpu::Instance::new(d).enumerate_adapters(wgpu::Backends::VULKAN)).len();
        let _ = tx.send(n);
    });
    let n = if spawned.is_ok() { rx.recv_timeout(PROBE_LIMIT).ok() } else { None };
    match n {
        Some(n) => log::info!("{n} Vulkan adapter(s), found in {} ms", t0.elapsed().as_millis()),
        None => log::warn!("the Vulkan driver didn't answer within {PROBE_LIMIT:?}; using GLES"),
    }
    if n.unwrap_or(0) > 0 {
        wgpu::Backends::VULKAN
    } else {
        wgpu::Backends::GL
    }
}

// ---------- notifications ----------
/// Who to check notifications for in the background, while the game is closed (`NotesJob` in Java):
/// the server's HTTP address, friend code and secret. Empty, or `on` false: nobody, no checks.
pub fn notes_account(http: &str, code: &str, secret: &str, on: bool) {
    with(|env, a| {
        let (h, c, s) = (JObject::from(env.new_string(http)?), JObject::from(env.new_string(code)?), JObject::from(env.new_string(secret)?));
        env.call_method(a, "notesAccount", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V", &[JValue::Object(&h), JValue::Object(&c), JValue::Object(&s), JValue::Bool(on as u8)])?;
        Ok(())
    });
}
/// Ask for permission to show notifications (Android 13 and up), if it hasn't been given.
pub fn ask_notify() {
    with(|env, a| {
        env.call_method(a, "askNotify", "()V", &[])?;
        Ok(())
    });
}
/// A notification from the server. Shown only while the game isn't on screen, and each one once
/// (the background check knows which it has had).
pub fn notify(n: &crate::net::Note) {
    with(|env, a| {
        let (from, name, text, room) = (JObject::from(env.new_string(&n.from)?), JObject::from(env.new_string(&n.name)?), JObject::from(env.new_string(&n.text)?), JObject::from(env.new_string(&n.room)?));
        env.call_method(a, "note", "(JILjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Long(n.id as i64), JValue::Int(n.kind as i32), JValue::Object(&from), JValue::Object(&name), JValue::Object(&text), JValue::Object(&room)])?;
        Ok(())
    });
}
/// A `Note` message as it arrives on the friends socket's own thread.
pub fn note_json(t: &str) {
    if let Ok(crate::net::FromServer::Note { note }) = serde_json::from_str(t) {
        notify(&note);
    }
}
/// What a tapped notification asks for, once: "room:CODE" (an invite) or "chat:CODE" (a message).
pub fn take_open() -> Option<String> {
    with(|env, a| {
        let o = env.call_method(a, "takeOpen", "()Ljava/lang/String;", &[])?.l()?;
        text(env, o)
    })
    .flatten()
    .filter(|s| !s.is_empty())
}

// ---------- updating ----------
/// Download the newest APK and hand it to the system's installer (it asks the player).
pub fn update(url: &str) {
    with(|env, a| {
        let u = JObject::from(env.new_string(url)?);
        env.call_method(a, "update", "(Ljava/lang/String;)V", &[JValue::Object(&u)])?;
        Ok(())
    });
}
/// How the update is going: "" (not started), "get:<percent>", "install" (the installer is up), "err:<why>".
pub fn update_state() -> String {
    with(|env, a| {
        let o = env.call_method(a, "updateState", "()Ljava/lang/String;", &[])?.l()?;
        text(env, o)
    })
    .flatten()
    .unwrap_or_default()
}
