import type { Thread } from "~/api";
import { isBusy } from "~/lib/threadEvents";

/** Signatures by thread object: a change makes a new object, so each is computed once, not on
 * every streamed update for every thread. */
const signatures = new WeakMap<Thread, string>();

/** What the sidebar shows of a thread; it re-renders only when one of these changes. */
export function sidebarSignature(t: Thread): string {
  let signature = signatures.get(t);
  if (signature === undefined) {
    signature = computeSidebarSignature(t);
    signatures.set(t, signature);
  }
  return signature;
}

function computeSidebarSignature(t: Thread): string {
  const last = t.messages[t.messages.length - 1];
  const needsApproval = last?.approvals.some((a) => a.status === "pending") ?? false;
  return [
    t.id,
    t.title,
    t.projectId,
    t.folderId,
    t.archivedAt,
    t.favorite,
    t.pinned,
    t.agent,
    t.accessMode,
    t.goal?.status,
    last?.createdAt,
    needsApproval,
    isBusy(t),
  ].join("|");
}
