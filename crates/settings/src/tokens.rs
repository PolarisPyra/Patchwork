//! Settings → AI Tokenomics: ways to spend fewer tokens. What is shown,
//! when to flag or compact a long context, and which model new tasks start with.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};

use gpui::{AnyElement, SharedString, div, prelude::*, px};
use patchwork_core::provider::{AgentKind, ModelOption};
use patchwork_core::settings::{Settings, TokenSettings};

use crate::SettingsDialog;
use crate::controls::{choices, segmented, select_box, switch_row, token_row};
use patchwork_ui::theme::{Theme, theme};
use patchwork_ui::{AGENT_KINDS, agent_label, format_tokens};

/// Context sizes offered by the token settings.
const TOKEN_LEVELS: [u64; 4] = [100_000, 150_000, 200_000, 300_000];

/// Context sizes to compact at; 0 is When full, the agent's own, near the model's full window.
const COMPACT_WINDOWS: [u64; 5] = [128_000, 200_000, 256_000, 512_000, 0];

/// The models each agent offers, loaded once per opening of the pane.
#[derive(Default)]
pub struct TokensState {
    asked: HashSet<AgentKind>,
    pub(super) models: HashMap<AgentKind, Vec<ModelOption>>,
}

/// The field that holds `kind`'s compact window (`COMPACT_SETTINGS`).
fn compact_window_mut(t: &mut TokenSettings, kind: AgentKind) -> &mut u64 {
    match kind {
        AgentKind::Claude => &mut t.claude_compact_window,
        AgentKind::Codex => &mut t.codex_compact_window,
        AgentKind::Antigravity => &mut t.antigravity_compact_window,
        AgentKind::OpenCode => &mut t.opencode_compact_window,
        AgentKind::Api => &mut t.api_compact_window,
    }
}

/// A level's label, with 0 read as "When full".
fn level_label(level: u64) -> SharedString {
    if level == 0 {
        "When full".into()
    } else {
        format_tokens(level).into()
    }
}

/// A context size picked from a few (`ContextLevels`).
fn context_levels(
    id: &str,
    label: &str,
    value: u64,
    levels: &[u64],
    t: &Theme,
    set: impl Fn(&mut TokenSettings, u64) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let options: Vec<(u64, SharedString)> = levels.iter().map(|&l| (l, level_label(l))).collect();
    segmented(
        id,
        label.to_string(),
        value,
        &options,
        t,
        move |d, level, cx| d.update(|s| set(&mut s.tokens, level), cx),
        cx,
    )
    .into_any_element()
}

impl SettingsDialog {
    pub(super) fn load_models(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        if !self.tokens.asked.insert(kind) {
            return;
        }
        self.run(
            move || patchwork_core::provider::list_models(kind),
            move |d, list, _| {
                d.tokens.models.insert(kind, list);
            },
            cx,
        );
    }

    /// A model picked from one agent's models, as a bordered select (`DefaultModel`). The empty
    /// choice is `empty`, such as the CLI's own default.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    fn default_model(
        &mut self,
        kind: AgentKind,
        label: &str,
        empty: &'static str,
        value: Option<String>,
        set: impl Fn(&mut Settings, Option<String>) + Clone + 'static,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.load_models(kind, cx);
        let models: Vec<(String, String)> = self
            .tokens
            .models
            .get(&kind)
            .into_iter()
            .flatten()
            .filter_map(|m| m.id.clone().map(|id| (id, m.label.clone())))
            .collect();
        let current = match &value {
            None => empty.to_string(),
            Some(id) => models
                .iter()
                .find(|(m, _)| m == id)
                .map_or_else(|| id.clone(), |(_, name)| name.clone()),
        };
        let key = format!("model-{label}");
        let open = self.menu_open(&key);
        let options = std::iter::once((None, SharedString::new_static(empty)))
            .chain(models.into_iter().map(|(id, name)| (Some(id), name.into())))
            .collect();
        select_box(key.clone(), label.to_string(), current, open, t)
            .w(px(200.))
            .gap(px(4.))
            .on_click(choices(
                key,
                label.to_string(),
                options,
                value,
                move |d, pick, cx| d.update(|s| set(s, pick), cx),
                cx,
            ))
            .into_any_element()
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let tokens = d.settings.tokens.clone();
    let connected: Vec<AgentKind> = AGENT_KINDS
        .into_iter()
        .filter(|k| d.settings.agents.contains(k))
        .collect();

