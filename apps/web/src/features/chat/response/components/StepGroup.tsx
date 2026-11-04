import AgentStepDisplay from "~/features/chat/response/components/AgentStepDisplay";
import { type Entry, workKey } from "~/features/chat/response/model";

/**
 * A run of work between two texts, one row per call or block of reasoning, so the list shows what
 * is running and what is done at a glance. A settled turn folds the whole run under its head line.
 */
export default function StepGroup({
  entries,
  live,
  onOpenFile,
}: {
  entries: readonly Entry[];
  live: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const last = entries.at(-1);
  return (
    <div className="pb-2">
      {entries.map((entry) => (
        <AgentStepDisplay
          key={workKey(entry)}
          entry={entry}
          live={live}
          entering={live && entry === last}
          onOpenFile={onOpenFile}
        />
      ))}
    </div>
  );
}
