//! Settings → Permissions (`system/PermissionsPane.tsx`): what agents may do without asking,
//! everywhere, in chats without a project, and in each project: a default for each kind of
//! request, and rules for commands. A rule in a project or in chats is checked before one for
//! everywhere; deny and ask rules win over allow rules. Every change saves at once.

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px,
};
use patchwork_server::permissions::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, ScopeRules,
    default_rules,
};
use patchwork_server::projects::Project;
use patchwork_server::system::new_id;

use crate::settings::SettingsDialog;
use crate::settings::controls::{
    FieldKind, choice, dropdown_chip, segmented, select_box, switch, token_row,
};
use crate::theme::{FONT_MONO, FS_SM, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, tooltip};

/// The kinds of request the pane offers rules for (`PERMISSION_KINDS`): its label and an example.
const PERMISSION_KINDS: [(PermissionKind, &str, &str); 1] =
    [(PermissionKind::Command, "Run commands", "git status")];

const EFFECTS: [(Effect, &str); 3] = [
    (Effect::Allow, "Allow"),
    (Effect::Ask, "Ask"),
    (Effect::Deny, "Deny"),
];

/// The field a new rule's pattern is typed in.
const ADD_FIELD: &str = "perm-add";

pub struct PermissionsState {
    scope: PermissionScope,
    /// The kind and pattern of the rule being added.
    kind: PermissionKind,
    pattern: String,
    /// The rule an approval opened the pane on, shown picked out.
    picked: Option<String>,
}

impl Default for PermissionsState {
    fn default() -> Self {
        Self {
            scope: PermissionScope::Everywhere,
            kind: PermissionKind::Command,
            pattern: String::new(),
            picked: None,
        }
    }
}

impl SettingsDialog {
    /// Opens the pane on `scope`, with `rule_id` picked out.
    pub fn focus_rule(&mut self, scope: PermissionScope, rule_id: Option<String>) {
        self.permissions.scope = scope;
        self.permissions.picked = rule_id;
    }
}

fn effect_label(effect: Effect) -> &'static str {
    EFFECTS
        .iter()
        .find(|(e, _)| *e == effect)
        .map_or("", |(_, label)| label)
}

pub(crate) fn kind_label(kind: PermissionKind) -> &'static str {
    PERMISSION_KINDS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map_or("", |(_, label, _)| label)
}

pub(crate) fn scope_name(scope: &PermissionScope, projects: &[Project]) -> String {
    match scope {
        PermissionScope::Everywhere => "Everywhere".into(),
        PermissionScope::Chats => "Chats".into(),
        PermissionScope::Project(id) => projects
            .iter()
            .find(|p| p.id == *id)
            .map_or_else(|| "Removed project".into(), |p| p.name.clone()),
    }
}

fn scope_rules(permissions: &PermissionSettings, scope: &PermissionScope) -> ScopeRules {
    match scope {
        PermissionScope::Everywhere => permissions.everywhere.clone(),
        PermissionScope::Chats => permissions.chats.clone(),
        PermissionScope::Project(id) => permissions.projects.get(id).cloned().unwrap_or_default(),
    }
}

fn scope_rules_mut<'a>(
    permissions: &'a mut PermissionSettings,
    scope: &PermissionScope,
) -> &'a mut ScopeRules {
    match scope {
        PermissionScope::Everywhere => &mut permissions.everywhere,
        PermissionScope::Chats => &mut permissions.chats,
        PermissionScope::Project(id) => permissions.projects.entry(id.clone()).or_default(),
    }
}

/// `rules` with the starting rules put back as they first were: deleted ones added again and
/// changed ones reset. The user's own rules stay, and a starting rule is not added again where one
/// of them already covers the same pattern.
fn with_default_rules(
    rules: &[PermissionRule],
    defaults: &[PermissionRule],
) -> Vec<PermissionRule> {
    let own: Vec<_> = rules
        .iter()
        .filter(|r| !defaults.iter().any(|d| d.id == r.id))
        .cloned()
        .collect();
    let restored = defaults
        .iter()
        .filter(|d| {
            !own.iter()
                .any(|r| r.kind == d.kind && r.pattern.trim() == d.pattern)
        })
        .cloned();
    restored.chain(own.iter().cloned()).collect()
}

