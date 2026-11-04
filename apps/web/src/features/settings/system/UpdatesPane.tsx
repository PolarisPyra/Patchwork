import { useEffect, useState, type ReactNode } from "react";
import { CaretRightIcon } from "@phosphor-icons/react";
import { api, onUpdateStatus, type UpdateState, type UpdateStatus } from "~/api";
import { Markdown } from "~/features/chat";
import { MinutesStepper, TokenRow } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";
import { COMMERCIAL_LICENSE_URL, LICENSE_URL, RELEASES_URL, restartToUpdate } from "~/lib/updates";
import { errorText } from "~/lib/util";
import changelog from "../../../../../../CHANGELOG.md?raw";

/** Where updating stands, in one line. */
function updateText(state: UpdateState, canInstall: boolean): string {
  switch (state.status) {
    case "idle":
      return canInstall ? "Updates install on their own" : "This copy can't install updates";
    case "checking":
      return "Checking for updates…";
    case "upToDate":
      return "Up to date";
    case "available":
      return `Patchwork ${state.version} is available`;
    case "downloading": {
      const progress = state.total
        ? `${Math.floor((state.downloaded / state.total) * 100)}%`
        : `${(state.downloaded / 2 ** 20).toFixed(1)} MB`;
      return `Downloading Patchwork ${state.version} · ${progress}`;
    }
    case "ready":
      return `Patchwork ${state.version} is installed. Restart to use it.`;
    case "failed":
      return state.message;
  }
}

/** The running version, where updating stands, and the action that moves it on. */
export default function UpdatesPane({ settings, updateSettings }: PaneProps) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const unlisten = onUpdateStatus((next) => {
      if (live) setStatus(next);
    });
    // An event that arrived first is newer than this reply.
    api
      .updateStatus()
      .then((next) => {
        if (live) setStatus((current) => current ?? next);
      })
      .catch((e: unknown) => setError(errorText(e)));
    return () => {
      live = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  if (!status)
    return (
      error && (
        <div role="alert" className="py-2.5 text-danger">
          {error}
        </div>
      )
    );
  const { state } = status;

  const run = (action: () => Promise<unknown>) => {
    setError(null);
    action().catch((e: unknown) => setError(errorText(e)));
  };
  let action: ReactNode;
  if (state.status === "ready") {
    action = (
      <button className="btn" onClick={() => run(restartToUpdate)}>
        Restart to update
      </button>
    );
  } else if (state.status === "available") {
    action = (
      <button className="btn" onClick={() => run(() => api.openUrl(RELEASES_URL))}>
        Download
      </button>
    );
  } else {
    const working = state.status === "checking" || state.status === "downloading";
    action = (
      <button className="btn" disabled={working} onClick={() => run(api.checkForUpdates)}>
        {state.status === "checking" ? "Checking…" : "Check for updates"}
      </button>
    );
  }
  return (
    <>
      <div className="flex items-center gap-3 border-b border-border py-2.5">
        <div className="min-w-0 flex-1">
          <div className="font-semibold">Patchwork {status.currentVersion}</div>
          <div
            className={`text-[length:var(--fs-sm)] ${state.status === "failed" ? "text-danger" : "text-fg-muted"}`}
          >
            {updateText(state, status.canInstall)}
          </div>
        </div>
        {action}
      </div>
      <TokenRow title="Check for updates every">
        <MinutesStepper
          label="Check for updates every"
          value={settings.updates.checkEveryMinutes}
          onChange={(checkEveryMinutes) =>
            void updateSettings((s) => ({ ...s, updates: { ...s.updates, checkEveryMinutes } }))
          }
        />
      </TokenRow>
      {error && (
        <div role="alert" className="py-2.5 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      {status.notes && state.status !== "upToDate" && (
        <div className="border-b border-border pt-1 pb-3">
          <Markdown text={releaseNotes(status.notes)} />
        </div>
      )}
      {/* The AGPL asks a program with a UI to show its copyright, warranty, and license. */}
      <p className="m-0 pt-3 text-[length:var(--fs-sm)] text-fg-subtle">
        Patchwork is free software under the{" "}
        <button
          type="button"
          className="cursor-pointer border-none bg-transparent p-0 text-accent-text underline underline-offset-2 hover:text-accent-hover"
          onClick={() => void api.openUrl(LICENSE_URL)}
        >
          GNU AGPL v3.0 or later
        </button>
        , and its paid features are under the{" "}
        <button
          type="button"
          className="cursor-pointer border-none bg-transparent p-0 text-accent-text underline underline-offset-2 hover:text-accent-hover"
          onClick={() => void api.openUrl(COMMERCIAL_LICENSE_URL)}
        >
          Patchwork Commercial License
        </button>
        . © 2026 PolarisPyra. No warranty.
      </p>
      <ReleaseList />
    </>
  );
}

/** A release's section of CHANGELOG.md, its `## [0.2.0] - 2026-01-02` heading read as
 * `## 0.2.0 · 2026-01-02`. */
function releaseNotes(section: string): string {
  return section.replace(/^## \[(.+?)\] - /, "## $1 · ");
}

/** The newest three releases in CHANGELOG.md as it was when this copy was built, newest first:
 * their version and date, and the notes under their heading. */
const RELEASES = changelog
  .split(/\n(?=## \[)/)
  .slice(1, 4)
  .map((section) => {
    const [heading, ...rest] = section.trim().split("\n");
    const [, version = heading, date = ""] = /^## \[(.+?)\](?: - (.+))?/.exec(heading) ?? [];
    return { version, date, notes: rest.join("\n").trim() };
  });

/** The releases' notes, one row each. Only the newest is open at first, and a release's notes
 * render only once it is opened, so the pane does not draw every release at once. */
function ReleaseList() {
  const [open, setOpen] = useState<ReadonlySet<string>>(
    () => new Set(RELEASES.slice(0, 1).map((r) => r.version)),
  );
  const toggle = (version: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(version)) next.add(version);
      return next;
    });
  return (
    <div className="pt-1">
      {RELEASES.map(({ version, date, notes }) => {
        const isOpen = open.has(version);
        return (
          <div key={version} className="border-b border-border last:border-b-0">
            <button
              className="flex w-full cursor-pointer items-center gap-1.5 border-none bg-transparent py-2 text-left text-fg"
              aria-expanded={isOpen}
              onClick={() => toggle(version)}
            >
              <CaretRightIcon
                size={12}
                className={`flex-none text-fg-muted transition-transform ${isOpen ? "rotate-90" : ""}`}
              />
              <span className="font-semibold">{version}</span>
              {date && <span className="text-fg-muted">{date}</span>}
            </button>
            {isOpen && (
              <div className="pb-3 pl-[18px]">
                <Markdown text={notes} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
