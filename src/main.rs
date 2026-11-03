#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    flunkiebal::app::stdin_cmds();
    let opts = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 860.0]).with_title("FlunkieBal"), ..Default::default() };
    eframe::run_native("FlunkieBal", opts, Box::new(|cc| Ok(Box::new(flunkiebal::app::FlunkiApp::new(cc)))))
}
#[cfg(target_arch = "wasm32")]
fn main() {}