/// Whether restoring the starting rules would change `rules`.
fn defaults_changed(rules: &[PermissionRule], defaults: &[PermissionRule]) -> bool {
    let restored = with_default_rules(rules, defaults);
    restored.len() != rules.len() || restored.iter().any(|r| !rules.contains(r))
}

/// The pane's headings (`HEADING`).
fn heading(text: &'static str, t: &Theme) -> Div {
    div()
        .mt(px(16.))
        .mb(px(2.))
        .text_size(px(FS_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text_muted)
        .child(text)
}

impl SettingsDialog {
    /// Saves the rules of `scope` as `change` leaves them.
    fn change_scope(
        &mut self,
        scope: &PermissionScope,
        change: impl FnOnce(&mut ScopeRules),
        cx: &mut Context<Self>,
    ) {
        self.update(|s| change(scope_rules_mut(&mut s.permissions, scope)), cx);
    }

    fn change_rule(
        &mut self,
        scope: &PermissionScope,
        id: &str,
        change: impl FnOnce(&mut PermissionRule),
        cx: &mut Context<Self>,
    ) {
        self.change_scope(
            scope,
            |s| {
                if let Some(rule) = s.rules.iter_mut().find(|r| r.id == id) {
                    change(rule);
                }
            },
            cx,
        );
    }

    fn move_rule(
        &mut self,
        from: &PermissionScope,
        to: &PermissionScope,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        self.update(
            |s| {
                let rules = &mut scope_rules_mut(&mut s.permissions, from).rules;
                let Some(at) = rules.iter().position(|r| r.id == id) else {
                    return;
                };
                let rule = rules.remove(at);
                scope_rules_mut(&mut s.permissions, to).rules.push(rule);
            },
            cx,
        );
    }

    /// Adds an allow rule for the pattern typed, then empties the field.
    fn add_rule(&mut self, cx: &mut Context<Self>) {
        let pattern = self.permissions.pattern.trim().to_string();
        if pattern.is_empty() {
            return;
        }
        let (scope, kind) = (self.permissions.scope.clone(), self.permissions.kind);
        self.change_scope(
            &scope,
            |s| {
                s.rules.push(PermissionRule {
                    id: new_id(),
                    kind,
                    pattern,
                    effect: Effect::Allow,
                })
            },
            cx,
        );
        self.permissions.pattern.clear();
        if let Some(area) = self.field_area(ADD_FIELD) {
            area.update(cx, |a, cx| a.set_text("", cx));
        }
    }
}

fn rule_row(
    d: &mut SettingsDialog,
    rule: &PermissionRule,
    placeholder: &str,
    targets: &[(PermissionScope, String)],
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    let t = *theme(cx);
    let scope = d.permissions.scope.clone();
    let id = rule.id.clone();
    let picked = d.permissions.picked.as_deref() == Some(id.as_str());
    let field = d.field(
        format!("perm-rule-{id}"),
        format!("{} rule", kind_label(rule.kind)),
        &rule.pattern,
        placeholder,
        FieldKind {
            mono: true,
            ..Default::default()
        },
        {
            let (scope, id) = (scope.clone(), id.clone());
            move |d, text, cx| {
                let pattern = text.trim().to_string();
                if !pattern.is_empty() {
                    d.change_rule(&scope, &id, |r| r.pattern = pattern, cx);
                }
            }
        },
        window,
        cx,
    );
    let effect = segmented(
        &format!("perm-effect-{id}"),
        format!("What {} gets", rule.pattern),
        rule.effect,
        &EFFECTS,
        &t,
        {
            let (scope, id) = (scope.clone(), id.clone());
            move |d, effect, cx| d.change_rule(&scope, &id, |r| r.effect = effect, cx)
        },
        cx,
    );
    let menu_key = SharedString::from(format!("perm-move-{id}"));
    let open = d.menu_open(&menu_key);
    let targets = targets.to_vec();
    let move_button = icon_button(
        menu_key.clone(),
        format!("Move {}", rule.pattern),
        "dots-three",
        14.,
        cx,
    )
    .aria_expanded(open)
    .text_color(t.text_muted)
    .tooltip(tooltip("Move to…"))
    .on_click(cx.listener({
        let (scope, id) = (scope.clone(), id.clone());
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = targets
                .iter()
                .map(|(to, name)| {
                    let (from, to, id) = (scope.clone(), to.clone(), id.clone());
                    choice(
                        weak.clone(),
                        format!("Move to {name}"),
                        false,
                        move |d, cx| {
                            d.menu = None;
                            d.move_rule(&from, &to, &id, cx);
                        },
                    )
                })
                .collect();
            d.toggle_menu(
                menu_key.clone(),
                "Move rule",
                items,
                e.position(),
                window,
                cx,
            );
        }
    }));
    let delete = icon_button(
        SharedString::from(format!("perm-delete-{id}")),
        format!("Delete {}", rule.pattern),
        "x",
        14.,
        cx,
    )
    .on_click(cx.listener(move |d, _, _, cx| {
        d.change_scope(&scope, |s| s.rules.retain(|r| r.id != id), cx)
    }));
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .mx(px(-6.))
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .py(px(2.))
        .when(picked, |row| row.bg(t.bg_selected_muted))
        .child(field.flex_1())
        .child(effect)
        .child(move_button)
        .child(delete)
}

