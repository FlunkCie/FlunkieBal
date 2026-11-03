//! FlunkieBal: the Dutch park drinking game, in Rust with wgpu.
pub use flunkiebal_core::{anim, game, icons};
pub mod app;
pub mod arenas;
pub mod audio;
pub mod chalk;
pub mod gfx;
pub mod model;
pub mod net;
pub mod voice;
pub mod save;
pub mod scene;
pub mod social;
pub mod ui;
#[cfg(target_os = "android")]
pub mod android;

/// The Android app starts here (see `MainActivity`).
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("flunkiebal"));
    std::panic::set_hook(Box::new(|info| log::error!("{info}")));
    android::init(&app);
    let mut opts = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    // one backend only: wgpu would otherwise attach a Vulkan surface to the window, after which GLES can't
    if let egui_wgpu::WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
        c.instance_descriptor.backends = android::backends();
    }
    if let Err(e) = eframe::run_native("FlunkieBal", opts, Box::new(|cc| Ok(Box::new(app::FlunkiApp::new(cc))))) {
        log::error!("the game couldn't start: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    /// The game stopped: say so on screen and offer a reload instead of a frozen picture.
    fn crashed(msg: &str) {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
        if let Some(el) = doc.get_element_by_id("boot") {
            let _ = el.set_attribute("class", "crashed");
        }
        if let Some(el) = doc.get_element_by_id("boot-msg") {
            let short: String = msg.chars().take(300).collect();
            el.set_text_content(Some(&format!("Something went wrong and the game stopped. Tap to restart.\n\n{short}")));
        }
    }
    fn set_status(msg: &str) {
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("boot-msg")) {
            el.set_text_content(Some(msg));
        }
    }

    /// Test hook: queue a command for the app (see `FlunkiApp::debug_cmds`).
    #[wasm_bindgen]
    pub fn flunki_cmd(c: &str) {
        crate::app::CMDS.with(|q| q.borrow_mut().push(c.to_string()));
    }

    /// Android back button: true when the game handled it, false on the main menu.
    #[wasm_bindgen]
    pub fn flunki_back() -> bool {
        if crate::app::SCREEN.with(|c| c.get()) == 0 {
            return false;
        }
        flunki_cmd("back");
        true
    }

    /// True when (x, y) in CSS pixels is on a text field: the page then focuses the keyboard input
    /// during the tap itself, which phones require before they show the keyboard.
    #[wasm_bindgen]
    pub fn flunki_text_hit(x: f32, y: f32) -> bool {
        crate::ui::TEXT_RECTS.with(|t| t.borrow().iter().any(|r| r.expand(6.0).contains(egui::pos2(x, y))))
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            prev(info);
            crashed(&info.to_string());
        }));
        wasm_bindgen_futures::spawn_local(async {
            let doc = web_sys::window().unwrap().document().unwrap();
            let canvas = doc.get_element_by_id("game").unwrap().dyn_into::<web_sys::HtmlCanvasElement>().unwrap();
            let mut opts = eframe::WebOptions::default();
            // Firefox's WebGPU is still young; its WebGL is solid, and it is what the game is tested on
            let ua = web_sys::window().and_then(|w| w.navigator().user_agent().ok()).unwrap_or_default();
            if ua.contains("Firefox") {
                if let egui_wgpu::WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
                    c.instance_descriptor.backends = egui_wgpu::wgpu::Backends::GL;
                }
            }
            let res = eframe::WebRunner::new().start(canvas, opts, Box::new(|cc| Ok(Box::new(crate::app::FlunkiApp::new(cc))))).await;
            match res {
                Ok(()) => {
                    if let Some(el) = doc.get_element_by_id("boot") {
                        let _ = el.set_attribute("class", "done");
                    }
                }
                Err(e) => set_status(&format!("Could not start the 3D view: {e:?}")),
            }
        });
    }
}
