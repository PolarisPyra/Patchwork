//! Settings → Providers' state, and finding, installing and saving providers.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use gpui::{Context, PathPromptOptions};
use patchwork_client::local::core::provider::api::{self, ApiProvider, KeyChange, ProviderView};
use patchwork_client::local::core::provider::{
    AgentKind, AgentStatus, CliUpdate, DISPLAY_ORDER, ModelOption,
};
use crate::settings::SettingsDialog;
use super::AntigravityAuth;
use super::cli;
use super::form;
use super::local;

/// The agents with a CLI of their own, in the order the pane lists them.
pub(super) fn cli_kinds() -> impl Iterator<Item = AgentKind> {
    DISPLAY_ORDER
        .into_iter()
        .filter(|kind| kind.identity().cli.is_some())
}

/// Agents whose models can read a message's images for an API model.
pub(super) const VISION_AGENTS: [AgentKind; 3] =
    [AgentKind::Api, AgentKind::Claude, AgentKind::Codex];

/// The field the open system prompt is typed in.
pub(super) const PROMPT_FIELD: &str = "provider-prompt";

/// Whose system prompt is open: a CLI's, kept in the settings, or a provider's, by id.
#[derive(Clone, PartialEq)]
pub(super) enum PromptOf {
    Cli(AgentKind),
    Api(String),
}

/// The system prompt being edited: what it was, what is typed, and whether it is saving.
pub(super) struct Prompt {
    pub(super) of: PromptOf,
    pub(super) saved: String,
    pub(super) draft: String,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
}

/// A newer release of a CLI, and where updating it stands.
pub(super) struct Update {
    pub(super) release: CliUpdate,
    pub(super) step: UpdateStep,
}

pub(super) enum UpdateStep {
    Offered,
    Running,
    Failed(String),
}

#[derive(Default)]
pub struct ProvidersState {
    /// Whether the agents were read and the providers loaded since the dialog opened.
    pub(super) started: bool,
    /// Each CLI's reading, by kind: missing while it is checked, None when it could not be.
    pub(super) status: HashMap<AgentKind, Option<AgentStatus>>,
    pub(super) installing: HashSet<AgentKind>,
    pub(super) install_errors: HashMap<AgentKind, String>,
    /// Each installed CLI with a newer release, and where updating it stands.
    pub(super) updates: HashMap<AgentKind, Update>,
    pub(super) providers: Vec<ProviderView>,
    /// The models of each agent the vision picker offers.
    pub(super) models: HashMap<AgentKind, Vec<ModelOption>>,
    pub(super) prompt: Option<Prompt>,
    pub(super) antigravity_auth: Option<AntigravityAuth>,
    pub(super) form: Option<form::Form>,
    pub(super) details: Option<cli::Details>,
    pub(super) local: local::LocalState,
    pub(super) error: Option<String>,
}

impl SettingsDialog {
    /// Reads each CLI again, and loads the providers, the first time the pane draws.
    pub(super) fn start_providers(&mut self, cx: &mut Context<Self>) {
        self.providers.started = true;
        for kind in cli_kinds() {
            self.detect(kind, cx);
        }
        self.reload_providers(cx);
        self.discover_servers(cx);
    }

    pub(super) fn detect(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.run(
            move || patchwork_client::local::core::provider::detect_agent(kind),
            move |d, status, cx| {
                if status.installed {
                    d.check_update(kind, cx);
                } else {
                    d.providers.updates.remove(&kind);
                }
                d.providers.status.insert(kind, Some(status));
            },
            cx,
        );
    }

    /// Looks up whether `kind`'s CLI has a newer release, keeping an update that is running.
    fn check_update(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        let running = move |d: &Self| {
            d.providers
                .updates
                .get(&kind)
                .is_some_and(|u| matches!(u.step, UpdateStep::Running))
        };
        if running(self) {
            return;
        }
        self.run(
            move || patchwork_client::local::core::provider::check_cli_update(kind),
            move |d, found, _| {
                if running(d) {
                    return;
                }
                match found {
                    Ok(Some(release)) => {
                        d.providers.updates.insert(
                            kind,
                            Update {
                                release,
                                step: UpdateStep::Offered,
                            },
                        );
                    }
                    Ok(None) => {
                        d.providers.updates.remove(&kind);
                    }
                    Err(e) => {
                        log::warn!(agent:? = kind, error:% = e; "could not check {{agent}} for updates: {{error}}");
                        d.providers.updates.remove(&kind);
                    }
                }
            },
            cx,
        );
    }

