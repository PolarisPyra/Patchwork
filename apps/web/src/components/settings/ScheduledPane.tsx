import { useEffect, useState, type ReactNode } from "react";
import {
  ArrowClockwiseIcon,
  CalendarBlankIcon,
  ChatCircleIcon,
  ClockIcon,
  FolderSimpleIcon,
  MagnifyingGlassIcon,
  PencilSimpleIcon,
  PlayIcon,
  PlusIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  AGENT_LABELS,
  api,
  type AgentKind,
  type AccessMode,
  type ScheduledRun,
  type ScheduledTask,
  type Settings,
  type Weekday,
} from "~/api";
import type { UpdateSettings } from "~/app/App";
import { INPUT, chip } from "~/components/settings/McpServersPane";
import { TokenSwitch } from "~/components/settings/SettingsDialog";
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
const RELATIVE_DAY = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
const DAY_MS = 86_400_000;

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

/** The local midnight that starts the day of a Unix-millisecond time. */
function startOfDay(at: number): number {
  const d = new Date(at);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** When a task last or next runs, in the reader's own time: `today 09:00`, `tomorrow 09:00`, `Tue 09:00`. */
function whenLabel(at: number | null): string {
  if (at === null) return "not yet";
  const days = Math.round((startOfDay(at) - startOfDay(Date.now())) / DAY_MS);
  const day =
    Math.abs(days) <= 1
      ? RELATIVE_DAY.format(days, "day")
      : new Date(at).toLocaleDateString(undefined, { weekday: "short" });
  const time = new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return `${day} ${time}`;
}

const OUTCOME: Record<ScheduledRun["outcome"], { verb: string; dot: string }> = {
  started: { verb: "Ran", dot: "bg-accent" },
  failed: { verb: "Failed", dot: "bg-danger" },
  skipped: { verb: "Skipped", dot: "bg-fg-subtle" },
};

/** A task's newest runs as dots, oldest to newest, then the newest one spelled out. */
function RunHistory({ runs }: { runs: ScheduledRun[] }) {
  const newest = runs[0];
  if (!newest) {
    return <div>Has not run yet</div>;
  }
  const title = runs
    .map((r) =>
      [whenLabel(r.at), OUTCOME[r.outcome].verb.toLowerCase(), r.detail]
        .filter(Boolean)
        .join(" · "),
    )
    .join("\n");
  return (
    <div className="flex min-w-0 items-center gap-1.5" title={title}>
      <span className="flex flex-none items-center gap-0.5">
        {[...runs].reverse().map((r, i) => (
          <span key={i} className={`size-1.5 rounded-full ${OUTCOME[r.outcome].dot}`} />
        ))}
      </span>
      <span className="truncate">
        {OUTCOME[newest.outcome].verb} {whenLabel(newest.at)} · {runs.length} recent{" "}
        {runs.length === 1 ? "run" : "runs"}
      </span>
    </div>
  );
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
    recentRuns: [],
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
      <div className="flex items-center gap-2 pb-3">
        <button className="btn" onClick={() => setEditing(blank(defaultAgent))}>
          <PlusIcon size={13} />
          New task
        </button>
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          Tasks run while Patchwork is open.
        </span>
        <button
          className="btn ml-auto"
          onClick={() => run(async () => setTasks(await api.listScheduledTasks()))}
        >
          <ArrowClockwiseIcon size={13} />
          Refresh
        </button>
      </div>

      {tasks && tasks.length > 0 && (
        <div className="flex flex-col gap-2">
          {tasks.map((task) => {
            const place = task.projectId
              ? (settings.projects.find((p) => p.id === task.projectId)?.name ?? "Project")
              : "Scratch chat";
            return (
              <div
                key={task.id}
                className="flex min-w-0 items-center gap-3 rounded-[var(--radius-md)] border border-border bg-panel px-3 py-2.5"
              >
                <span
                  className={`flex size-8 flex-none items-center justify-center rounded-full ${task.enabled ? "bg-accent-soft text-accent" : "bg-hover text-fg-subtle"}`}
                >
                  <ClockIcon size={16} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="truncate font-semibold">{task.name}</span>
                    {!task.enabled && (
                      <span className="flex-none rounded-full bg-hover px-2 text-[length:var(--fs-xs)] text-fg-muted">
                        Paused
                      </span>
                    )}
                  </div>
                  <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5 text-[length:var(--fs-sm)] text-fg-muted">
                    <span className="flex items-center gap-1">
                      <CalendarBlankIcon size={12} />
                      {describeSchedule(task)}
                    </span>
                    <span className="flex min-w-0 items-center gap-1">
                      {task.projectId ? (
                        <FolderSimpleIcon size={12} />
                      ) : (
                        <ChatCircleIcon size={12} />
                      )}
                      <span className="truncate">{place}</span>
                    </span>
                    <span>{AGENT_LABELS[task.agent]}</span>
                  </div>
                  <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5 text-[length:var(--fs-xs)] text-fg-subtle">
                    <RunHistory runs={task.recentRuns} />
                    {task.enabled && (
                      <span className="flex-none">Next {whenLabel(task.nextRunAt)}</span>
                    )}
                  </div>
                </div>
                <div className="flex flex-none items-center gap-1.5">
                  <button
                    className="btn"
                    title="Start this task now"
                    onClick={() =>
                      run(async () => {
                        await api.runScheduledTask(task.id);
                        setTasks(await api.listScheduledTasks());
                      })
                    }
                  >
                    <PlayIcon size={13} />
                    Run
                  </button>
                  <button className="btn" onClick={() => setEditing(task)}>
                    <PencilSimpleIcon size={13} />
                    Edit
                  </button>
                  <TokenSwitch
                    label={`${task.name} runs on its schedule`}
                    on={task.enabled}
                    onChange={(on) => run(() => toggle(task, on))}
                  />
                  <button
                    className="btn-icon"
                    title="Delete task"
                    aria-label={`Delete ${task.name}`}
                    onClick={() => run(() => remove(task))}
                  >
                    <TrashIcon size={14} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {tasks && tasks.length === 0 && !editing && (
        <div className="flex flex-col items-center gap-2 rounded-[var(--radius-md)] border border-dashed border-border-strong py-8 text-center">
          <span className="flex size-10 items-center justify-center rounded-full bg-accent-soft text-accent">
            <ClockIcon size={20} />
          </span>
          <div className="font-semibold">No scheduled tasks yet</div>
          <div className="text-[length:var(--fs-sm)] text-fg-muted">
            Each run opens a new task that starts with your prompt.
          </div>
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
    </>
  );
}

/** A form field: a small label above its control. */
function Field({
  label,
  className = "",
  children,
}: {
  label: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`flex min-w-0 flex-col gap-1 ${className}`}>
      <span className="text-[length:var(--fs-xs)] font-medium text-fg-muted">{label}</span>
      {children}
    </div>
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
    <div className="mt-3 flex flex-col gap-3 rounded-[var(--radius-md)] border border-border-strong bg-panel p-3.5">
      <Field label="Name">
        <input
          aria-label="Name"
          className={INPUT}
          placeholder="Weekly report"
          value={draft.name}
          spellCheck={false}
          onChange={(e) => patch({ name: e.target.value })}
        />
      </Field>
      <Field label="Prompt">
        <textarea
          aria-label="Prompt"
          className={TEXTAREA}
          placeholder="The prompt the task starts with."
          value={draft.prompt}
          spellCheck
          onChange={(e) => patch({ prompt: e.target.value })}
        />
      </Field>

      <div className="flex flex-wrap items-end gap-3">
        <Field label="Repeats">
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
        </Field>
        {schedule.type === "hourly" ? (
          <Field label="Every">
            <span className="flex items-center gap-1.5 text-[length:var(--fs-sm)] text-fg-muted">
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
            </span>
          </Field>
        ) : (
          <Field label="At">
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
          </Field>
        )}
        {schedule.type === "weekly" && (
          <Field label="On">
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
          </Field>
        )}
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <Field label="Where" className="min-w-[200px] flex-1">
          {settings.projects.length > 4 && (
            <label className="relative flex items-center">
              <MagnifyingGlassIcon
                size={13}
                className="pointer-events-none absolute left-2 text-fg-muted"
              />
              <input
                aria-label="Find a project"
                className={`${INPUT} pl-7`}
                value={query}
                placeholder="Find a project"
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
          )}
          <select
            aria-label="Project"
            className={`${SELECT} w-full`}
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
        </Field>
        <Field label="Agent">
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
        </Field>
        <Field label="Access">
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
        </Field>
      </div>
      {draft.projectId && (
        <div className="-mt-1 flex items-center gap-1.5 text-[length:var(--fs-xs)] text-fg-subtle">
          <FolderSimpleIcon size={12} />
          {settings.projects.find((p) => p.id === draft.projectId)?.path}
        </div>
      )}

      <div className="flex items-center gap-2 border-t border-border pt-3">
        <label className="flex items-center gap-2 text-[length:var(--fs-sm)] text-fg-muted">
          Enabled
          <TokenSwitch
            label="Task is enabled"
            on={draft.enabled}
            onChange={(enabled) => patch({ enabled })}
          />
        </label>
        <span className="ml-auto flex items-center gap-1.5">
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="btn btn-primary"
            disabled={!ready}
            title={ready ? undefined : "Add a name and a prompt"}
            onClick={() => onSave(draft)}
          >
            Save task
          </button>
        </span>
      </div>
    </div>
  );
}