    let token_switch = |id: &'static str,
                        title: &'static str,
                        desc: Option<&'static str>,
                        on: bool,
                        flip: fn(&mut TokenSettings, bool),
                        cx: &mut Context<SettingsDialog>| {
        switch_row(
            id,
            title,
            desc,
            on,
            move |d, on, cx| d.update(|s| flip(&mut s.tokens, on), cx),
            cx,
        )
    };

    let mut rows: Vec<AnyElement> = vec![
        token_switch(
            "show-usage",
            "Show token counts",
            Some("Context, cache reads and writes, input and output under each finished reply"),
            tokens.show_usage,
            |t, on| t.show_usage = on,
            cx,
        )
        .into_any_element(),
        token_row(
            "Warn above",
            Some("Shows an above-threshold context warning below Worked for and above the divider"),
            &t,
        )
        .child(context_levels(
            "warn-above",
            "Warn above",
            tokens.warn_above,
            &TOKEN_LEVELS,
            &t,
            |t, level| t.warn_above = level,
            cx,
        ))
        .into_any_element(),
        token_switch(
            "generate-titles",
            "Generate chat and task titles",
            None,
            tokens.generate_titles,
            |t, on| t.generate_titles = on,
            cx,
        )
        .into_any_element(),
    ];
    if tokens.generate_titles {
        let picker = d.default_model(
            AgentKind::Api,
            "Write titles with",
            "The chat's model",
            tokens.title_model.clone(),
            |s, id| s.tokens.title_model = id,
            &t,
            cx,
        );
        rows.push(
            token_row("Write titles with", None, &t)
                .child(picker)
                .into_any_element(),
        );
        rows.push(
            token_switch(
                "titles-local",
                "Only local models write titles",
                Some(
                    "A chat's first message never leaves this computer or your network for a title",
                ),
                tokens.titles_local_only,
                |t, on| t.titles_local_only = on,
                cx,
            )
            .into_any_element(),
        );
    }
    for &kind in &connected {
        let title = format!("{} compacts at", agent_label(kind));
        let value = tokens.compact_window(kind);
        // A size saved before these levels stays shown, so the row still marks it.
        let mut levels = COMPACT_WINDOWS.to_vec();
        if !levels.contains(&value) {
            levels.push(value);
            // When full (0) stays last, as the largest.
            levels.sort_unstable_by_key(|&l| if l == 0 { u64::MAX } else { l });
        }
        rows.push(
            token_row(
                title.clone(),
                Some("Compacts a task once its context reaches this. When full leaves it to the agent, near the model's full window"),
                &t,
            )
            .child(context_levels(
                &format!("compact-{kind:?}"),
                &title,
                value,
                &levels,
                &t,
                move |t, level| *compact_window_mut(t, kind) = level,
                cx,
            ))
            .into_any_element(),
        );
    }
    for &kind in &connected {
        let name = agent_label(kind);
        let picker = d.default_model(
            kind,
            &format!("{name} model for new tasks"),
            "Default",
            tokens.default_models.get(&kind).cloned(),
            move |s, id| match id {
                Some(id) => {
                    s.tokens.default_models.insert(kind, id);
                }
                None => {
                    s.tokens.default_models.remove(&kind);
                }
            },
            &t,
            cx,
        );
        rows.push(
            token_row(
                format!("{name} model"),
                Some("Used by new tasks. A smaller model spends fewer tokens on each turn"),
                &t,
            )
            .child(picker)
            .into_any_element(),
        );
    }
    div().flex().flex_col().children(rows).into_any_element()
}
