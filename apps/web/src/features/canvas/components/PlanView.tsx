import { CheckIcon, CircleIcon, XIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import type { Message, StepStatus } from "~/api";

function StepIcon({ status }: { status: StepStatus }) {
  switch (status) {
    case "done":
      return <CheckIcon size={14} className="mt-1 flex-none text-accent-text" />;
    case "running":
      return <Spinner size={14} className="mt-1 text-accent-text" />;
    case "error":
      return <XIcon size={14} className="mt-1 flex-none text-danger" />;
    case "pending":
      return <CircleIcon size={14} className="mt-1 flex-none text-fg-subtle" />;
  }
}

export default function PlanView({ message }: { message: Message | null }) {
  const steps = message?.plan ?? [];
  const files = message?.files ?? [];

  return (
    <div className="py-4 px-5">
      {steps.length === 0 ? (
        <div className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-muted">
          The agent hasn't shared a plan for this task.
        </div>
      ) : (
        <ol className="flex flex-col gap-2 m-0 p-0 list-none">
          {steps.map((step, i) => (
            <li className="flex items-start gap-2" key={i}>
              <StepIcon status={step.status} />
              <span className="min-w-0 leading-normal">{step.text}</span>
            </li>
          ))}
        </ol>
      )}

      {files.length > 0 && (
        <>
          <div className="mt-6 mb-2 text-[length:var(--fs-xs)] font-semibold tracking-[0.04em] uppercase text-fg-subtle">
            Files changed
          </div>
          <ul className="flex flex-col gap-1 m-0 p-0 list-none">
            {files.map((file) => (
              <li
                className="py-1 px-2 rounded-[var(--radius-sm)] font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)] whitespace-nowrap overflow-hidden text-ellipsis select-text"
                key={file}
                title={file}
              >
                {file}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
