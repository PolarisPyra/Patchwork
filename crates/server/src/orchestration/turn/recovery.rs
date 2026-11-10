//! What a new app session does with the goals and replies the last one left behind: replies
//! that were cut off end with an error, and goals are dismissed or paused.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::persistence::Store;
use crate::threads::CheckStatus;
use crate::threads::GoalStatus;
use crate::threads::MessageStatus;
use crate::orchestration::goal;

/// Finished goals remain visible for the session in which they ended. A new app session
/// dismisses them, including cached reports from the provider on its next run. Unfinished
/// goals survive, paused if their agent was interrupted by quitting, and replies that were
/// still streaming end with an error.
pub(crate) fn recover(store: &Store) -> Result<(), Error> {
    for mut thread in store.threads() {
        if let Some(finished) = thread
            .goal
            .as_ref()
            .filter(|goal| matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked))
        {
            store.dismiss_goal(&thread.id, &goal::signature(finished))?;
            thread.goal = None;
        }
        let dismissed = thread
            .goal
            .as_ref()
            .is_some_and(|g| goal::is_dismissed(store, &thread.id, g));
        let goal_active = thread
            .goal
            .as_ref()
            .is_some_and(|g| g.status == GoalStatus::Active);
        let streaming = thread
            .messages
            .iter()
            .any(|m| m.status == MessageStatus::Streaming);
        // A check that was running when the app closed never finished, so it is not a pass.
        let checking = thread.messages.iter().any(|m| {
            m.checks
                .as_ref()
                .is_some_and(|c| c.status == CheckStatus::Running)
        });
        if dismissed || goal_active || streaming || checking {
            store.update_thread(&thread.id, |t| {
                if dismissed {
                    t.goal = None;
                } else if let Some(goal) =
                    t.goal.as_mut().filter(|g| g.status == GoalStatus::Active)
                {
                    goal.status = GoalStatus::Paused;
                }
                for message in &mut t.messages {
                    if message.status == MessageStatus::Streaming {
                        message.status = MessageStatus::Error;
                        message.error =
                            Some("Patchwork closed while this reply was running.".into());
                    }
                    if let Some(checks) = message
                        .checks
                        .as_mut()
                        .filter(|c| c.status == CheckStatus::Running)
                    {
                        checks.status = CheckStatus::NotRun;
                        checks.note =
                            Some("Patchwork closed while the checks were running.".into());
                    }
                }
            })?;
        }
    }
    Ok(())
}
