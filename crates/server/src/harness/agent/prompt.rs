//! The system prompt the agent runs with: Patchwork's guidance, the plan-mode and computer-use parts, the project's instructions, and the provider's own prompt from Settings.

use std::path::Path;
use super::Request;

/// Longest project instructions file read into the system prompt.
const MAX_INSTRUCTIONS: usize = 20_000;

pub(super) fn system_prompt(request: &Request, root_dir: &Path) -> String {
    let root = &request.root;
    let git = root_dir.join(".git").exists();
    let today = crate::system::time::format_rfc3339_ms(crate::system::now_ms());
    let today = today.get(..10).unwrap_or(&today);
    let mut prompt = format!(
        "You are a coding agent working in Patchwork, a desktop workspace, on the user's own machine.\n\
         You are working in {root}{}. Today is {today}. The platform is {}.\n\n{GUIDANCE}",
        if git { ", a git repository" } else { "" },
        std::env::consts::OS,
    );
    if let Some(instructions) = &request.instructions {
        prompt.push_str(&format!("\n\n{instructions}"));
    }
    if request.plan {
        prompt.push_str(&format!("\n\n{PLAN_MODE}"));
    } else if crate::computer::enabled() {
        prompt.push_str(&format!("\n\n{COMPUTER_USE}"));
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if let Ok(text) = std::fs::read_to_string(root_dir.join(name)) {
            let text: String = text.chars().take(MAX_INSTRUCTIONS).collect();
            prompt.push_str(&format!(
                "\n\nThe project's instructions, from {name}:\n\n{text}"
            ));
            break;
        }
    }
    // The user's own prompt comes last, as it does for the CLIs, so a project's file does not
    // override it.
    let custom = request.system_prompt.trim();
    if !custom.is_empty() {
        prompt.push_str(&format!("\n\n{custom}"));
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

const COMPUTER_USE: &str = "\
Computer use is on. computer_screenshot writes a PNG of the screen and returns its path; read it \
with read_file to see the screen. The picture is the screen at its own size, so click where you see \
something. Only on a scaled display is its width not screenWidth; then multiply a position by \
screenWidth divided by the picture's width. Take a screenshot \
before acting, and again after, because the screen only changes when you act. Prefer a tool over \
the screen whenever one exists. Stop and report when a step needs the user's password.";

const PLAN_MODE: &str = "\
Plan mode is on: do not change anything. Research with the read-only tools, then call propose_plan \
once with a concrete, step-by-step implementation plan in Markdown, and stop.";
