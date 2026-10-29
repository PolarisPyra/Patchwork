import { XIcon } from "@phosphor-icons/react";
import { defaultTerminalSettings, type TerminalSettings } from "~/api";
import {
  NumberField,
  Segmented,
  TextField,
  TokenRow,
  TokenSwitch,
} from "~/features/settings/components/controls";
import type { PaneProps } from "~/features/settings/types";

const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);

/** The integrated terminal, with VS Code's `terminal.integrated.*` settings. The shell is the
 * Environment pane's. */
export default function TerminalPane({ settings, updateSettings }: PaneProps) {
  const t = settings.terminal;
  const set = (patch: Partial<TerminalSettings>) =>
    void updateSettings((s) => ({ ...s, terminal: { ...s.terminal, ...patch } }));
  const heading = (text: string) => (
    <h3 className="m-0 mt-4 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle first:mt-0">
      {text}
    </h3>
  );
  return (
    <>
      {heading("Shell")}
      <TokenRow title="Shell arguments">
        <TextField
          label="Shell arguments"
          value={t.shellArgs?.join(" ") ?? ""}
          placeholder={isMacPlatform ? "-l" : "None"}
          onSave={(text) => set({ shellArgs: text.trim() ? text.trim().split(/\s+/) : null })}
        />
      </TokenRow>
      <TokenRow title="Starting folder">
        <TextField
          label="Starting folder"
          value={t.cwd ?? ""}
          placeholder="Project folder"
          onSave={(text) => set({ cwd: text.trim() || null })}
        />
      </TokenRow>
      <TokenRow title="Inherit environment" desc="Start from Patchwork's environment">
        <TokenSwitch
          label="Inherit environment"
          on={t.inheritEnv}
          onChange={(on) => set({ inheritEnv: on })}
        />
      </TokenRow>
      <TokenRow title="UTF-8 locale" desc="Sets LANG; Auto only when it is not UTF-8">
        <Segmented
          label="UTF-8 locale"
          value={t.detectLocale}
          options={[
            { value: "auto", label: "Auto" },
            { value: "on", label: "On" },
            { value: "off", label: "Off" },
          ]}
          onChange={(detectLocale) => set({ detectLocale })}
        />
      </TokenRow>
      <EnvEditor env={t.env} onChange={(env) => set({ env })} />

      {heading("Appearance")}
      <TokenRow title="Font">
        <TextField
          label="Font"
          value={t.fontFamily ?? ""}
          placeholder="App monospace font"
          onSave={(text) => set({ fontFamily: text.trim() || null })}
        />
      </TokenRow>
      <TokenRow title="Font size">
        <NumberField
          label="Font size"
          value={t.fontSize}
          min={6}
          max={100}
          onSave={(fontSize) => set({ fontSize })}
        />
      </TokenRow>
      <TokenRow title="Line height">
        <NumberField
          label="Line height"
          value={t.lineHeight}
          step={0.1}
          min={1}
          onSave={(lineHeight) => set({ lineHeight })}
        />
      </TokenRow>
      <TokenRow title="Letter spacing">
        <NumberField
          label="Letter spacing"
          value={t.letterSpacing}
          step={0.5}
          onSave={(letterSpacing) => set({ letterSpacing })}
        />
      </TokenRow>
      <TokenRow title="Font weight">
        <TextField
          label="Font weight"
          value={t.fontWeight}
          width="w-[80px]"
          onSave={(text) => text.trim() && set({ fontWeight: text.trim() })}
        />
      </TokenRow>
      <TokenRow title="Bold font weight">
        <TextField
          label="Bold font weight"
          value={t.fontWeightBold}
          width="w-[80px]"
          onSave={(text) => text.trim() && set({ fontWeightBold: text.trim() })}
        />
      </TokenRow>
      <TokenRow title="Cursor">
        <Segmented
          label="Cursor style"
          value={t.cursorStyle}
          options={[
            { value: "block", label: "Block" },
            { value: "line", label: "Line" },
            { value: "underline", label: "Underline" },
          ]}
          onChange={(cursorStyle) => set({ cursorStyle })}
        />
      </TokenRow>
      {t.cursorStyle === "line" && (
        <TokenRow title="Cursor width">
          <NumberField
            label="Cursor width"
            value={t.cursorWidth}
            min={1}
            onSave={(cursorWidth) => set({ cursorWidth })}
          />
        </TokenRow>
      )}
      <TokenRow title="Cursor blinking">
        <TokenSwitch
          label="Cursor blinking"
          on={t.cursorBlinking}
          onChange={(cursorBlinking) => set({ cursorBlinking })}
        />
      </TokenRow>
      <TokenRow title="Minimum contrast ratio">
        <NumberField
          label="Minimum contrast ratio"
          value={t.minimumContrastRatio}
          step={0.5}
          min={1}
          max={21}
          onSave={(minimumContrastRatio) => set({ minimumContrastRatio })}
        />
      </TokenRow>
      <TokenRow title="Bold text in bright colors">
        <TokenSwitch
          label="Bold text in bright colors"
          on={t.drawBoldTextInBrightColors}
          onChange={(drawBoldTextInBrightColors) => set({ drawBoldTextInBrightColors })}
        />
      </TokenRow>

      {heading("Behavior")}
      <TokenRow title="Scrollback" desc="Lines kept above the screen">
        <NumberField
          label="Scrollback"
          value={t.scrollback}
          min={0}
          onSave={(scrollback) => set({ scrollback })}
        />
      </TokenRow>
      <TokenRow title="Copy on selection">
        <TokenSwitch
          label="Copy on selection"
          on={t.copyOnSelection}
          onChange={(copyOnSelection) => set({ copyOnSelection })}
        />
      </TokenRow>
      <TokenRow title="Right click">
        <Segmented
          label="Right click"
          value={t.rightClickBehavior}
          options={[
            { value: "default", label: "Menu" },
            { value: "copyPaste", label: "Copy or paste" },
            { value: "paste", label: "Paste" },
            { value: "selectWord", label: "Select word" },
            { value: "nothing", label: "Nothing" },
          ]}
          onChange={(rightClickBehavior) => set({ rightClickBehavior })}
        />
      </TokenRow>
      <TokenRow title="Alt-click moves the cursor">
        <TokenSwitch
          label="Alt-click moves the cursor"
          on={t.altClickMovesCursor}
          onChange={(altClickMovesCursor) => set({ altClickMovesCursor })}
        />
      </TokenRow>
      {isMacPlatform && (
        <TokenRow title="Option is Meta">
          <TokenSwitch
            label="Option is Meta"
            on={t.macOptionIsMeta}
            onChange={(macOptionIsMeta) => set({ macOptionIsMeta })}
          />
        </TokenRow>
      )}
      <TokenRow title="Smooth scrolling">
        <TokenSwitch
          label="Smooth scrolling"
          on={t.smoothScrolling}
          onChange={(smoothScrolling) => set({ smoothScrolling })}
        />
      </TokenRow>
      <TokenRow title="Scroll speed">
        <NumberField
          label="Scroll speed"
          value={t.mouseWheelScrollSensitivity}
          step={0.5}
          min={0.1}
          onSave={(mouseWheelScrollSensitivity) => set({ mouseWheelScrollSensitivity })}
        />
      </TokenRow>
      <TokenRow title="Fast scroll speed" desc="While Alt is held">
        <NumberField
          label="Fast scroll speed"
          value={t.fastScrollSensitivity}
          step={0.5}
          min={0.1}
          onSave={(fastScrollSensitivity) => set({ fastScrollSensitivity })}
        />
      </TokenRow>
      <div className="mt-4">
        <button className="btn" onClick={() => set(defaultTerminalSettings())}>
          Reset to defaults
        </button>
      </div>
    </>
  );
}

