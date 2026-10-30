import { useState } from "react";
import { CaretDownIcon, ShieldWarningIcon } from "@phosphor-icons/react";
import type { Approval, PermissionScope } from "~/api";
import Dropdown from "~/components/Dropdown";
import {
  type AllowChoice,
  lastAllowChoice,
  rememberAllowChoice,
  shownPattern,
} from "~/lib/permissions";

interface PendingApprovalBannerProps {
  /** Approvals waiting for an answer, oldest first. Only the first is shown. */
  approvals: Approval[];
  /** The thread's own scope, and its name ("Chats" or the project's). */
  scope: PermissionScope;
  scopeName: string;
  /** `always` saves the approval's suggested rules in that scope as well. */
  onRespond: (approval: Approval, allow: boolean, always?: PermissionScope) => void;
}

/** The tool approval the agent is waiting on, above the composer, with Deny and Allow. Allow
 * can also save a rule, for the thread's project (or chats) or for everywhere; the button offers
 * the choice made last for the same kind of request. */
export default function PendingApprovalBanner({
  approvals,
  scope,
  scopeName,
  onRespond,
}: PendingApprovalBannerProps) {
  const [approval] = approvals;
  // Re-read on each new approval, so a choice made on the last one carries over.
  const [, setChoiceVersion] = useState(0);
  if (!approval) return null;
  const { title, detail, kind, suggestions = [] } = approval;
  const canSave = suggestions.length > 0;
  const choices: { choice: AllowChoice; label: string }[] = [
    { choice: "once", label: "Allow once" },
    { choice: "local", label: `Always allow in ${scopeName}` },
    { choice: "everywhere", label: "Always allow everywhere" },
  ];
  const saved = lastAllowChoice(kind);
  const main = canSave ? (choices.find((c) => c.choice === saved) ?? choices[0]) : choices[0];
  const allow = (choice: AllowChoice) => {
    rememberAllowChoice(kind, choice);
    setChoiceVersion((v) => v + 1);
    const always =
      choice === "local"
        ? scope
        : choice === "everywhere"
          ? { type: "everywhere" as const }
          : undefined;
    onRespond(approval, true, always);
  };
  const patterns = suggestions.map(shownPattern).join(", ");

  return (
    <div className="mb-1.5 flex flex-col gap-2 rounded-[var(--radius)] border border-warning bg-panel p-2.5 text-[length:var(--fs-sm)]">
      <div className="flex items-center gap-1.5 text-warning">
        <ShieldWarningIcon size={15} />
        <span className="min-w-0 truncate font-semibold text-fg">{title}</span>
        {approvals.length > 1 && (
          <span className="ml-auto flex-none text-fg-muted tabular-nums">1/{approvals.length}</span>
        )}
      </div>
      {detail && (
        <code className="max-h-40 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 font-[family-name:var(--font-mono)] break-all whitespace-pre-wrap selectable">
          {detail}
        </code>
      )}
      <div className="flex justify-end gap-1.5">
        <button className="btn" onClick={() => onRespond(approval, false)}>
          Deny
        </button>
        <div className="inline-flex">
          <button
            className={`btn btn-primary ${canSave ? "rounded-r-none" : ""}`}
            onClick={() => allow(main.choice)}
          >
            {main.choice === "once" ? "Allow" : main.label}
          </button>
          {canSave && (
            <Dropdown
              up
              alignEnd
              trigger={({ open, toggle }) => (
                <button
                  className="btn btn-primary !bg-accent rounded-l-none border-l border-l-[color-mix(in_srgb,var(--text-on-accent),transparent_70%)] !px-1.5"
                  aria-haspopup="menu"
                  aria-expanded={open}
                  aria-label="More ways to allow"
                  onClick={toggle}
                >
                  <CaretDownIcon size={12} />
                </button>
              )}
            >
              {(close) =>
                choices.map(({ choice, label }) => (
                  <button
                    key={choice}
                    role="menuitem"
                    className={`flex min-w-[260px] cursor-pointer flex-col items-start gap-0.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg ${choice === main.choice ? "bg-selected-muted" : "hover:bg-hover"}`}
                    onClick={() => {
                      close();
                      allow(choice);
                    }}
                  >
                    <span>{label}</span>
                    {choice !== "once" && (
                      <span className="max-w-[360px] truncate font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted">
                        {patterns}
                      </span>
                    )}
                  </button>
                ))
              }
            </Dropdown>
          )}
        </div>
      </div>
    </div>
  );
}
