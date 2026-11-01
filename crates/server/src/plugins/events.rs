//! The events a hook runs on: their names, and which of them Patchwork runs itself.

use super::HookEvent;

impl HookEvent {
    pub const ALL: [HookEvent; 12] = [
        HookEvent::UserPromptSubmit,
        HookEvent::SessionStart,
        HookEvent::PreToolUse,
        HookEvent::PermissionRequest,
        HookEvent::PostToolUse,
        HookEvent::PreCompact,
        HookEvent::PostCompact,
        HookEvent::SubagentStart,
        HookEvent::SubagentStop,
        HookEvent::Stop,
        HookEvent::Interrupt,
        HookEvent::SessionEnd,
    ];

    /// The event as Patchwork names it, and as `PATCHWORK_EVENT` gives it: `preToolUse`.
    pub fn name(self) -> &'static str {
        match self {
            HookEvent::UserPromptSubmit => "userPromptSubmit",
            HookEvent::SessionStart => "sessionStart",
            HookEvent::PreToolUse => "preToolUse",
            HookEvent::PermissionRequest => "permissionRequest",
            HookEvent::PostToolUse => "postToolUse",
            HookEvent::PreCompact => "preCompact",
            HookEvent::PostCompact => "postCompact",
            HookEvent::SubagentStart => "subagentStart",
            HookEvent::SubagentStop => "subagentStop",
            HookEvent::Stop => "stop",
            HookEvent::Interrupt => "interrupt",
            HookEvent::SessionEnd => "sessionEnd",
        }
    }

    /// The event as the agent CLIs name it: `PreToolUse`.
    pub fn cli_name(self) -> String {
        let name = self.name();
        name[..1].to_ascii_uppercase() + &name[1..]
    }

    /// The event a manifest names, whatever its case.
    pub fn parse(name: &str) -> Option<HookEvent> {
        HookEvent::ALL
            .into_iter()
            .find(|event| event.name().eq_ignore_ascii_case(name))
    }

    /// Whether Patchwork runs the event itself, for every agent, since only it knows how a reply
    /// ended. The rest happen inside the agent, so the agent runs them on its own hooks.
    pub fn runs_in_patchwork(self) -> bool {
        matches!(
            self,
            HookEvent::UserPromptSubmit
                | HookEvent::Stop
                | HookEvent::Interrupt
                | HookEvent::SessionEnd
        )
    }
}
