//! The system prompt the agent runs with: Patchwork's guidance, the computer-use part, the project's instructions, and the provider's own prompt from Settings.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::path::Path;
use super::Request;

pub(super) fn system_prompt(request: &Request, root_dir: &Path) -> String {
    let root = &request.root;
    let git = root_dir.join(".git").exists();
    let today = patchwork_platform::time::format_rfc3339_ms(patchwork_platform::now_ms())
        .unwrap_or_default();
    let today = today.get(..10).unwrap_or(&today);
    if request.dispatcher {
        let instructions = request.instructions.as_deref().unwrap_or_default();
        return format!("{instructions}\n\nToday is {today}.");
    }
    let mut prompt = format!(
        "You are a coding agent working in Patchwork, a desktop workspace, on the user's own machine.\n\
         You are working in {root}{}. Today is {today}. The platform is {}.\n\n{GUIDANCE}",
        if git { ", a git repository" } else { "" },
        std::env::consts::OS,
    );
    if let Some(instructions) = &request.instructions {
        let _ = write!(prompt, "\n\n{instructions}");
    }
    match (crate::computer::seeing(), crate::computer::enabled()) {
        (true, true) => {
            let _ = write!(prompt, "\n\n{SEE_SCREEN}\n{COMPUTER_USE}");
        }
        (true, false) => {
            let _ = write!(prompt, "\n\n{SEE_SCREEN}");
        }
        (false, true) => {
            let _ = write!(prompt, "\n\n{COMPUTER_USE_BLIND}");
        }
        (false, false) => {}
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if let Ok(text) = std::fs::read_to_string(root_dir.join(name)) {
            let _ = write!(
                prompt,
                "\n\nThe project's instructions, from {name}:\n\n{text}"
            );
            break;
        }
    }
    // The user's own prompt comes last, as it does for the CLIs, so a project's file does not
    // override it.
    let custom = request.system_prompt.trim();
    if !custom.is_empty() {
        let _ = write!(prompt, "\n\n{custom}");
    }
    prompt
}

const GUIDANCE: &str = "\
Help with software engineering tasks: fixing bugs, adding features, refactoring, explaining code.
- Use the tools to look before you act: read files before you edit them, and search the code instead of guessing.
- Make the smallest change that does what was asked, in the style of the code around it.
- Prefer edit_file for changes to existing files; write_file is for new files or full rewrites.
- Run the project's own build, lint, or test commands with bash when they help you check your work.
- For a task of several steps, keep a checklist with update_plan: list every step before you start, then call it again with the whole list each time a step starts or finishes.
- Ask with ask_user only when a decision is genuinely the user's; otherwise make the sensible choice.
- When a tool is denied, do not retry it; find another way or ask.
- Be brief. Write in Markdown, and say what you changed and anything left undone when you finish.";

/// What the agent is told while it may see the screen.
const SEE_SCREEN: &str = "\
You may see the screen. computer_screenshot writes a PNG of the screen and returns its path; read \
it with read_file to see the screen. The picture is the screen at its own size. Only on a scaled \
display is its width not screenWidth; then multiply a position by screenWidth divided by the \
picture's width.";

/// What the agent is told while it may drive the pointer and keyboard.
const COMPUTER_USE: &str = "\
Computer use is on: you can move, click, type, press keys and scroll. Click where you see \
something in a screenshot. Take a screenshot before acting, and again after, because the screen \
only changes when you act. Prefer a tool over the screen whenever one exists. Stop and report \
when a step needs the user's password.";

/// What the agent is told while it may drive the screen but not see it.
const COMPUTER_USE_BLIND: &str = "\
Computer use is on: you can move, click, type, press keys and scroll. But you cannot see the \
screen, since the user has not turned on \u{201c}Let the agent see the screen\u{201d} in \
Settings → Permissions. Do not act on the screen without seeing it; ask the user to turn it on \
when a task needs the screen.";
