//! Tells when an agent is stuck in a loop of tool calls, such as a model calling one tool again
//! and again and getting the same answer, so the turn can be stopped before it burns through
//! tokens. Agents have their own guards for this, but not every model or plugin trips them.

use std::collections::HashMap;

use crate::types::ToolCall;

/// How many times in a row the same call may repeat before the turn is stopped.
pub(super) const LOOP_LIMIT: usize = 8;

/// The calls of one turn, and how the latest ones repeat.
#[derive(Default)]
pub(super) struct Repeats {
    /// Calls under way: their tool's name, and that name with what the call was given.
    calls: HashMap<String, (String, String)>,
    /// What the latest finished call came to, and how many in a row came to the same.
    last: Option<String>,
    count: usize,
}

impl Repeats {
    /// Codex reports a call when it starts and again when it completes; the first counts.
    pub(super) fn started(&mut self, tool: &ToolCall) {
        let call = format!("{}\0{}", tool.name, tool.summary);
        self.calls
            .entry(tool.id.clone())
            .or_insert((tool.name.clone(), call));
    }

    /// Records a finished call. Returns its tool's name once the agent has made the same call
    /// with the same result, or had the same call fail whatever the error said, `LOOP_LIMIT`
    /// times in a row. Different calls that fail, such as searches that find nothing, do not
    /// count as a loop.
    pub(super) fn finished(
        &mut self,
        id: &str,
        failed: bool,
        output: Option<&str>,
    ) -> Option<String> {
        let (name, call) = self.calls.remove(id)?;
        let outcome = if failed {
            format!("{call}\0failed")
        } else {
            format!("{call}\0{}", output.unwrap_or_default())
        };
        if self.last.as_ref() == Some(&outcome) {
            self.count += 1;
        } else {
            self.last = Some(outcome);
            self.count = 1;
        }
        (self.count == LOOP_LIMIT).then_some(name)
    }
}
