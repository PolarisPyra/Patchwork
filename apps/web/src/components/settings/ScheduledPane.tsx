import { useEffect, useState } from "react";
import {
  ClockIcon,
  FolderSimpleIcon,
  MagnifyingGlassIcon,
  PlayIcon,
  PlusIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  AGENT_LABELS,
  api,
  type AgentKind,
  type AccessMode,
  type ScheduledTask,
  type Settings,
  type Weekday,
} from "~/api";
import type { UpdateSettings } from "~/app/App";
import { INPUT, chip } from "~/components/settings/McpServersPane";
import { TokenRow, TokenSwitch } from "~/components/settings/SettingsDialog";
import { AGENT_KINDS, errorText } from "~/lib/util";

/**
 * Settings → Scheduled: prompts that start a task at a fixed time, in a project or in a chat.
 * The app runs them on a timer of its own while it is open.
 */

const MODES: { mode: AccessMode; label: string }[] = [
  { mode: "supervised", label: "Supervised" },
  { mode: "auto-accept-edits", label: "Auto-accept edits" },
  { mode: "auto", label: "Auto" },
  { mode: "full-access", label: "Full access" },
];

const DAYS: { day: Weekday; label: string }[] = [
  { day: "MO", label: "Mon" },
  { day: "TU", label: "Tue" },
  { day: "WE", label: "Wed" },
  { day: "TH", label: "Thu" },
  { day: "FR", label: "Fri" },
  { day: "SA", label: "Sat" },
  { day: "SU", label: "Sun" },
];

const SELECT =
  "h-7 rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2 text-[length:var(--fs-sm)] text-fg";
const TEXTAREA = `${INPUT} h-auto min-h-[70px] w-full resize-y py-1.5`;

/** What a schedule says in a list row, e.g. `Weekdays at 09:00`. */
export function describeSchedule(task: ScheduledTask): string {
  const schedule = task.schedule;
  switch (schedule.type) {
    case "hourly":
      return schedule.intervalHours === 1 ? "Every hour" : `Every ${schedule.intervalHours} hours`;
    case "daily":
      return `Daily at ${schedule.time}`;
    case "weekdays":
      return `Weekdays at ${schedule.time}`;
    case "weekly": {
      const names = schedule.days
        .map((day) => DAYS.find((d) => d.day === day)?.label ?? day)
        .join(", ");
      return `${names || "Every day"} at ${schedule.time}`;
    }
  }
}

