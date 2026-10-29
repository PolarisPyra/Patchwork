import { useEffect, useState } from "react";
import { api } from "~/api";

/**
 * The user messages whose turn can be undone. Each turn takes a checkpoint as it starts, so the
 * list is read again whenever a turn starts or ends.
 */
export function useCheckpoints(
  threadId: string | undefined,
  projectId: string | null,
  working: boolean,
): ReadonlySet<string> {
  const [checkpoints, setCheckpoints] = useState<ReadonlySet<string>>(() => new Set());
  useEffect(() => {
    if (!threadId || !projectId) {
      setCheckpoints(new Set());
      return;
    }
    let live = true;
    api.checkpoints(threadId).then(
      (ids) => live && setCheckpoints(new Set(ids)),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [threadId, projectId, working]);
  return checkpoints;
}
