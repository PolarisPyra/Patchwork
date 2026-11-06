import { useEffect, useState } from "react";
import { api, AGENT_LABELS, type AgentKind, type ModelOption, type TokenSettings } from "~/api";
import { COMPACT_LEVELS, COMPACT_SETTINGS } from "~/components/CompactPicker";
import { Segmented, TokenRow, TokenSwitch } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";
import { formatTokens } from "~/lib/tokens";
import { AGENT_KINDS } from "~/lib/util";

/** Context sizes offered by the token settings. */
const TOKEN_LEVELS = [100_000, 150_000, 200_000, 300_000];

/** Context sizes to compact at; 0 is Default. */
const COMPACT_WINDOWS = [0, ...COMPACT_LEVELS];

/** A context size picked from a few, with 0 read as the given word. */
function ContextLevels({
  label,
  value,
  onChange,
  levels = TOKEN_LEVELS,
  zero = "Default",
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  levels?: number[];
  /** What 0 is called. */
  zero?: string;
}) {
  return (
    <Segmented
      label={label}
      value={String(value)}
      options={levels.map((level) => ({
        value: String(level),
        label: level === 0 ? zero : formatTokens(level),
      }))}
      onChange={(next) => onChange(Number(next))}
    />
  );
}

/** A model picked from one agent's models. The empty choice is `empty`, such as the CLI's own default. */
function DefaultModel({
  kind,
  label,
  empty,
  value,
  onChange,
}: {
  kind: AgentKind;
  label: string;
  empty: string;
  value: string | undefined;
  onChange: (id: string | undefined) => void;
}) {
  const [models, setModels] = useState<ModelOption[]>([]);
  useEffect(() => {
    let cancelled = false;
    api.listModels(kind).then(
      (list) => {
        if (!cancelled) setModels(list);
      },
      () => {
        if (!cancelled) setModels([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [kind]);
  return (
    <select
      aria-label={label}
      className="h-7 w-[200px] rounded-[var(--radius-sm)] border border-border-strong bg-editor pl-2 pr-6 text-[length:var(--fs-sm)] text-fg"
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || undefined)}
    >
      <option value="">{empty}</option>
      {models.flatMap((m) =>
        m.id
          ? [
              <option key={m.id} value={m.id}>
                {m.label}
              </option>,
            ]
          : [],
      )}
    </select>
  );
}

/** Ways to spend fewer tokens: what is shown, when to flag or compact a long context, and which
 * model new tasks start with. */
export default function TokensPane({ settings, updateSettings }: PaneProps) {
  const tokens = settings.tokens;
  const update = (patch: Partial<TokenSettings>) =>
    void updateSettings((s) => ({ ...s, tokens: { ...s.tokens, ...patch } }));
  const setDefaultModel = (kind: AgentKind, id: string | undefined) => {
    const next = { ...tokens.defaultModels };
    if (id) next[kind] = id;
    else delete next[kind];
    update({ defaultModels: next });
  };
  const connected = AGENT_KINDS.filter((k) => settings.agents.includes(k));
  return (
    <>
      <TokenRow
        title="Show token counts"
        desc="Context, cache reads and writes, input and output under each finished reply"
      >
        <TokenSwitch
          label="Show token counts"
          on={tokens.showUsage}
          onChange={(showUsage) => update({ showUsage })}
        />
      </TokenRow>
      <TokenRow
        title="Warn above"
        desc="Shows an above-threshold context warning below Worked for and above the divider"
      >
        <ContextLevels
          label="Warn above"
          value={tokens.warnAbove}
          onChange={(warnAbove) => update({ warnAbove })}
        />
      </TokenRow>
      <TokenRow
        title="Ask before resuming a large task"
        desc="When its prompt cache has expired, so resuming would re-read the whole context"
      >
        <TokenSwitch
          label="Ask before resuming a large task"
          on={tokens.askBeforeResume}
          onChange={(askBeforeResume) => update({ askBeforeResume })}
        />
      </TokenRow>
      <TokenRow title="Generate chat and task titles">
        <TokenSwitch
          label="Generate chat and task titles"
          on={tokens.generateTitles}
          onChange={(generateTitles) => update({ generateTitles })}
        />
      </TokenRow>
      {connected.map((kind) => {
        const key = COMPACT_SETTINGS[kind];
        const value = tokens[key];
        return (
          <TokenRow
            key={`compact-${kind}`}
            title={`${AGENT_LABELS[kind]} compacts at`}
            desc="Compacts a task once its context reaches this. Default leaves it to the agent, near the model's full window"
          >
            <ContextLevels
              label={`${AGENT_LABELS[kind]} compacts at`}
              value={value}
              // A size saved before these levels stays shown, so the row still marks it.
              levels={
                COMPACT_WINDOWS.includes(value)
                  ? COMPACT_WINDOWS
                  : [...COMPACT_WINDOWS, value].sort((a, b) => a - b)
              }
              zero="Default"
              onChange={(window) => update({ [key]: window })}
            />
          </TokenRow>
        );
      })}
      {connected.map((kind) => (
        <TokenRow
          key={kind}
          title={`${AGENT_LABELS[kind]} model`}
          desc="Used by new tasks. A smaller model spends fewer tokens on each turn"
        >
          <DefaultModel
            kind={kind}
            label={`${AGENT_LABELS[kind]} model for new tasks`}
            empty="Default"
            value={tokens.defaultModels[kind]}
            onChange={(id) => setDefaultModel(kind, id)}
          />
        </TokenRow>
      ))}
    </>
  );
}
