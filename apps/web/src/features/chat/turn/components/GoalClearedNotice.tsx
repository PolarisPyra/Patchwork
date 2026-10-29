import { CheckIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";

/**
 * The result of clearing a goal: a status line, like the head of a turn. It fades in as it arrives
 * and not on every remount, so scrolling a list of turns does not replay it.
 */
export default function GoalClearedNotice({
  text,
  live,
  pending = false,
}: {
  text: string;
  live: boolean;
  /** The clear is still being confirmed, so the line shows a spinner instead of a tick. */
  pending?: boolean;
}) {
  return (
    <p
      className={`${live ? "goal-cleared-in " : ""}m-0 flex min-h-6 items-center gap-1.5 text-[length:var(--fs-md)] text-fg-muted [word-break:break-word]`}
    >
      {pending ? (
        <Spinner size={14} className="text-fg-subtle" />
      ) : (
        <CheckIcon size={14} className="flex-none text-fg-subtle" />
      )}
      {text}
    </p>
  );
}
