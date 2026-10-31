import { useState } from "react";
import { WarningCircleIcon } from "@phosphor-icons/react";
import AgentStepDisplay, {
  ENTRY_CLASS,
  EntryText,
  LineGutter,
} from "~/features/chat/response/components/AgentStepDisplay";
import Expander from "~/features/chat/response/components/Expander";
import { actionIcon, entryIcon } from "~/features/chat/response/components/ToolIcon";
import { type Entry, entryActive, workKey } from "~/features/chat/response/lib/model";
import { actionOf, entryLabel, groupLabel } from "~/features/chat/response/lib/tools";

/**
 * A run of work between two texts: one row, or a summary row over several that opens to list them.
 * While its last call runs, the summary shows that call instead, so the row always says what is
 * happening now rather than how many things have happened.
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
  const [open, setOpen] = useState(false);
  const [first] = entries;
  const last = entries.at(-1);
  if (entries.length === 1 && first !== undefined) {
    return (
      <div className="pb-2">
        <AgentStepDisplay entry={first} live={live} entering={live} onOpenFile={onOpenFile} />
      </div>
    );
  }
  const current = live ? [...entries].reverse().find(entryActive) : undefined;
  const tools = entries.flatMap((entry) => (entry.kind === "tool" ? [entry.tool] : []));
  const failed = tools.some((tool) => tool.status === "error");
  const head = tools[0];
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ENTRY_CLASS}
      >
        <LineGutter>
          {current ? (
            entryIcon(current)
          ) : failed ? (
            <WarningCircleIcon size={15} className="text-danger" />
          ) : head ? (
            actionIcon(actionOf(head))
          ) : (
            first && entryIcon(first)
          )}
        </LineGutter>
        <EntryText entering={live} running={current !== undefined}>
          {current ? entryLabel(current) : groupLabel(tools)}
        </EntryText>
      </button>
      {open && (
        <Expander>
          {entries.map((entry) => (
            <AgentStepDisplay
              key={workKey(entry)}
              entry={entry}
              live={live}
              entering={live && entry === last}
              onOpenFile={onOpenFile}
            />
          ))}
        </Expander>
      )}
    </div>
  );
}
