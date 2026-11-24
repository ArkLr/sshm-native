//! SSHM — native, GPU-rendered SSH manager.
//!
//! Stack:
//!   * eframe/egui on wgpu (Vulkan / Metal / DX12) for the window and UI
//!   * alacritty_terminal for VT parsing, the cell grid, scrollback and the PTY
//!   * the system `ssh` binary running inside that PTY (swap for `russh` later)

mod app;
mod config;
mod terminal;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SSHM")
            .with_app_id("sshm")
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([640.0, 400.0]),
        // Keep Ctrl+C/X/V as egui clipboard events so text fields in dialogs
        // work. The terminal view converts them back to ^C / ^X / paste.
        clipboard_shortcuts: true,
        ..Default::default()
    };

    eframe::run_native(
        "SSHM",
        options,
        Box::new(|cc| Ok(Box::new(app::SshmApp::new(cc)))),
    )
}
