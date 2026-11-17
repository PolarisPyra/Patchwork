//! Reading images for a model that cannot: the vision model describes them, and the model works
//! from the description.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};
use crate::provider::AgentKind;
use super::{Agent, ImageIn, Vision};

/// What the vision model is asked for. The user's message follows, so it knows what matters.
const DESCRIBE: &str = "Another model, which cannot see images, will answer the user's message below from your description of the attached images. Describe each image fully: transcribe every piece of visible text exactly, including code, settings, field names and values, labels and error messages, and describe the layout and anything else that could matter to the answer. Do not answer the message yourself.";

/// How long a CLI has to describe the images.
const CLI_TIMEOUT: Duration = Duration::from_secs(300);

impl Vision {
    fn model(&self) -> &str {
        match self {
            Vision::Cli { model, .. } | Vision::Endpoint { model, .. } => model,
        }
    }
}

impl Agent {
    /// `text` with the vision model's description of `images` after it, or a note for the
    /// model to pass on when the vision model could not describe them.
    pub(super) fn described(&self, vision: &Vision, text: &str, images: &[ImageIn]) -> String {
        let what = if images.len() == 1 {
            "the image"
        } else {
            "the images"
        };
        let model = vision.model();
        match self.describe(vision, text, images) {
            Ok(description) => format!(
                "{text}\n\n(You cannot see images, so the vision model {model} describes {what} here:)\n\n{description}"
            ),
            Err(e) => format!(
                "{text}\n\n(You cannot see images, and the vision model {model} could not describe {what}: {e}. Tell the user.)"
            ),
        }
    }

    /// Asks the vision model to describe `images`, with the user's message beside them.
    fn describe(&self, vision: &Vision, text: &str, images: &[ImageIn]) -> Result<String, Error> {
        let (base_url, api_key, headers, model, provider) = match vision {
            Vision::Cli { agent, model, path } => {
                return describe_with_cli(*agent, model, path.as_deref(), text, images);
            }
            Vision::Endpoint {
                base_url,
                api_key,
                headers,
                model,
                provider,
            } => (base_url, api_key, headers, model, provider),
        };
        let mut content = vec![
            json!({"type": "text", "text": format!("{DESCRIBE}\n\nThe user's message:\n{text}")}),
        ];
        content.extend(images.iter().map(|i| {
            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", i.media_type, i.data)}})
        }));
        let body = json!({
            "model": model,
            "messages": [{"role": "user", "content": content}],
            "stream": false,
        });
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(key) = api_key {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let mut response = request
            .send(body.to_string())
            .map_err(|e| format!("could not reach {provider}: {e}"))?;
        let status = response.status().as_u16();
        let raw = response.body_mut().read_to_string().unwrap_or_default();
        let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        if !response.status().is_success() {
            return Err(crate::harness::error_text(status, &parsed, &raw).into());
        }
        let reply = parsed["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        // A reasoning model served without a reasoning parser thinks inside its reply.
        let reply = reply
            .rsplit_once("</think>")
            .map_or(reply, |(_, after)| after)
            .trim();
        if reply.is_empty() {
            return Err(format!("{provider} sent back no description").into());
        }
        Ok(reply.to_string())
    }
}

/// Has the CLI `agent` describe `images` with `model`. The images are saved to a folder of their
/// own, which the CLI runs in, so it reads no project instructions.
fn describe_with_cli(
    agent: AgentKind,
    model: &str,
    path: Option<&str>,
    text: &str,
    images: &[ImageIn],
) -> Result<String, Error> {
    if let Some(path) = path {
        crate::provider::set_agent_paths(&HashMap::from([(agent, path.to_string())]));
    }
    let dir = std::env::temp_dir().join(format!("patchwork-vision-{}", crate::system::new_id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not save the images: {e}"))?;
    let described =
        save_images(&dir, images).and_then(|files| run_cli(agent, model, &dir, text, &files));
    let _ = std::fs::remove_dir_all(&dir);
    described
}

/// Writes each image to `dir` as `image-<n>.<type>`, and returns their file names.
fn save_images(dir: &Path, images: &[ImageIn]) -> Result<Vec<String>, Error> {
    images
        .iter()
        .enumerate()
        .map(|(n, image)| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .map_err(|e| format!("an image could not be read: {e}"))?;
            let extension = image
                .media_type
                .strip_prefix("image/")
                .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("png");
            let name = format!("image-{}.{extension}", n + 1);
            std::fs::write(dir.join(&name), bytes)
                .map_err(|e| format!("could not save the images: {e}"))?;
            Ok(name)
        })
        .collect()
}

/// Claude Code reads the images with its Read tool; Codex gets them attached.
fn run_cli(
    agent: AgentKind,
    model: &str,
    dir: &Path,
    text: &str,
    files: &[String],
) -> Result<String, Error> {
    let message = format!("The user's message:\n{text}");
    match agent {
        AgentKind::Claude => {
            let prompt = format!(
                "{DESCRIBE}\n\nThe images are the files {} in the current folder. Read each one with the Read tool.\n\n{message}",
                files.join(", ")
            );
            let args = [
                "-p",
                &prompt,
                "--output-format",
                "json",
                "--model",
                model,
                "--tools",
                "Read",
                "--disable-slash-commands",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--settings",
                r#"{"disableAllHooks":true}"#,
                "--permission-mode",
                "dontAsk",
            ];
            let out = crate::provider::probe_in(agent, &args, dir, CLI_TIMEOUT)
                .ok_or("Claude Code did not start, or took longer than 5 minutes")?;
            let value: Value = serde_json::from_str(&out.stdout).unwrap_or(Value::Null);
            let reply = value["result"].as_str().unwrap_or_default().trim();
            if !out.success || value["is_error"] == true {
                return Err(failure("Claude Code", reply, &out.stderr).into());
            }
            described(reply, "Claude Code")
        }
        AgentKind::Codex => {
            let answer: PathBuf = dir.join("answer.txt");
            let answer_arg = answer.to_string_lossy();
            let images = files.join(",");
            let prompt = format!("{DESCRIBE}\n\n{message}");
            let args = [
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "-m",
                model,
                "--image",
                &images,
                "--output-last-message",
                &answer_arg,
                "--",
                &prompt,
            ];
            let out = crate::provider::probe_in(agent, &args, dir, CLI_TIMEOUT)
                .ok_or("Codex did not start, or took longer than 5 minutes")?;
            if !out.success {
                return Err(failure("Codex", "", &out.stderr).into());
            }
            described(
                std::fs::read_to_string(&answer).unwrap_or_default().trim(),
                "Codex",
            )
        }
        _ => Err("only an API provider, Claude Code or Codex can read images".into()),
    }
}

fn described(reply: &str, cli: &str) -> Result<String, Error> {
    if reply.is_empty() {
        return Err(format!("{cli} sent back no description").into());
    }
    Ok(reply.to_string())
}

/// What a CLI that failed said: its reply, else the last line it wrote to stderr.
fn failure(cli: &str, reply: &str, stderr: &str) -> String {
    let said = if reply.is_empty() {
        stderr.trim().lines().last().unwrap_or_default()
    } else {
        reply
    };
    if said.is_empty() {
        format!("{cli} failed")
    } else {
        format!("{cli} failed: {said}")
    }
}
