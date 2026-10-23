// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // An OpenCode turn starts this binary as its bridge to `opencode serve`, without the app.
    if std::env::args().nth(1).as_deref() == Some(patchwork_server::provider::opencode_bridge::BRIDGE_ARG) {
        std::process::exit(patchwork_server::provider::opencode_bridge::run());
    }
    // WebKitGTK's DMA-BUF renderer crashes on Wayland with some GPUs (notably
    // NVIDIA: "Error 71 (Protocol error) dispatching to Wayland display").
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: runs first in main, before any other threads exist.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    // GTK sets the Wayland app id from the program name, and KWin uses it to find the desktop
    // entry for the title bar icon. The local build's entry is dev.patchwork.ade.desktop, so it
    // gets that id. The AppImage keeps the default id, which its own launcher matches. The AppImage
    // runtime sets APPIMAGE.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        glib::set_prgname(Some("dev.patchwork.ade"));
    }
    patchwork_lib::run();
}
