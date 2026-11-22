#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    flunkiebal::app::stdin_cmds();
    let mut opts = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 860.0]).with_title("FlunkieBal"), ..Default::default() };
    // FLUNKI_GPU_TIME: no vsync, and the GPU time of the shadow and scene passes in the log
    if std::env::var_os("FLUNKI_GPU_TIME").is_some() {
        opts.wgpu_options.surface.present_mode = egui_wgpu::wgpu::PresentMode::AutoNoVsync;
        if let egui_wgpu::WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
            let base = c.device_descriptor.clone();
            c.device_descriptor = std::sync::Arc::new(move |a| {
                let mut d = base(a);
                d.required_features |= a.features() & egui_wgpu::wgpu::Features::TIMESTAMP_QUERY;
                d
            });
        }
    }
    eframe::run_native("FlunkieBal", opts, Box::new(|cc| Ok(Box::new(flunkiebal::app::FlunkiApp::new(cc)))))
}
#[cfg(target_arch = "wasm32")]
fn main() {}
