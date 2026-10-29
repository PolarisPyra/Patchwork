import { useEffect, useRef, useState } from "react";
import { api } from "~/api";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowCounterClockwiseIcon,
  CaretDownIcon,
  DotsThreeIcon,
  FolderOpenIcon,
  XIcon,
} from "@phosphor-icons/react";
import type {
  Effect,
  PermissionKind,
  PermissionRule,
  PermissionScope,
  PermissionSettings,
  Settings,
} from "~/api";
import type { UpdateSettings } from "~/api";
import { CHIP_CLASS } from "~/components/chip";
import {
  Segmented,
  TextField,
  TokenRow,
  TokenSwitch,
} from "~/features/settings/components/controls";
import Dropdown from "~/components/Dropdown";
import {
  defaultsChanged,
  EFFECT_LABELS,
  PERMISSION_KINDS,
  sameScope,
  scopeName,
  scopeRules,
  withDefaultRules,
  withScopeRules,
} from "~/lib/permissions";

/** The scope the pane opens on, and the rule to pick out. */
export interface PermissionsFocus {
  scope: PermissionScope;
  ruleId?: string;
}

const EFFECTS: Effect[] = ["allow", "ask", "deny"];

const MENU_ITEM =
  "flex cursor-pointer items-center gap-2 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover";

const HEADING = "mt-4 mb-0.5 text-[length:var(--fs-sm)] font-semibold text-fg-muted";

/** What agents may do without asking, everywhere, in chats without a project, and in each
 * project: a default for each kind of request, and rules for folders, commands, web hosts, and
 * MCP servers. A rule in a project or in chats is checked before one for everywhere; deny and ask
 * rules win over allow rules. Every change saves at once and applies to requests already waiting. */
