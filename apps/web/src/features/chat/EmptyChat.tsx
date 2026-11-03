import { FolderPlusIcon } from "@phosphor-icons/react";
import type { AgentKind, Project } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";

const SUGGESTIONS = [
  { label: "Explain this codebase", prefill: "Explain this codebase" },
  { label: "Find and fix a bug", prefill: "Find and fix a bug" },
  { label: "Write tests for…", prefill: "Write tests for " },
];

const CHAT_SUGGESTIONS = [
  { label: "Explain a concept", prefill: "Explain " },
  { label: "Draft a script", prefill: "Write a script that " },
  { label: "Review a snippet", prefill: "Review this code:\n" },
];

/** What a chat shows before its first message: what to ask, and a few ways to start. */
export default function EmptyChat({
  agent,
  project,
  onOpenFolder,
  onPrefill,
}: {
  agent: AgentKind;
  project: Project | null;
  onOpenFolder: () => void;
  onPrefill: (text: string) => void;
}) {
  return (
    <div className="flex h-auto flex-col items-center justify-center gap-4 p-6 text-center">
      <ProviderLogos kind={agent} size={40} />
      <h2 className="m-0 text-[length:var(--fs-xl)] font-semibold">
        {project ? `What should we build in ${project.name}?` : "What's on your mind?"}
      </h2>
      {!project && (
        <>
          <p className="-mt-1.5 mb-0 text-fg-muted">
            This chat has no project; the agent works in a private scratch folder.
          </p>
          <button className="btn my-2 h-8 !px-4" onClick={onOpenFolder}>
            <FolderPlusIcon size={14} />
            Open a project
          </button>
        </>
      )}
      <div className="flex max-w-[560px] flex-wrap justify-center gap-2">
        {(project ? SUGGESTIONS : CHAT_SUGGESTIONS).map((s) => (
          <button
            key={s.label}
            className="h-7 cursor-pointer rounded-[14px] border border-border-strong bg-panel px-3 text-fg hover:bg-hover"
            onClick={() => onPrefill(s.prefill)}
          >
            {s.label}
          </button>
        ))}
      </div>
    </div>
  );
}