/** When a task last and next runs, in the reader's own time. */
function when(at: number | null): string {
  if (at === null) return "not yet";
  return new Date(at).toLocaleString(undefined, {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** A blank task, with the settings a new one starts from. */
function blank(agent: AgentKind): ScheduledTask {
  return {
    id: "",
    name: "",
    prompt: "",
    schedule: { type: "daily", time: "09:00" },
    projectId: null,
    agent,
    model: null,
    accessMode: "supervised",
    enabled: true,
    lastRunAt: null,
    nextRunAt: null,
    createdAt: Date.now(),
  };
}

export default function ScheduledPane({
  settings,
}: {
  settings: Settings;
  updateSettings: UpdateSettings;
}) {
  const [tasks, setTasks] = useState<ScheduledTask[] | null>(null);
  const [editing, setEditing] = useState<ScheduledTask | null>(null);
  const [error, setError] = useState<string | null>(null);

  const defaultAgent = settings.agents[0] ?? "claude";

  useEffect(() => {
    api
      .listScheduledTasks()
      .then(setTasks)
      .catch((e: unknown) => setError(errorText(e)));
  }, []);

  function run(action: () => Promise<unknown>) {
    setError(null);
    action().catch((e: unknown) => setError(errorText(e)));
  }

  async function save(task: ScheduledTask) {
    const saved = await api.saveScheduledTask(task);
    setTasks((list) => {
      const next = list ? list.filter((t) => t.id !== saved.id) : [];
      return [...next, saved].sort((a, b) => a.createdAt - b.createdAt);
    });
    setEditing(null);
  }

  async function toggle(task: ScheduledTask, on: boolean) {
    const saved = await api.saveScheduledTask({ ...task, enabled: on });
    setTasks((list) => list?.map((t) => (t.id === saved.id ? saved : t)) ?? null);
  }

  async function remove(task: ScheduledTask) {
    await api.removeScheduledTask(task.id);
    setTasks((list) => list?.filter((t) => t.id !== task.id) ?? null);
    if (editing?.id === task.id) setEditing(null);
  }

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <button className="btn" onClick={() => setEditing(blank(defaultAgent))}>
          <PlusIcon size={13} />
          New task
        </button>
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          Tasks run while Patchwork is open.
        </span>
      </div>

      {tasks && tasks.length > 0 && (
        <div className="divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
          {tasks.map((task) => (
            <div
              key={task.id}
              className={`flex min-w-0 items-center gap-2.5 px-3 py-2 ${task.enabled ? "" : "opacity-55"}`}
            >
              <ClockIcon size={16} className="flex-none text-fg-muted" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-semibold">{task.name}</div>
                <div className="truncate text-[length:var(--fs-sm)] text-fg-muted">
                  {describeSchedule(task)} · next {when(task.nextRunAt)}
                  {task.projectId
                    ? ` · ${settings.projects.find((p) => p.id === task.projectId)?.name ?? "project"}`
                    : " · chat"}
                </div>
              </div>
              <button
                className="btn flex-none"
                title="Start this task now"
                onClick={() => run(() => api.runScheduledTask(task.id))}
              >
                <PlayIcon size={13} />
                Run
              </button>
              <button className="btn flex-none" onClick={() => setEditing(task)}>
                Edit
              </button>
              <TokenSwitch
                label={`${task.name} runs on its schedule`}
                on={task.enabled}
                onChange={(on) => run(() => toggle(task, on))}
              />
              <button
                className="btn flex-none"
                aria-label={`Delete ${task.name}`}
                onClick={() => run(() => remove(task))}
              >
                <TrashIcon size={13} />
              </button>
            </div>
          ))}
        </div>
      )}

      {tasks && tasks.length === 0 && !editing && (
        <div className="py-6 text-center text-fg-muted">
          No scheduled tasks. Add one to have an agent start a task on its own.
        </div>
      )}

      {editing && (
        <TaskEditor
          task={editing}
          settings={settings}
          onCancel={() => setEditing(null)}
          onSave={(task) => run(() => save(task))}
        />
      )}

      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}

      <div className="mt-3">
        <TokenRow
          title="Scheduled tasks"
          desc="A task starts in a new task of its own, with the prompt as its first message."
        >
          <button
            className="btn"
            onClick={() => run(async () => setTasks(await api.listScheduledTasks()))}
          >
            Refresh
          </button>
        </TokenRow>
      </div>
    </>
  );
}

/** The form for one task. */
function TaskEditor({
  task,
  settings,
  onCancel,
  onSave,
}: {
  task: ScheduledTask;
  settings: Settings;
  onCancel: () => void;
  onSave: (task: ScheduledTask) => void;
}) {
  const [draft, setDraft] = useState(task);
  const [query, setQuery] = useState("");
  const patch = (fields: Partial<ScheduledTask>) => setDraft((d) => ({ ...d, ...fields }));
  const schedule = draft.schedule;
  const at = "time" in schedule ? schedule.time : "09:00";

  function setSchedule(next: ScheduledTask["schedule"]) {
    patch({ schedule: next });
  }

  function setKind(kind: ScheduledTask["schedule"]["type"]) {
    if (kind === "hourly") setSchedule({ type: "hourly", intervalHours: 6 });
    else if (kind === "daily") setSchedule({ type: "daily", time: at });
    else if (kind === "weekdays") setSchedule({ type: "weekdays", time: at });
    else setSchedule({ type: "weekly", days: ["MO"], time: at });
  }

  function toggleDay(day: Weekday) {
    if (schedule.type !== "weekly") return;
    const days = schedule.days.includes(day)
      ? schedule.days.filter((d) => d !== day)
      : [...schedule.days, day];
    setSchedule({ type: "weekly", days, time: schedule.time });
  }

  const ready = draft.name.trim() && draft.prompt.trim();
  const projects = settings.projects.filter((p) =>
    p.name.toLowerCase().includes(query.trim().toLowerCase()),
  );

  return (
    <div className="mt-3 rounded-[var(--radius-md)] border border-border-strong bg-panel p-3">
      <div className="mb-2 flex items-center gap-2">
        <input
          className={`${INPUT} w-56`}
          placeholder="Name, such as Weekly report"
          value={draft.name}
          spellCheck={false}
          onChange={(e) => patch({ name: e.target.value })}
        />
        <label className="ml-auto flex items-center gap-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          Enabled
          <TokenSwitch
            label="Task is enabled"
            on={draft.enabled}
            onChange={(enabled) => patch({ enabled })}
          />
        </label>
      </div>
      <textarea
        className={TEXTAREA}
        placeholder="The prompt the task starts with."
        value={draft.prompt}
        spellCheck
        onChange={(e) => patch({ prompt: e.target.value })}
      />

      <div className="mt-2 flex flex-wrap items-center gap-2">
        <select
          aria-label="How often"
          className={SELECT}
          value={schedule.type}
          onChange={(e) => setKind(e.target.value as ScheduledTask["schedule"]["type"])}
        >
          <option value="hourly">Every few hours</option>
          <option value="daily">Every day</option>
          <option value="weekdays">Weekdays</option>
          <option value="weekly">Days of the week</option>
        </select>
        {schedule.type === "hourly" ? (
          <label className="flex items-center gap-1.5 text-[length:var(--fs-sm)] text-fg-muted">
            every
            <input
              aria-label="Hours between runs"
              className={`${INPUT} w-14`}
              type="number"
              min={1}
              max={168}
              value={schedule.intervalHours}
              onChange={(e) =>
                setSchedule({
                  type: "hourly",
                  intervalHours: Math.min(168, Math.max(1, Number(e.target.value) || 1)),
                })
              }
            />
            hours
          </label>
        ) : (
          <input
            aria-label="Time of day"
            className={`${INPUT} w-24`}
            type="time"
            value={at}
            onChange={(e) => {
              const time = e.target.value || "09:00";
              if (schedule.type === "daily") setSchedule({ type: "daily", time });
              else if (schedule.type === "weekdays") setSchedule({ type: "weekdays", time });
              else if (schedule.type === "weekly")
                setSchedule({ type: "weekly", days: schedule.days, time });
            }}
          />
        )}
        {schedule.type === "weekly" && (
          <div className="flex items-center gap-0.5">
            {DAYS.map((day) => (
              <button
                key={day.day}
                className={chip(schedule.days.includes(day.day))}
                aria-pressed={schedule.days.includes(day.day)}
                onClick={() => toggleDay(day.day)}
              >
                {day.label}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-2">
        <select
          aria-label="Project"
          className={`${SELECT} max-w-[220px]`}
          value={draft.projectId ?? ""}
          onChange={(e) => patch({ projectId: e.target.value || null })}
        >
          <option value="">A chat in a scratch folder</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {project.name}
            </option>
          ))}
        </select>
        <select
          aria-label="Agent"
          className={SELECT}
          value={draft.agent}
          onChange={(e) => patch({ agent: e.target.value as AgentKind, model: null })}
        >
          {AGENT_KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {AGENT_LABELS[kind]}
            </option>
          ))}
        </select>
        <select
          aria-label="Access mode"
          className={SELECT}
          value={draft.accessMode}
          onChange={(e) => patch({ accessMode: e.target.value as AccessMode })}
        >
          {MODES.map((mode) => (
            <option key={mode.mode} value={mode.mode}>
              {mode.label}
            </option>
          ))}
        </select>
        <span className="ml-auto flex items-center gap-1.5">
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!ready} onClick={() => onSave(draft)}>
            Save task
          </button>
        </span>
      </div>
      {settings.projects.length > 4 && (
        <label className="relative mt-2 flex w-56 items-center">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute left-2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Find a project"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
      )}
      {draft.projectId && (
        <div className="mt-1.5 flex items-center gap-1.5 text-[length:var(--fs-sm)] text-fg-subtle">
          <FolderSimpleIcon size={12} />
          {settings.projects.find((p) => p.id === draft.projectId)?.path}
        </div>
      )}
    </div>
  );
}