export default function PermissionsPane({
  settings,
  updateSettings,
  focus,
}: {
  settings: Settings;
  updateSettings: UpdateSettings;
  focus: PermissionsFocus | null;
}) {
  const [scope, setScope] = useState<PermissionScope>(focus?.scope ?? { type: "everywhere" });
  const { permissions, projects } = settings;
  const own = scopeRules(permissions, scope);
  const everywhere = scope.type === "everywhere";
  const change = (f: (p: PermissionSettings) => PermissionSettings) =>
    void updateSettings((s) => ({ ...s, permissions: f(s.permissions) }));
  const changeRules = (f: (rules: PermissionRule[]) => PermissionRule[]) =>
    change((p) => withScopeRules(p, scope, (s) => ({ ...s, rules: f(s.rules) })));
  const updateRule = (id: string, patch: Partial<PermissionRule>) =>
    changeRules((rules) => rules.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  // The starting rules, for Restore defaults; they live in the backend.
  const [defaults, setDefaults] = useState<PermissionRule[] | null>(null);
  useEffect(() => {
    api.defaultPermissionRules().then(setDefaults, () => setDefaults(null));
  }, []);
  const restorable = everywhere && !!defaults && defaultsChanged(own.rules, defaults);

  const scopes: PermissionScope[] = [
    { type: "everywhere" },
    { type: "chats" },
    ...projects.map((p) => ({ type: "project" as const, projectId: p.id })),
  ];
  const moveRule = (rule: PermissionRule, to: PermissionScope) =>
    change((p) =>
      withScopeRules(
        withScopeRules(p, scope, (s) => ({ ...s, rules: s.rules.filter((r) => r.id !== rule.id) })),
        to,
        (s) => ({ ...s, rules: [...s.rules, rule] }),
      ),
    );

  return (
    <>
      <div className="flex items-center gap-2 border-b border-border pb-2.5">
        <Dropdown
          trigger={({ open: isOpen, toggle }) => (
            <button
              className={CHIP_CLASS}
              aria-haspopup="menu"
              aria-expanded={isOpen}
              aria-label={`Rules for ${scopeName(scope, projects)}`}
              onClick={toggle}
            >
              <span>{scopeName(scope, projects)}</span>
              <CaretDownIcon size={14} />
            </button>
          )}
        >
          {(close) =>
            scopes.map((s) => (
              <button
                key={s.type === "project" ? s.projectId : s.type}
                role="menuitemradio"
                aria-checked={sameScope(s, scope)}
                className={`${MENU_ITEM} justify-between ${sameScope(s, scope) ? "bg-selected-muted" : ""}`}
                onClick={() => {
                  close();
                  setScope(s);
                }}
              >
                <span>{scopeName(s, projects)}</span>
                <span className="text-fg-muted tabular-nums">
                  {scopeRules(permissions, s).rules.length || ""}
                </span>
              </button>
            ))
          }
        </Dropdown>
        {everywhere && (
          <button
            className="btn ml-auto"
            disabled={!restorable}
            title={restorable ? undefined : "The starting rules are all in place"}
            onClick={() => {
              if (defaults) changeRules((rules) => withDefaultRules(rules, defaults));
            }}
          >
            <ArrowCounterClockwiseIcon size={14} />
            Restore defaults
          </button>
        )}
      </div>

      <div className={HEADING}>Defaults</div>
      {PERMISSION_KINDS.map(({ kind, label }) => {
        const inherited = permissions.everywhere.defaults[kind] ?? "ask";
        const value = own.defaults[kind] ?? (everywhere ? "ask" : "inherit");
        const options = [
          ...(everywhere
            ? []
            : [{ value: "inherit", label: `Inherit · ${EFFECT_LABELS[inherited]}` }]),
          ...EFFECTS.map((e) => ({ value: e, label: EFFECT_LABELS[e] })),
        ];
        return (
          <TokenRow key={kind} title={label}>
            <Segmented
              label={label}
              value={value}
              options={options}
              onChange={(next) =>
                change((p) =>
                  withScopeRules(p, scope, (s) => {
                    const defaults = { ...s.defaults };
                    if (next === "inherit") delete defaults[kind];
                    else defaults[kind] = next as Effect;
                    return { ...s, defaults };
                  }),
                )
              }
            />
          </TokenRow>
        );
      })}

      <div className={HEADING}>Rules</div>
      {PERMISSION_KINDS.map(({ kind, label, placeholder }) => {
        const rules = own.rules.filter((r) => r.kind === kind);
        if (rules.length === 0) return null;
        return (
          <div key={kind} className="border-b border-border py-2">
            <div className="mb-1 font-semibold">{label}</div>
            {rules.map((rule) => (
              <RuleRow
                key={rule.id}
                rule={rule}
                placeholder={placeholder}
                picked={focus?.ruleId === rule.id && sameScope(focus.scope, scope)}
                moveTargets={scopes
                  .filter((s) => !sameScope(s, scope))
                  .map((s) => ({ scope: s, name: scopeName(s, projects) }))}
                onChange={(patch) => updateRule(rule.id, patch)}
                onMove={(to) => moveRule(rule, to)}
                onDelete={() => changeRules((rs) => rs.filter((r) => r.id !== rule.id))}
              />
            ))}
          </div>
        );
      })}
      <AddRule onAdd={(rule) => changeRules((rs) => [...rs, rule])} />

      <div className={HEADING}>Computer use</div>
      <TokenRow
        title="Let the agent use the screen"
        desc="Adds screenshot, click, type, key and scroll tools to Patchwork's own agent. Each use asks here unless a rule above allows the screen."
      >
        <TokenSwitch
          label="Computer use"
          on={settings.computerUse}
          onChange={(on) => void updateSettings((s) => ({ ...s, computerUse: on }))}
        />
      </TokenRow>
    </>
  );
}

function RuleRow({
  rule,
  placeholder,
  picked,
  moveTargets,
  onChange,
  onMove,
  onDelete,
}: {
  rule: PermissionRule;
  placeholder: string;
  picked: boolean;
  moveTargets: { scope: PermissionScope; name: string }[];
  onChange: (patch: Partial<PermissionRule>) => void;
  onMove: (to: PermissionScope) => void;
  onDelete: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (picked) ref.current?.scrollIntoView({ block: "center" });
  }, [picked]);
  return (
    <div
      ref={ref}
      className={`-mx-1.5 flex items-center gap-1.5 rounded-[var(--radius-sm)] px-1.5 py-0.5 ${picked ? "bg-selected-muted" : ""}`}
    >
      <TextField
        label={`${rule.kind} rule`}
        value={rule.pattern}
        placeholder={placeholder}
        width="min-w-0 flex-1 font-[family-name:var(--font-mono)]"
        onSave={(pattern) => pattern.trim() && onChange({ pattern: pattern.trim() })}
      />
      <Segmented
        label={`What ${rule.pattern} gets`}
        value={rule.effect}
        options={EFFECTS.map((e) => ({ value: e, label: EFFECT_LABELS[e] }))}
        onChange={(effect) => onChange({ effect })}
      />
      <Dropdown
        alignEnd
        trigger={({ open: isOpen, toggle }) => (
          <button
            className="btn-icon"
            aria-haspopup="menu"
            aria-expanded={isOpen}
            aria-label={`Move ${rule.pattern}`}
            title="Move to…"
            onClick={toggle}
          >
            <DotsThreeIcon size={14} />
          </button>
        )}
      >
        {(close) =>
          moveTargets.map(({ scope, name }) => (
            <button
              key={scope.type === "project" ? scope.projectId : scope.type}
              role="menuitem"
              className={MENU_ITEM}
              onClick={() => {
                close();
                onMove(scope);
              }}
            >
              Move to {name}
            </button>
          ))
        }
      </Dropdown>
      <button className="btn-icon" aria-label={`Delete ${rule.pattern}`} onClick={onDelete}>
        <XIcon size={14} />
      </button>
    </div>
  );
}

