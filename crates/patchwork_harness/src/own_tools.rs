//! The tools Patchwork serves to every agent that its own agent offers by name.

// Rust guideline compliant 2026-11-27

/// Asking the user for a secret in a private card.
pub mod secret {
    use serde_json::{Value, json};

    /// The tool every agent asks for a secret with.
    pub const TOOL: &str = "request_secret";

    /// What the agent is told the tool does.
    pub const DESCRIPTION: &str = "Ask the user to enter a secret, such as an API key, token or password, in a private card in the chat, and wait for the answer. You never see the value. Once saved, the result names an owner-only file holding it: read it inside the command that needs it, such as TOKEN=\"$(cat <file>)\" some-cli ..., and never print, echo or copy it. The file is deleted after 24 hours. When the user asks how to give you a secret, call this rather than explaining.";

    /// The tool's arguments, as JSON Schema.
    #[must_use]
    pub fn input_schema() -> Value {
        json!({"type": "object", "properties": {
            "label": {"type": "string", "description": "The card's title, such as \"GitHub token\"."},
            "reason": {"type": "string", "description": "What it is for and where to get it."},
            "placeholder": {"type": "string", "description": "Hint shown in the empty field."},
            "timeoutMs": {"type": "integer", "minimum": 1000, "maximum": 3_600_000, "description": "How long to wait for the user. Default 10 minutes."}
        }, "required": ["label", "reason"]})
    }
}

/// Starting a sub agent and collecting its answer.
pub mod sub_agent {
    /// The tool a chat's agent starts a sub agent on another agent with.
    pub const TOOL: &str = "run_sub_agent";

    /// The tool Patchwork's own agent collects the answer of a sub agent it started in the background
    /// with.
    pub const RESULT: &str = "sub_agent_result";
}

/// A quilt being written.
pub mod quilt {
    /// How many more lines are written before a card being written is told again.
    const STEP: u32 = 10;

    /// What a card being written shows of a quilt-tool call whose input is still arriving: its
    /// title, its type, its command, and how many lines it holds so far.
    #[derive(Debug, Clone, Default)]
    pub struct Written {
        pub title: Option<String>,
        pub kind: Option<String>,
        pub command: Option<String>,
        pub lines: u32,
    }

    impl Written {
        /// Reads the input `json` as written so far, and returns whether the card has something new
        /// to show: the title, type or command once each is whole, or another [`STEP`] lines.
        pub fn read(&mut self, json: &str) -> bool {
            let title = string_field(json, "title");
            let kind = string_field(json, "type");
            let command = string_field(json, "command");
            let lines = u32::try_from(json.matches("\\n").count()).unwrap_or(u32::MAX);
            let mut news = false;
            if title.is_some() && self.title.is_none() {
                self.title = title;
                news = true;
            }
            if kind.is_some() && self.kind.is_none() {
                self.kind = kind;
                news = true;
            }
            if command.is_some() && self.command.is_none() {
                self.command = command;
                news = true;
            }
            if lines >= self.lines + STEP {
                self.lines = lines;
                news = true;
            }
            news
        }
    }

    /// The string `key` holds in the partly written JSON object `json`, once all of it is written.
    fn string_field(json: &str, key: &str) -> Option<String> {
        let after = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
        let after = after.trim_start().strip_prefix(':')?.trim_start();
        if !after.starts_with('"') {
            return None;
        }
        let mut escaped = false;
        let end = after.char_indices().skip(1).find_map(|(i, c)| {
            let end = c == '"' && !escaped;
            escaped = c == '\\' && !escaped;
            end.then_some(i)
        })?;
        match serde_json::from_str::<serde_json::Value>(&after[..=end]) {
            Ok(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            _ => None,
        }
    }
}
