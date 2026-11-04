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
        Some(shell_instructions(agent)),
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

/// The line that tells `agent` to keep a checklist with its own tools. Antigravity has no such
/// tool, and Patchwork's own agent is told in its own guidance. Claude Code offers newer models
/// `TaskCreate` and `TaskUpdate` instead of `TodoWrite`, and told of a tool it lacks it goes looking
/// for it before every list.
fn checklist_instructions(agent: AgentKind) -> Option<String> {
    let tools = match agent {
        AgentKind::Claude => {
            "TaskCreate and TaskUpdate tools (TodoWrite if you have no TaskCreate)"
        }
        AgentKind::Codex => "update_plan tool",
        AgentKind::OpenCode => "todowrite tool",
        AgentKind::Antigravity | AgentKind::Api => return None,
    };
    Some(format!(
        "For any task with more than one step, keep a checklist with your {tools}: add every step \
         before you start, mark each one in progress before you work on it, and mark it done as \
         soon as it is finished."
    ))
}

/// Which shells are installed and which one runs `agent`'s commands, which is the one its CLI
/// picks.
fn shell_instructions(agent: AgentKind) -> String {
    let mut prompt = String::new();
    let installed = crate::system::available_shells();
    if !installed.is_empty() {
        prompt.push_str(&format!(
            "Shells on this machine: {}.",
            installed.join(", ")
        ));
    }
    if let Some(shell) = crate::provider::agent_shell(agent) {
        prompt.push_str(&format!(
            " Shell commands run in {shell}; write them for it."
        ));
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
        for (agent, tools) in [
            (AgentKind::Claude, "TaskCreate and TaskUpdate tools"),
            (AgentKind::Codex, "update_plan tool"),
            (AgentKind::OpenCode, "todowrite tool"),
        ] {
            let text = agent_instructions(&settings, agent);
            assert!(text.contains(&format!("your {tools}")), "{agent:?}: {text}");
        }
        for agent in [AgentKind::Antigravity, AgentKind::Api] {
            assert_eq!(checklist_instructions(agent), None, "{agent:?}");
            let text = agent_instructions(&settings, agent);
            assert!(!text.contains("checklist"), "{agent:?}: {text}");
        }
    }

    #[test]
    fn the_instructions_carry_the_checklist_and_the_shell_line() {
        let shell = shell_instructions(AgentKind::Claude);
        let text = agent_instructions(&Settings::default(), AgentKind::Claude);
        assert!(text.starts_with("For any task with more than one step"));
        assert!(text.ends_with(&shell), "{text}");
        assert!(!text.contains("\n\n\n"), "no double blank line: {text:?}");
        assert!(!text.ends_with('\n'));
    }
}
