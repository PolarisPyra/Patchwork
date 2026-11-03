//! Tells when an agent is stuck in a loop of tool calls, such as a model calling one tool again
//! and again and getting the same answer, so the turn can be stopped before it burns through
//! tokens. Agents have their own guards for this, but not every model or plugin trips them.

use std::collections::HashMap;

use crate::threads::ToolCall;

/// How many times in a row the same call may repeat before the turn is stopped.
pub(in crate::orchestration) const LOOP_LIMIT: usize = 8;

/// The calls of one turn, and how the latest ones repeat.
#[derive(Default)]
pub(in crate::orchestration) struct Repeats {
    /// Calls under way: their tool's name, that name with what the call was given, and whether a
    /// file changed while it ran.
    calls: HashMap<String, (String, String, bool)>,
    /// What the latest finished call came to, and how many in a row came to the same.
    last: Option<String>,
    count: usize,
    /// A file changed since the latest finished call. Providers report it as the call starts or
    /// just after it ends, so it is taken up by the next call that finishes.
    changed: bool,
}

impl Repeats {
    /// A call changed a file. Edits to one file all look alike, the same path and the same
    /// "updated" answer, yet each is progress. Every call under way is marked, since a batch of
    /// edits made at once all start before the first one finishes.
    pub(in crate::orchestration) fn changed_file(&mut self) {
        self.changed = true;
        self.calls.values_mut().for_each(|call| call.2 = true);
    }

    /// Codex reports a call when it starts and again when it completes; the first counts.
    pub(in crate::orchestration) fn started(&mut self, tool: &ToolCall) {
        let call = format!("{}\0{}", tool.name, tool.summary);
        self.calls
            .entry(tool.id.clone())
            .or_insert((tool.name.clone(), call, false));
    }

    /// Records a finished call. Returns its tool's name once the agent has made the same call
    /// with the same result, or had the same call fail whatever the error said, `LOOP_LIMIT`
    /// times in a row. Different calls that fail, such as searches that find nothing, do not
    /// count as a loop, and neither does a call that succeeded after a file changed.
    pub(in crate::orchestration) fn finished(
        &mut self,
        id: &str,
        failed: bool,
        output: Option<&str>,
    ) -> Option<String> {
        let (name, call, changed_while_running) = self.calls.remove(id)?;
        let changed = std::mem::take(&mut self.changed) || changed_while_running;
        if changed && !failed {
            self.last = None;
            self.count = 0;
            return None;
        }
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
