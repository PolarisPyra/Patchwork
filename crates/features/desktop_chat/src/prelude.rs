//! The chat's traits on the window, to bring their methods into scope: `use patchwork_desktop_chat::prelude::*`.

// Rust guideline compliant 2026-11-27

pub use crate::window::ChatWindow;
pub use crate::worktree_marks::WorktreeMarksExt as _;
pub use crate::backdrop::BackdropExt as _;
pub use crate::worktree_switch::WorktreeSwitchExt as _;
pub use crate::tabs::TabsExt as _;
pub use crate::session::SessionExt as _;
pub use crate::above_composer::approval_banner::ApprovalBannerExt as _;
pub use crate::above_composer::proposed_plan::ProposedPlanExt as _;
pub use crate::above_composer::interrupted_reply::InterruptedReplyExt as _;
pub use crate::above_composer::plan_steps::PlanStepsExt as _;
pub use crate::above_composer::secret_request::SecretRequestExt as _;
pub use crate::above_composer::agent_questions::AgentQuestionsExt as _;
pub use crate::above_composer::in_terminal::InTerminalExt as _;
pub use crate::above_composer::goal_bar::GoalBarExt as _;
pub use crate::above_composer::queued_messages::QueuedMessagesExt as _;
pub use crate::above_composer::worktree_status::WorktreeStatusExt as _;
pub use crate::above_composer::missing_folder::MissingFolderExt as _;
pub use crate::panes::docking::DockingExt as _;
pub use crate::panes::named::NamedExt as _;
pub use crate::panes::saved::SavedExt as _;
pub use crate::panes::presets::PresetsExt as _;
pub use crate::panes::commands::CommandsExt as _;
pub use crate::panes::PanesExt as _;
pub use crate::top_right::find_bar::FindBarExt as _;
pub use crate::top_right::subagent_list::SubagentListExt as _;
pub use crate::transcript::turn_rail::TurnRailExt as _;
pub use crate::transcript::message_list::MessageListExt as _;
pub use crate::transcript::user_message::UserMessageExt as _;
pub use crate::transcript::reply::changed_files::ChangedFilesExt as _;
pub use crate::transcript::reply::approvals::ApprovalsExt as _;
pub use crate::transcript::reply::background_tasks::BackgroundTasksExt as _;
pub use crate::transcript::reply::checks::ChecksExt as _;
