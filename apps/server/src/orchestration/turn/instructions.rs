//! What an agent is told when it starts: to keep its checklist with its own tool, which shell
//! runs its commands, and the system prompt the user gave it.

use crate::provider::AgentKind;
use crate::settings::Settings;

/// What an agent that takes instructions at launch is told: to keep its task list with its own
/// tool, so the Tasks badge fills in, then which shells are installed and which one runs its
/// commands, then the user's own system prompt for it.
pub(crate) fn agent_instructions(settings: &Settings, agent: AgentKind) -> String {
    [
        checklist_instructions(agent),
        Some(shell_instructions(settings, agent)),
        system_prompt(settings, agent).map(str::to_string),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// The system prompt the user gave `agent` in Settings, if any.
pub(crate) fn system_prompt(settings: &Settings, agent: AgentKind) -> Option<&str> {
    settings
        .system_prompts
        .get(&agent)
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
}

/// The line that tells `agent` to keep a checklist with its own tool. Antigravity has no such
/// tool, and Patchwork's own agent is told in its own guidance.
fn checklist_instructions(agent: AgentKind) -> Option<String> {
    let tool = match agent {
        AgentKind::Claude => "TodoWrite",
        AgentKind::Codex => "update_plan",
        AgentKind::OpenCode => "todowrite",
        AgentKind::Antigravity | AgentKind::Api => return None,
    };
    Some(format!(
        "For any task with more than one step, keep a checklist with your {tool} tool: add every \
         step before you start, mark one in progress at a time, and mark each done as soon as it \
         is finished."
    ))
}

/// Which shells are installed and which one runs `agent`'s commands, which is the one its CLI
/// picks rather than always the one Settings chose.
fn shell_instructions(settings: &Settings, agent: AgentKind) -> String {
    let mut prompt = String::new();
    let installed = crate::system::available_shells();
    if !installed.is_empty() {
        prompt.push_str(&format!(
            "Shells on this machine: {}.",
            installed.join(", ")
        ));
    }
    let chosen = settings.shell.as_deref();
    if let Some(shell) = crate::provider::agent_shell(agent, chosen) {
        if chosen == Some(shell.as_str()) {
            prompt.push_str(&format!(
                " The user chose {shell} for shell commands; run them in it and write them for it."
            ));
        } else {
            prompt.push_str(&format!(
                " Shell commands run in {shell}; write them for it."
            ));
        }
        if crate::system::shell_name(&shell) == "zsh" {
            prompt.push_str(ZSH_NOTES);
        }
    }
    prompt.trim_start().to_string()
}

/// Where zsh reads a command written for bash differently, each of which has failed agents'
/// commands outright.
const ZSH_NOTES: &str = " Unlike bash, zsh refuses to run a command with a glob that matches \
     nothing, so quote patterns meant for the program (`--include='*.rs'`, `find -name '*.ts'`); \
     reads a word starting with `=` as a program's path and stops the whole command line when \
     there is none, so quote it (`echo '===='`); and does not split an unquoted `$var` into words.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_agent_is_told_to_keep_a_checklist_with_its_own_tool() {
        let settings = Settings::default();
        for (agent, tool) in [
            (AgentKind::Claude, "TodoWrite"),
            (AgentKind::Codex, "update_plan"),
            (AgentKind::OpenCode, "todowrite"),
        ] {
            let text = agent_instructions(&settings, agent);
            assert!(
                text.contains(&format!("your {tool} tool")),
                "{agent:?}: {text}"
            );
        }
        for agent in [AgentKind::Antigravity, AgentKind::Api] {
            assert_eq!(checklist_instructions(agent), None, "{agent:?}");
            let text = agent_instructions(&settings, agent);
            assert!(!text.contains("checklist"), "{agent:?}: {text}");
        }
    }

    #[test]
    fn the_instructions_carry_the_checklist_and_the_shell_line() {
        let shell = shell_instructions(&Settings::default(), AgentKind::Claude);
        let text = agent_instructions(&Settings::default(), AgentKind::Claude);
        assert!(text.starts_with("For any task with more than one step"));
        assert!(text.ends_with(&shell), "{text}");
        assert!(!text.contains("\n\n\n"), "no double blank line: {text:?}");
        assert!(!text.ends_with('\n'));
    }
}