    /// Runs `kind`'s own update command, then reads the CLI again.
    pub(super) fn update_cli(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        let Some(update) = self.providers.updates.get_mut(&kind) else {
            return;
        };
        if matches!(update.step, UpdateStep::Running) {
            return;
        }
        update.step = UpdateStep::Running;
        self.run(
            move || patchwork_client::local::core::provider::update_cli(kind),
            move |d, done, cx| match done {
                Ok(()) => {
                    d.providers.updates.remove(&kind);
                    d.detect(kind, cx);
                }
                Err(e) => {
                    if let Some(update) = d.providers.updates.get_mut(&kind) {
                        update.step = UpdateStep::Failed(e.to_string());
                    }
                }
            },
            cx,
        );
    }

    /// Loads the saved providers, then puts the API agent among the connected ones exactly when a
    /// provider can run a task (`syncAgents`).
    pub(super) fn reload_providers(&mut self, cx: &mut Context<Self>) {
        self.run(
            api::list,
            |d, list, cx| {
                patchwork_views_logos::reload_api_providers();
                let usable = list
                    .iter()
                    .any(|p| p.provider.enabled && !p.provider.models.is_empty());
                if usable != d.settings.agents.contains(&AgentKind::Api) {
                    d.update(
                        |s| {
                            if usable {
                                s.agents.push(AgentKind::Api);
                            } else {
                                s.agents.retain(|k| *k != AgentKind::Api);
                            }
                        },
                        cx,
                    );
                }
                let ids: Vec<String> = list
                    .iter()
                    .filter(|p| p.provider.local)
                    .map(|p| p.provider.id.clone())
                    .collect();
                d.providers.providers = list;
                d.probe_local(ids, cx);
                d.load_vision_models(cx);
            },
            cx,
        );
    }

    /// Saves a provider as it is with its key kept, then loads the list again.
    pub(super) fn save_provider(&mut self, provider: ApiProvider, cx: &mut Context<Self>) {
        self.providers.error = None;
        self.run(
            move || api::save(provider, KeyChange::Keep),
            |d, saved, cx| match saved {
                Ok(_) => d.reload_providers(cx),
                Err(e) => d.providers.error = Some(e.to_string()),
            },
            cx,
        );
    }

    pub(super) fn load_vision_models(&mut self, cx: &mut Context<Self>) {
        for kind in VISION_AGENTS {
            if !self.settings.agents.contains(&kind) || self.providers.models.contains_key(&kind) {
                continue;
            }
            self.run(
                move || patchwork_client::local::core::provider::list_models(kind),
                move |d, list, _| {
                    d.providers.models.insert(kind, list);
                },
                cx,
            );
        }
    }

    pub(super) fn install(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.providers.installing.insert(kind);
        self.providers.install_errors.remove(&kind);
        let core = Arc::clone(&self.core);
        self.run(
            move || core.install_cli(kind),
            move |d, done, cx| {
                d.providers.installing.remove(&kind);
                match done {
                    Ok(()) => d.detect(kind, cx),
                    Err(e) => {
                        d.providers.install_errors.insert(kind, e.to_string());
                    }
                }
            },
            cx,
        );
    }

    /// Uses `path` for the CLI, or finds it again when None, then reads it again.
    pub(super) fn set_agent_path(
        &mut self,
        kind: AgentKind,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.providers.status.remove(&kind);
        self.update(
            |s| match path {
                Some(path) => {
                    s.agent_paths.insert(kind, path);
                }
                None => {
                    s.agent_paths.remove(&kind);
                }
            },
            cx,
        );
        self.detect(kind, cx);
    }

    pub(super) fn locate(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        patchwork_design_tasks::pick_paths(
            cx,
            PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            },
            move |d, paths, cx| {
                let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                    return;
                };
                d.set_agent_path(kind, Some(path), cx);
            },
        );
    }
}
