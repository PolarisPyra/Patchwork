// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // An OpenCode turn starts this binary as its bridge to `opencode serve`, without the app.
    if std::env::args().nth(1).as_deref()
        == Some(patchwork_server::provider::opencode::bridge::BRIDGE_ARG)
    {
        std::process::exit(patchwork_server::provider::opencode::bridge::run());
    }
    // An API provider's turn starts this binary as Patchwork's own agent, without the app.
    if std::env::args().nth(1).as_deref()
        == Some(patchwork_server::provider::api::bridge::BRIDGE_ARG)
    {
        std::process::exit(patchwork_server::provider::api::bridge::run());
    }
    // Codex and Antigravity start this binary as a hook that checks a tool call against the
    // permission rules.
    if std::env::args().nth(1).as_deref() == Some(patchwork_server::permissions::hook::HOOK_ARG) {
        std::process::exit(patchwork_server::permissions::hook::run());
    }
    // Antigravity starts this binary at session start for the system prompt from Settings.
    if std::env::args().nth(1).as_deref() == Some(patchwork_server::provider::hooks::PROMPT_ARG) {
        std::process::exit(patchwork_server::provider::hooks::run_prompt_hook());
    }
    // An agent starts this binary as a plugin's built-in hook, such as `boundedWaits`.
    if std::env::args().nth(1).as_deref() == Some(patchwork_server::plugins::builtin::HOOK_ARG) {
        std::process::exit(patchwork_server::plugins::builtin::run());
    }
    // Claude Code and Codex start this binary as a relay to an MCP server Patchwork runs.
    if std::env::args().nth(1).as_deref() == Some(patchwork_server::provider::mcp::PROXY_ARG) {
        std::process::exit(patchwork_server::provider::mcp::run_proxy());
    }
    // WebKitGTK's DMA-BUF renderer stays on: without it every animated frame is copied whole
    // from the web process, so a single spinner cost about a fifth of a core. On older NVIDIA
    // drivers under Wayland it can crash ("Error 71 (Protocol error) dispatching to Wayland
    // display"); WebKit itself reads WEBKIT_DISABLE_DMABUF_RENDERER=1 to turn it off there.
    // GTK sets the Wayland app id from the program name, and KWin uses it to find the desktop
    // entry for the title bar icon. `scripts/build.sh install` installs dev.patchwork.ade.desktop,
    // and the dev session shares it, so they get that id. An AppImage run without that entry
    // keeps the default id, which its own launcher matches. The AppImage runtime sets APPIMAGE.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() || installed_launcher() {
        glib::set_prgname(Some("dev.patchwork.ade"));
    }
    patchwork_lib::run();
}

/// Whether `scripts/build.sh install` put Patchwork's launcher in the user's applications.
#[cfg(target_os = "linux")]
fn installed_launcher() -> bool {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".local/share"))
        });
    data_home.is_some_and(|dir| dir.join("applications/dev.patchwork.ade.desktop").is_file())
}
