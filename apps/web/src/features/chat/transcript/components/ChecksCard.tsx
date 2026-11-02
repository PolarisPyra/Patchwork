import { createContext, useContext, useState } from "react";
import {
  CaretDownIcon,
  CheckCircleIcon,
  ClockIcon,
  MinusCircleIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import { api, type CheckSettings, type CheckStatus, type TurnChecks } from "~/api";
import Spinner from "~/components/Spinner";
import { errorText, fullElapsed } from "~/lib/util";

/** Settings → Checks as the app holds it, so the card's switch saves through the same settings
 * and the next save of any other setting keeps it. */
export interface ChecksContextValue {
  settings: CheckSettings;
  setProject: (projectId: string, on: boolean) => void;
}

export const ChecksContext = createContext<ChecksContextValue | null>(null);

/**
 * What the project's own checks made of a reply's turn, under the files it changed.
 *
 * Shown only for a reply whose checks ran, or that the person asked to check: a project with its
 * checks off leaves no card at all, and Settings → Checks is where they are turned on. The footer
 * turns them on or off for the project's next replies, through `ChecksContext`.
 *
 * "Passed" is only ever a command that exited zero having run something: one stopped at its time
 * limit says the result is unknown, and one that ran no tests says nothing was checked.
 */
interface Props {
  threadId: string;
  projectId: string;
  messageId: string;
  checks: TurnChecks;
}

export function ChecksCard({ threadId, projectId, messageId, checks }: Props) {
  const { status, command, durationMs, tests, failures, output, note } = checks;
  const [open, setOpen] = useState(false);
  const [rerunning, setRerunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ctx = useContext(ChecksContext);

  const running = status === "running";
  const autoOn = ctx
    ? (ctx.settings.projects[projectId] ?? ctx.settings.afterEveryTask)
    : checks.enabled;
  const { title, tone } = say(status, failures.length, durationMs, tests);

  function rerun() {
    setError(null);
    setRerunning(true);
    // The result arrives as a `checksRecorded` event, which replaces this card's data.
    api
      .runChecks(threadId, messageId)
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setRerunning(false));
    setOpen(true);
  }

  function toggleAuto() {
    ctx?.setProject(projectId, !autoOn);
  }

  return (
    // The same frame as the change summary above it: same padding, same row, and its button in
    // the same corner, so the two read as one stack rather than two kinds of card.
    <div
      className="flex flex-col gap-1 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 text-[length:var(--fs-sm)]"
      role="status"
      aria-live="polite"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {/* Everything but the button opens the details, so the chevron sits with the words it
            belongs to and the button stays where "Show changes" is. */}
        <button
          className="flex min-w-0 cursor-pointer items-center gap-x-2 border-none bg-transparent p-0 text-left"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          title={open ? "Hide details" : "Show details"}
        >
          <StatusIcon status={status} />
          <span className={`flex-none whitespace-nowrap ${tone}`}>{title}</span>
          {command && (
            <code
              className="min-w-0 truncate font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted select-text"
              title={command}
            >
              {command}
            </code>
          )}
          <CaretDownIcon
            size={12}
            className="flex-none text-fg-subtle transition-transform"
            style={{ transform: open ? undefined : "rotate(-90deg)" }}
          />
        </button>
        <span className="flex-1" />
        {!running && (
          <button
            className="btn flex-none"
            onClick={rerun}
            disabled={rerunning}
            title={command ? `Run ${command} in the project` : "Run the project's checks"}
          >
            {rerunning && <Spinner size={13} />}
            {rerunning ? "Starting…" : status === "notRun" && !output ? "Run checks" : "Run again"}
          </button>
        )}
      </div>

      {/* Why it did not run, or that it was stopped: the row says the outcome, this says why, and
          it is there whether or not the details are open. */}
      {note && <p className="m-0 text-[length:var(--fs-xs)] text-fg-muted">{note}</p>}

      {open && (
        <div className="flex flex-col gap-2 pt-0.5">
          {failures.length > 0 ? (
            <ul className="m-0 flex list-none flex-col gap-1 p-0">
              {failures.map((failure, i) => (
                <li key={`${failure.name}-${i}`} className="flex min-w-0 flex-col">
                  <span className="min-w-0 break-words text-danger select-text">
                    {failure.name}
                  </span>
                  {failure.detail && (
                    <span className="min-w-0 break-words font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted select-text">
                      {failure.detail}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          ) : (
            !note && (
              <p className="m-0 text-[length:var(--fs-xs)] text-fg-muted">
                {sayMore(status, command, durationMs)}
              </p>
            )
          )}

          {output && (
            <details className="group">
              <summary className="cursor-pointer list-none text-[length:var(--fs-xs)] text-fg-muted hover:text-fg">
                <span className="inline-flex items-center gap-1">
                  <CaretDownIcon
                    size={11}
                    className="transition-transform group-open:rotate-0 -rotate-90"
                  />
                  Output
                </span>
              </summary>
              <pre className="mt-1.5 mb-0 max-h-[320px] overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] whitespace-pre-wrap select-text">
                {output}
              </pre>
            </details>
          )}

          {ctx && (
            <div className="flex items-center gap-2 text-[length:var(--fs-xs)] text-fg-subtle">
              <button
                className="cursor-pointer border-none bg-transparent p-0 text-fg-muted hover:text-fg hover:underline"
                onClick={toggleAuto}
              >
                {autoOn ? "Stop running after every task" : "Run after every task"}
              </button>
              <span className="flex-1" />
            </div>
          )}
        </div>
      )}

      {error && (
        <p className="m-0 text-[length:var(--fs-xs)] text-danger select-text" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

function StatusIcon({ status }: { status: CheckStatus }) {
  switch (status) {
    case "running":
      return <Spinner size={14} className="text-fg-muted" />;
    case "passed":
      return <CheckCircleIcon size={14} className="flex-none text-success" />;
    case "failed":
      return <XCircleIcon size={14} className="flex-none text-danger" />;
    case "timedOut":
      return <ClockIcon size={14} className="flex-none text-warning" />;
    case "notRun":
      return <MinusCircleIcon size={14} className="flex-none text-fg-subtle" />;
  }
}

/** The line under the row, for a run that named nothing as failing. */
function sayMore(
  status: CheckStatus,
  command: string | null | undefined,
  durationMs: number,
): string {
  const took = durationMs >= 1000 ? ` after ${fullElapsed(durationMs)}` : "";
  switch (status) {
    case "running":
      return command ? `Running ${command} in the project…` : "Running checks…";
    case "passed":
      return command ? `“${command}” exited zero${took}.` : "The command exited zero.";
    case "timedOut":
      return "It was stopped before it finished, so the result is unknown.";
    case "notRun":
      return "Nothing ran.";
    case "failed":
      return "The output named no failures; open it to see what it said.";
  }
}

/** The row's words, and how they read. */
function say(
  status: CheckStatus,
  failed: number,
  durationMs: number,
  tests: number | null | undefined,
): { title: string; tone: string } {
  // A test run that takes less than a second still took a moment; "0s" would read as nothing.
  const took =
    durationMs >= 1000
      ? ` · ${fullElapsed(durationMs)}`
      : durationMs > 0
        ? ` · ${durationMs}ms`
        : "";
  // How many tests ran, when the command said. It is what makes "passed" mean something.
  const counted = tests ? ` · ${tests} ${tests === 1 ? "test" : "tests"}` : "";
  switch (status) {
    case "running":
      return { title: "Running checks…", tone: "text-fg-muted" };
    case "passed":
      return { title: `Checks passed${counted}${took}`, tone: "text-success" };
    case "failed":
      return {
        title:
          failed > 0
            ? `${failed} ${failed === 1 ? "check" : "checks"} failed${took}`
            : `Checks failed${took}`,
        tone: "text-danger",
      };
    case "timedOut":
      return { title: "Checks stopped before they finished", tone: "text-warning" };
    case "notRun":
      return { title: "Checks not run", tone: "text-fg-muted" };
  }
}
