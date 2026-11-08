import type { GitSyncState } from "~/api";

/** What the Commit button and its menu offer, from where the branch stands. */

export type GitAction = "commit" | "commitPush" | "commitPushPr" | "push" | "openPr" | "viewPr";

export interface GitActions {
  /** The big button's action and label; a null action leaves it disabled. */
  primary: { action: GitAction | null; label: string };
  /** The menu's actions, in order. */
  menu: GitAction[];
}

export const ACTION_LABELS: Record<GitAction, string> = {
  commit: "Commit",
  commitPush: "Commit & Push",
  commitPushPr: "Commit, Push & Open Pull Request",
  push: "Push",
  openPr: "Open Pull Request",
  viewPr: "View Pull Request",
};

export function gitActions(state: GitSyncState | null, hasChanges: boolean): GitActions {
  if (!state?.branch) {
    return { primary: { action: hasChanges ? "commit" : null, label: "Commit" }, menu: [] };
  }
  const onDefault = state.branch === state.defaultBranch;
  const pr = state.pullRequest;
  // A pull request needs a branch other than the one it goes into, and a remote.
  const canPr = state.hasRemote && !onDefault && !!state.defaultBranch && !pr;
  const canPush = state.hasRemote && (!state.hasUpstream || state.ahead > 0);
  const menu: GitAction[] = [];
  if (hasChanges) {
    menu.push("commit");
    if (state.hasRemote) menu.push("commitPush");
    if (canPr) menu.push("commitPushPr");
  }
  if (canPush) menu.push("push");
  if (canPr && state.hasUpstream && !hasChanges) menu.push("openPr");
  if (pr) menu.push("viewPr");

  let primary: GitActions["primary"];
  if (hasChanges) primary = { action: "commit", label: "Commit" };
  else if (canPush && !state.hasUpstream) primary = { action: "push", label: "Publish Branch" };
  else if (canPush) primary = { action: "push", label: `Push ↑${state.ahead}` };
  else if (canPr && state.hasUpstream) primary = { action: "openPr", label: "Open Pull Request" };
  else if (pr) primary = { action: "viewPr", label: `View Pull Request #${pr.number}` };
  else primary = { action: null, label: "Commit" };
  return { primary, menu };
}
