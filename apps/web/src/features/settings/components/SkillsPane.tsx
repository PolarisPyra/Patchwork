import { useCallback, useEffect, useState } from "react";
import { FolderSimpleIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type SkillsView } from "~/api";
import { Badge, INPUT, chip, TokenRow, TokenSwitch } from "~/features/settings/components/controls";
import type { PaneProps } from "~/features/settings/types";
import { useSigils } from "~/lib/sigils";
import { errorText, tildify } from "~/lib/util";

/** How a skills folder is laid out: one folder per skill, named like the skill, with its
 * SKILL.md and anything the skill refers to. */
const SKILLS_EXAMPLE = `skills/
├── release-notes/
│   ├── SKILL.md
│   └── template.md
└── review-ui/
    ├── SKILL.md
    └── scripts/
        └── measure.sh

release-notes/SKILL.md
---
name: release-notes
description: Draft release notes from the commits since the last tag
---
Read template.md, then list the commits since the last tag…`;

type SkillFilter = "on" | "off";

/** The skills folder every agent loads, and the skills in it, kept up to date as it changes. */
export default function SkillsPane({ settings, updateSettings }: PaneProps) {
  const sigil = useSigils().skills;
  const [view, setView] = useState<SkillsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<SkillFilter | null>(null);
  const [query, setQuery] = useState("");

  // A folder that can't be watched still lists its skills.
  const watch = useCallback(
    () =>
      api.watchSkills(setView).then(setView, (e) => {
        setError(errorText(e));
        return api.listSkills().then(setView);
      }),
    [],
  );
  useEffect(() => {
    void watch();
    return () => void api.unwatchSkills();
  }, [watch]);

  async function choose(dir: string | null) {
    setError(null);
    await updateSettings((s) => ({ ...s, skillsDir: dir }));
    await watch();
  }

  async function toggle(name: string, on: boolean) {
    setView(
      (v) =>
        v && { ...v, skills: v.skills.map((s) => (s.name === name ? { ...s, enabled: on } : s)) },
    );
    await updateSettings((s) => ({
      ...s,
      disabledSkills: on
        ? s.disabledSkills.filter((n) => n !== name)
        : [...s.disabledSkills.filter((n) => n !== name), name],
    }));
  }

  async function pick() {
    const picked = await open({ directory: true, defaultPath: view?.dir });
    if (typeof picked === "string") await choose(picked);
  }

  if (view === null) return null;

  const run = (action: Promise<void>) => {
    setError(null);
    action.catch((e) => setError(errorText(e)));
  };
  const q = query.trim().toLowerCase();
  const on = view.skills.filter((s) => s.enabled).length;
  const counts = { on, off: view.skills.length - on };
  const shown = view.skills.filter(
    (s) =>
      (!filter || s.enabled === (filter === "on")) &&
      (!q || `${s.name} ${s.description ?? ""}`.toLowerCase().includes(q)),
  );

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <button
          className={chip(filter === null)}
          aria-pressed={filter === null}
          onClick={() => setFilter(null)}
        >
          All <span className="text-fg-muted">{view.skills.length}</span>
        </button>
        {(["on", "off"] as const).map((f) => (
          <button
            key={f}
            className={chip(filter === f)}
            aria-pressed={filter === f}
            onClick={() => setFilter(filter === f ? null : f)}
          >
            {f === "on" ? "On" : "Off"} <span className="text-fg-muted">{counts[f]}</span>
          </button>
        ))}
        <label className="relative ml-auto w-52">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Search skills"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button className="btn flex-none" onClick={() => run(api.openSkillsFolder())}>
          <FolderSimpleIcon size={13} />
          Open folder
        </button>
      </div>
      {shown.length > 0 && (
        <div className="divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
          {shown.map((skill) => (
            <div
              key={skill.path}
              className={`flex min-w-0 items-center gap-2.5 px-3 py-2 ${skill.enabled ? "" : "opacity-55"}`}
            >
              <Badge name={skill.name} size={24} />
              <div className="min-w-0 flex-1">
                <div
                  className="truncate font-semibold"
                  title={
                    skill.path.startsWith(`${view.dir}/`)
                      ? skill.path.slice(view.dir.length + 1)
                      : tildify(skill.path)
                  }
                >
                  {sigil}
                  {skill.name}
                </div>
                <div
                  className="truncate text-[length:var(--fs-sm)] text-fg-muted"
                  title={skill.description ?? undefined}
                >
                  {skill.description}
                </div>
              </div>
              <button className="btn flex-none" onClick={() => run(api.openSkill(skill.path))}>
                Edit
              </button>
              <TokenSwitch
                label={`${sigil}${skill.name}`}
                on={skill.enabled}
                onChange={(on) => void toggle(skill.name, on)}
              />
            </div>
          ))}
        </div>
      )}
      {shown.length === 0 && (
        <div className="py-6 text-center text-fg-muted">
          {q ? `Nothing matches “${query.trim()}”` : "No skills"}
        </div>
      )}
      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <div className="mt-3">
        <TokenRow title="Folder" desc={view.dir}>
          {settings.skillsDir && (
            <button className="btn" onClick={() => void choose(null)}>
              Use default
            </button>
          )}
          <button className="btn" onClick={() => void pick()}>
            Change…
          </button>
        </TokenRow>
      </div>
      <div className="mt-5 mb-1.5 font-semibold">Example</div>
      <pre className="m-0 overflow-x-auto rounded-[var(--radius-sm)] border border-border-strong bg-editor px-3 py-2.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] leading-relaxed text-fg-muted">
        {SKILLS_EXAMPLE}
      </pre>
    </>
  );
}
