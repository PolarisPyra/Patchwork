//! Installing an agent CLI with its official script.

use std::path::PathBuf;

use crate::provider::AgentKind;
use crate::Server;

impl Server {
    /// Downloads `kind`'s official install script to a file, then runs it so it asks nothing. The
    /// script is saved first, so the file that runs is the one that was fetched. Each CLI goes
    /// where its installer puts it by default, which Patchwork already searches. Codex and
    /// OpenCode are kept from editing the shell profile; Antigravity's script always adds its
    /// folder to it, with no way to opt out. On Windows only the Antigravity CLI is set up, with
    /// its PowerShell script.
    pub fn install_cli(&self, kind: AgentKind) -> Result<(), String> {
        let windows = cfg!(windows);
        if windows && kind != AgentKind::Antigravity {
            return Err(
                "Setting up this CLI from Patchwork is for macOS and Linux for now.".into(),
            );
        }
        let home = crate::system::fs::home_dir().ok_or("Could not find the home folder")?;
        let (url, shell, name, bin) = match kind {
            AgentKind::Claude => (
                "https://claude.ai/install.sh",
                "bash",
                "claude",
                home.join(".local/bin/claude"),
            ),
            AgentKind::Codex => (
                "https://chatgpt.com/codex/install.sh",
                "sh",
                "codex",
                home.join(".local/bin/codex"),
            ),
            AgentKind::Antigravity if windows => (
                "https://antigravity.google/cli/install.ps1",
                "powershell",
                "antigravity",
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(r"agy\bin\agy.exe"),
            ),
            AgentKind::Antigravity => (
                "https://antigravity.google/cli/install.sh",
                "bash",
                "antigravity",
                home.join(".local/bin/agy"),
            ),
            AgentKind::OpenCode => (
                "https://opencode.ai/install",
                "bash",
                "opencode",
                home.join(".opencode/bin/opencode"),
            ),
            AgentKind::Api => return Err("API providers have no CLI to install".into()),
        };
        std::fs::create_dir_all(&self.installers_dir).map_err(|e| e.to_string())?;
        let ext = if windows { "ps1" } else { "sh" };
        let script = self.installers_dir.join(format!("install-{name}.{ext}"));
        let fetch = crate::system::host_command("curl")
            .args(["-fsSL", url, "-o"])
            .arg(&script)
            .output()
            .map_err(|e| format!("Could not run curl: {e}"))?;
        if !fetch.status.success() {
            return Err(format!(
                "Download failed: {}",
                String::from_utf8_lossy(&fetch.stderr).trim()
            ));
        }
        let mut cmd = crate::system::host_command(shell);
        if windows {
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
        }
        cmd.arg(&script);
        match kind {
            AgentKind::Codex => {
                // The installer adds its folder to the shell profile unless it is on PATH already.
                let mut paths = vec![home.join(".local/bin")];
                if let Some(existing) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&existing));
                }
                if let Ok(joined) = std::env::join_paths(paths) {
                    cmd.env("PATH", joined);
                }
                cmd.env("CODEX_NON_INTERACTIVE", "1");
            }
            AgentKind::OpenCode => {
                cmd.arg("--no-modify-path");
            }
            AgentKind::Claude | AgentKind::Antigravity | AgentKind::Api => {}
        }
        let run = cmd
            .output()
            .map_err(|e| format!("Could not run the installer: {e}"))?;
        let _ = std::fs::remove_file(&script);
        if !run.status.success() || !bin.is_file() {
            return Err(format!(
                "The install did not finish: {}",
                String::from_utf8_lossy(&run.stderr).trim()
            ));
        }
        Ok(())
    }
}
