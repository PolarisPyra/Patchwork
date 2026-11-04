import { useContext } from "react";
import {
  CaretDownIcon,
  CheckCircleIcon,
  ShieldWarningIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import { type Approval } from "~/api";
import { SecretCard } from "~/features/chat/transcript/components/SecretCard";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import Dropdown from "~/components/Dropdown";
import {
  findRule,
  kindLabel,
  PermissionsContext,
  scopeName,
  shownPattern,
  suggestedRules,
} from "~/lib/permissions";

/** Approvals in order, with each run of the same resolved request, answered the same way,
 * folded into its first one and counted. */
/** A reply's approvals and questions. A pending one is answered in the banner above the composer,
 * and a secret in its own card here. */
export function Approvals({ threadId, approvals }: { threadId: string; approvals: Approval[] }) {
  return groupApprovals(approvals).map(({ approval, count }) =>
    approval.secret ? (
      <SecretCard key={approval.id} threadId={threadId} approval={approval} />
    ) : approval.status === "pending" ? (
      <p
        key={approval.id}
        className="m-0 flex items-center gap-1.5 px-1 text-[length:var(--fs-sm)] text-warning"
      >
        <ShieldWarningIcon size={15} />
        {approval.questions?.length
          ? "Waiting for your answer"
          : `Waiting for approval: ${approval.title}`}
      </p>
    ) : (
      <ApprovalCard key={approval.id} approval={approval} count={count} />
    ),
  );
}

function groupApprovals(approvals: Approval[]): { approval: Approval; count: number }[] {
  const groups: { approval: Approval; count: number }[] = [];
  for (const approval of approvals) {
    const last = groups.at(-1)?.approval;
    const same =
      last &&
      approval.status !== "pending" &&
      !approval.questions?.length &&
      !approval.secret &&
      last.status === approval.status &&
      last.title === approval.title &&
      last.detail === approval.detail &&
      !!last.auto === !!approval.auto;
    if (same) groups[groups.length - 1].count += 1;
    else groups.push({ approval, count: 1 });
  }
  return groups;
}

/** A resolved approval, kept in the transcript with its outcome. One a rule answered is a single
 * quiet line; either way the outcome opens the rule behind it, or offers to save one. */
function ApprovalCard({ approval, count }: { approval: Approval; count: number }) {
  const { title, detail, status, questions } = approval;
  if (questions?.length) return <AnsweredQuestions approval={approval} />;
  const allowed = status === "allowed";
  if (approval.auto) {
    return (
      <div className="flex min-w-0 items-center gap-1.5 px-1 text-[length:var(--fs-sm)] text-fg-muted">
        {allowed ? (
          <CheckCircleIcon size={15} className="flex-none text-success" />
        ) : (
          <XCircleIcon size={15} className="flex-none text-danger" />
        )}
        <span className="flex-none text-fg">{title}</span>
        {detail && (
          <span className="min-w-0 truncate font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)]">
            {detail}
          </span>
        )}
        <span className="ml-auto flex-none">
          <RuleMenu approval={approval} count={count} />
        </span>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2 rounded-[var(--radius)] border border-border-strong bg-panel px-3 py-2.5 text-[length:var(--fs-sm)]">
      <div className={`flex items-center gap-1.5 ${allowed ? "text-success" : "text-danger"}`}>
        {allowed ? <CheckCircleIcon size={15} /> : <XCircleIcon size={15} />}
        <span className="font-semibold text-fg">{title}</span>
        <span className="ml-auto">
          <RuleMenu approval={approval} count={count} />
        </span>
      </div>
      {detail && (
        <code className="rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 font-[family-name:var(--font-mono)] break-all whitespace-pre-wrap selectable">
          {detail}
        </code>
      )}
    </div>
  );
}

const RULE_MENU_ITEM =
  "flex cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover";

/** An approval's outcome, which opens what can be done about the rule behind it: change or
 * remove the rule that answered it, or save one so the next request like it is not asked. */
function RuleMenu({ approval, count }: { approval: Approval; count: number }) {
  const ctx = useContext(PermissionsContext);
  const allowed = approval.status === "allowed";
  const { rule } = approval;
  const outcome = approval.auto
    ? allowed
      ? "Auto-allowed"
      : "Auto-denied"
    : !allowed
      ? "Denied"
      : rule
        ? "Always allowed"
        : "Allowed";
  const label = count > 1 ? `${outcome} ×${count}` : outcome;
  const saveable = allowed && !rule && !!approval.kind && (approval.suggestions?.length ?? 0) > 0;
  if (!ctx || (!rule && !saveable)) return <span>{label}</span>;
  const found = rule?.ruleId ? findRule(ctx.permissions, rule.scope, rule.ruleId) : undefined;
  const where = rule ? scopeName(rule.scope, ctx.projects) : "";
  const kind = found?.kind ?? approval.kind;
  return (
    <Dropdown
      up
      alignEnd
      label="Approval rule"
      trigger={({ open, toggle }) => (
        <button
          className="inline-flex cursor-pointer items-center gap-0.5 border-none bg-transparent p-0 text-inherit hover:underline"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={toggle}
        >
          {label}
          <CaretDownIcon size={11} />
        </button>
      )}
    >
      {(close) => (
        <>
          {rule && (
            <div className="max-w-[360px] truncate px-2 py-1 text-[length:var(--fs-xs)] text-fg-muted">
              {rule.ruleId
                ? found
                  ? `${kindLabel(found.kind)}: ${shownPattern(found.pattern)} · ${where}`
                  : `The rule was removed · ${where}`
                : `Default for ${kind ? kindLabel(kind) : "this kind"} · ${where}`}
            </div>
          )}
          {rule && (
            <button
              role="menuitem"
              className={RULE_MENU_ITEM}
              onClick={() => {
                close();
                ctx.openRule(rule.scope, found?.id);
              }}
            >
              Change in Settings…
            </button>
          )}
          {rule && found && (
            <button
              role="menuitem"
              className={RULE_MENU_ITEM}
              onClick={() => {
                close();
                ctx.removeRule(rule.scope, found.id);
              }}
            >
              Ask next time
            </button>
          )}
          {saveable &&
            [ctx.scope, { type: "everywhere" as const }].map((scope) => (
              <button
                key={scope.type}
                role="menuitem"
                className={RULE_MENU_ITEM}
                onClick={() => {
                  close();
                  ctx.addRules(scope, suggestedRules(approval));
                }}
              >
                {scope.type === "everywhere"
                  ? "Always allow everywhere"
                  : `Always allow in ${scopeName(scope, ctx.projects)}`}
              </button>
            ))}
        </>
      )}
    </Dropdown>
  );
}

/** The agent's questions and the user's answers, or that the user declined to answer. */
function AnsweredQuestions({ approval }: { approval: Approval }) {
  const { questions = [], answers = {}, status } = approval;
  return (
    <div className="flex flex-col gap-1.5 rounded-[var(--radius)] border border-border-strong bg-panel px-3 py-2.5 text-[length:var(--fs-sm)]">
      {questions.map((q) => (
        <div key={q.id} className="flex flex-col gap-0.5">
          <span className="font-semibold text-fg selectable">{q.question}</span>
          <span className="text-fg-muted selectable">
            {status === "denied" ? "Not answered" : (answers[q.id] ?? []).join(", ")}
          </span>
        </div>
      ))}
    </div>
  );
}