/** A row that adds an allow rule of any kind; folder rules can be picked with the folder dialog. */
function AddRule({ onAdd }: { onAdd: (rule: PermissionRule) => void }) {
  const [kind, setKind] = useState<PermissionKind>("folder");
  const [pattern, setPattern] = useState("");
  const current = PERMISSION_KINDS.find((k) => k.kind === kind) ?? PERMISSION_KINDS[0];
  const add = (value = pattern) => {
    if (!value.trim()) return;
    onAdd({ id: crypto.randomUUID(), kind, pattern: value.trim(), effect: "allow" });
    setPattern("");
  };
  const pickFolder = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") add(dir);
  };
  return (
    <div className="flex items-center gap-1.5 py-2.5">
      <Dropdown
        trigger={({ open: isOpen, toggle }) => (
          <button
            className="btn"
            aria-haspopup="menu"
            aria-expanded={isOpen}
            aria-label="Kind of rule"
            onClick={toggle}
          >
            {current.label}
            <CaretDownIcon size={12} />
          </button>
        )}
      >
        {(close) =>
          PERMISSION_KINDS.map((k) => (
            <button
              key={k.kind}
              role="menuitemradio"
              aria-checked={k.kind === kind}
              className={`${MENU_ITEM} ${k.kind === kind ? "bg-selected-muted" : ""}`}
              onClick={() => {
                close();
                setKind(k.kind);
              }}
            >
              {k.label}
            </button>
          ))
        }
      </Dropdown>
      <input
        className="input min-w-0 flex-1 font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)]"
        aria-label="Pattern"
        value={pattern}
        placeholder={current.placeholder}
        spellCheck={false}
        onChange={(e) => setPattern(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && add()}
      />
      {(kind === "folder" || kind === "edit") && (
        <button
          className="btn-icon"
          aria-label="Pick a folder"
          title="Pick a folder"
          onClick={() => void pickFolder()}
        >
          <FolderOpenIcon size={14} />
        </button>
      )}
      <button className="btn" disabled={!pattern.trim()} onClick={() => add()}>
        Add rule
      </button>
    </div>
  );
}
