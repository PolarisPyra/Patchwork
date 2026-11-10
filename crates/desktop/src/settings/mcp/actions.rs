//! Reloading the servers, running a server action, and opening a client's config.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::Context;
use patchwork_server::provider::mcp::clis;
use patchwork_server::provider::mcp;
use crate::settings::SettingsDialog;

impl SettingsDialog {
    /// Reads Patchwork's servers and the CLIs' again, and follows them each second while one
    /// starts or lists its tools.
    pub(super) fn reload_mcp(&mut self, cx: &mut Context<Self>) {
        let projects = self.settings.projects.clone();
        self.run(
            move || (mcp::list(), clis::list(&projects), mcp::tool_budget()),
            |d, (own, cli, budget), cx| {
                let busy = own.iter().any(|s| s.status == "starting")
                    || cli
                        .iter()
                        .any(|g| g.servers.iter().any(|s| s.status == "listing"));
                d.mcp.servers = Some(own);
                d.mcp.clis = cli;
                d.mcp.budget = Some(budget);
                if busy && !d.mcp.polling {
                    d.mcp.polling = true;
                    cx.spawn(async move |this, cx| {
                        // Once a second, while a server starts or a CLI lists its tools.
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        let _ = this.update(cx, |d, cx| {
                            d.mcp.polling = false;
                            d.reload_mcp(cx);
                        });
                    })
                    .detach();
                }
            },
            cx,
        );
    }

    /// Runs an MCP change off the main thread, shows its error, and reads the servers again.
    pub(super) fn mcp_run(
        &mut self,
        work: impl FnOnce() -> Result<(), patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.mcp.error = None;
        self.run(
            work,
            |d, out, cx| {
                d.mcp.error = out.err().map(|e| e.to_string());
                d.reload_mcp(cx);
            },
            cx,
        );
    }

    /// Opens a server's config file in the editor.
    pub(super) fn open_mcp_config(&mut self, path: String, cx: &mut Context<Self>) {
        let projects = self.settings.projects.clone();
        self.mcp_run(
            move || {
                let path = std::path::PathBuf::from(path);
                if !clis::is_config_file(&path, &projects) {
                    return Err("That is not an MCP config file".into());
                }
                let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err("That is not an MCP config file".into());
                };
                patchwork_server::workspace::open_in_editor(
                    &dir.to_string_lossy(),
                    &name.to_string_lossy(),
                    None,
                )
            },
            cx,
        );
    }
}