/** The terminal's environment variables. A removed variable is taken out of the shell's
 * environment, as `null` in VS Code's `terminal.integrated.env`. */
function EnvEditor({
  env,
  onChange,
}: {
  env: Record<string, string | null>;
  onChange: (env: Record<string, string | null>) => void;
}) {
  const entries = Object.entries(env);
  const replace = (index: number, name: string, value: string | null) =>
    onChange(
      Object.fromEntries(
        entries.map((entry, i) => (i === index ? [name, value] : entry)).filter(([n]) => n),
      ),
    );
  return (
    <div className="border-b border-border py-2.5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="font-semibold">Environment variables</div>
          <div className="text-[length:var(--fs-sm)] text-fg-muted">
            {"Values can use ${env:NAME} and ${workspaceFolder}"}
          </div>
        </div>
        <button
          className="btn"
          onClick={() => {
            let name = "NAME";
            for (let n = 2; name in env; n++) name = `NAME_${n}`;
            onChange({ ...env, [name]: "" });
          }}
        >
          Add
        </button>
      </div>
      {entries.map(([name, value], i) => (
        <div key={i} className="mt-1.5 flex items-center gap-1.5">
          <TextField
            label="Variable name"
            value={name}
            width="w-[160px]"
            onSave={(next) => replace(i, next.trim(), value)}
          />
          <TextField
            label={`${name} value`}
            value={value ?? ""}
            width="min-w-0 flex-1"
            placeholder={value === null ? "Removed from the shell" : ""}
            onSave={(next) => replace(i, name, next)}
          />
          <button
            className={`btn ${value === null ? "bg-selected-muted" : ""}`}
            aria-pressed={value === null}
            title="Take this variable out of the shell's environment"
            onClick={() => replace(i, name, value === null ? "" : null)}
          >
            Remove from shell
          </button>
          <button
            className="btn-icon"
            aria-label={`Delete ${name}`}
            onClick={() => onChange(Object.fromEntries(entries.filter((_, j) => j !== i)))}
          >
            <XIcon size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