/// A row that adds an allow rule (`AddRule`).
fn add_rule_row(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    let t = *theme(cx);
    let kind = d.permissions.kind;
    let placeholder = PERMISSION_KINDS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map_or("", |(.., p)| p);
    let pattern = d.permissions.pattern.clone();
    let field = d.field(
        ADD_FIELD,
        "Pattern",
        &pattern,
        placeholder,
        FieldKind {
            live: true,
            mono: true,
            ..Default::default()
        },
        |d, text, cx| {
            d.permissions.pattern = text;
            cx.notify();
        },
        window,
        cx,
    );
    d.on_submit(ADD_FIELD, |d, _, cx| d.add_rule(cx));
    let key = "perm-kind";
    let open = d.menu_open(key);
    let select = select_box(key, "Kind of rule", kind_label(kind), open, &t).on_click(cx.listener(
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = PERMISSION_KINDS
                .iter()
                .map(|&(k, label, _)| {
                    choice(
                        weak.clone(),
                        label,
                        k == d.permissions.kind,
                        move |d, cx| {
                            d.menu = None;
                            d.permissions.kind = k;
                            cx.notify();
                        },
                    )
                })
                .collect();
            d.toggle_menu(key, "Kind of rule", items, e.position(), window, cx);
        },
    ));
    let empty = pattern.trim().is_empty();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .py(px(10.))
        .child(select)
        .child(field.flex_1().font_family(FONT_MONO))
        .child(
            btn("perm-add-rule", "Add rule", BtnKind::Plain, empty, &t)
                .child("Add rule")
                .when(!empty, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| d.add_rule(cx)))
                }),
        )
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let scope = d.permissions.scope.clone();
    let permissions = d.settings.permissions.clone();
    let projects = d.settings.projects.clone();
    let own = scope_rules(&permissions, &scope);
    let everywhere = scope == PermissionScope::Everywhere;
    let defaults = default_rules();
    let restorable = everywhere && defaults_changed(&own.rules, &defaults);
    let scopes: Vec<PermissionScope> = [PermissionScope::Everywhere, PermissionScope::Chats]
        .into_iter()
        .chain(
            projects
                .iter()
                .map(|p| PermissionScope::Project(p.id.clone())),
        )
        .collect();

    let name = scope_name(&scope, &projects);
    let scope_menu = "perm-scope";
    let scope_chip = dropdown_chip(
        scope_menu,
        format!("Rules for {name}"),
        name,
        d.menu_open(scope_menu),
        &t,
    )
    .on_click(cx.listener({
        let (scopes, permissions, projects) =
            (scopes.clone(), permissions.clone(), projects.clone());
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = scopes
                .iter()
                .map(|s| {
                    let count = scope_rules(&permissions, s).rules.len();
                    let name = scope_name(s, &projects);
                    let label = if count > 0 {
                        format!("{name} · {count}")
                    } else {
                        name
                    };
                    let pick = s.clone();
                    choice(
                        weak.clone(),
                        label,
                        *s == d.permissions.scope,
                        move |d, cx| {
                            d.menu = None;
                            d.permissions.scope = pick.clone();
                            cx.notify();
                        },
                    )
                })
                .collect();
            d.toggle_menu(scope_menu, "Rule scope", items, e.position(), window, cx);
        }
    }));
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .border_b_1()
        .border_color(t.border)
        .pb(px(10.))
        .child(scope_chip)
        .when(everywhere, |row| {
            row.child(
                btn(
                    "perm-restore",
                    "Restore defaults",
                    BtnKind::Plain,
                    !restorable,
                    &t,
                )
                .ml_auto()
                .px(px(10.))
                .gap(px(6.))
                .child(icon("arrow-counter-clockwise", 14.))
                .child("Restore defaults")
                .map(|b| {
                    if restorable {
                        b.on_click(cx.listener(move |d, _, _, cx| {
                            let defaults = defaults.clone();
                            d.change_scope(
                                &PermissionScope::Everywhere,
                                |s| s.rules = with_default_rules(&s.rules, &defaults),
                                cx,
                            );
                        }))
                    } else {
                        b.tooltip(tooltip("The starting rules are all in place"))
                    }
                }),
            )
        });

    let default_rows = PERMISSION_KINDS.iter().map(|&(kind, label, _)| {
        let inherited = permissions
            .everywhere
            .defaults
            .get(&kind)
            .copied()
            .unwrap_or(Effect::Ask);
        let value = own
            .defaults
            .get(&kind)
            .copied()
            .or(everywhere.then_some(Effect::Ask));
        let mut options: Vec<(Option<Effect>, String)> = Vec::new();
        if !everywhere {
            options.push((None, format!("Inherit · {}", effect_label(inherited))));
        }
        options.extend(EFFECTS.iter().map(|&(e, l)| (Some(e), l.to_string())));
        let scope = scope.clone();
        token_row(label, None, &t).child(segmented(
            &format!("perm-default-{label}"),
            label,
            value,
            &options,
            &t,
            move |d, next, cx| {
                d.change_scope(
                    &scope,
                    |s| match next {
                        Some(effect) => {
                            s.defaults.insert(kind, effect);
                        }
                        None => {
                            s.defaults.remove(&kind);
                        }
                    },
                    cx,
                )
            },
            cx,
        ))
    });
    let default_rows: Vec<_> = default_rows.collect();

    let targets: Vec<(PermissionScope, String)> = scopes
        .iter()
        .filter(|s| **s != scope)
        .map(|s| (s.clone(), scope_name(s, &projects)))
        .collect();
    let mut rule_groups = Vec::new();
    for &(kind, label, placeholder) in &PERMISSION_KINDS {
        let rules: Vec<_> = own.rules.iter().filter(|r| r.kind == kind).collect();
        if rules.is_empty() {
            continue;
        }
        let rows: Vec<_> = rules
            .into_iter()
            .map(|rule| rule_row(d, rule, placeholder, &targets, window, cx))
            .collect();
        rule_groups.push(
            div()
                .border_b_1()
                .border_color(t.border)
                .py(px(8.))
                .child(
                    div()
                        .mb(px(4.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .children(rows),
        );
    }
    let add = add_rule_row(d, window, cx);

    let screen_on = d.settings.computer_use
        && permissions.everywhere.defaults.get(&PermissionKind::Screen) == Some(&Effect::Allow);
    let computer_use = token_row(
        "Let the agent use the screen",
        Some("Adds screenshot, click, type, key and scroll tools to Patchwork's own agent."),
        &t,
    )
    .child(
        switch("perm-computer-use", "Computer use", screen_on, &t).on_click(cx.listener(
            move |d, _, _, cx| {
                d.update(
                    |s| {
                        s.computer_use = !screen_on;
                        let defaults = &mut s.permissions.everywhere.defaults;
                        if screen_on {
                            defaults.remove(&PermissionKind::Screen);
                        } else {
                            defaults.insert(PermissionKind::Screen, Effect::Allow);
                        }
                    },
                    cx,
                )
            },
        )),
    );

    div()
        .flex()
        .flex_col()
        .child(header)
        .child(heading("Defaults", &t))
        .children(default_rows)
        .child(heading("Rules", &t))
        .children(rule_groups)
        .child(add)
        .child(heading("Computer use", &t))
        .child(computer_use)
        .into_any_element()
}
