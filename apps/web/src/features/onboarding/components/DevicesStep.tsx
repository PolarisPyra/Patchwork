import { HardDrivesIcon, LaptopIcon } from "@phosphor-icons/react";

export default function DevicesStep() {
  return (
    <>
      <div className="text-[length:var(--fs-md)] font-semibold">Execution environments</div>
      <div className="flex flex-col gap-2">
        <div className="flex w-full items-center gap-4 rounded-[var(--radius)] border border-accent bg-accent-soft p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease]">
          <span className="inline-flex h-10 w-10 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-[length:var(--fs-lg)] font-semibold text-accent-text">
            <LaptopIcon size={20} />
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="text-[length:var(--fs-lg)] font-semibold">This computer</span>
            <span className="text-[length:var(--fs-sm)] text-fg-muted">
              Run agents and tools on your local development machine.
            </span>
          </span>
          <span className="flex-none text-[length:var(--fs-sm)] font-semibold text-accent-text">
            ● Selected
          </span>
        </div>

        <div className="flex w-full items-center gap-4 rounded-[var(--radius)] border border-border bg-transparent p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease]">
          <span className="inline-flex h-10 w-10 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-[length:var(--fs-lg)] font-semibold text-accent-text">
            <HardDrivesIcon size={20} />
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="text-[length:var(--fs-lg)] font-semibold">Add a remote device</span>
            <span className="text-[length:var(--fs-sm)] text-fg-muted">
              Connect another machine or remote environment later.
            </span>
          </span>
          <span className="flex flex-none items-center gap-3">
            <button
              type="button"
              className="link disabled:cursor-default disabled:opacity-50 disabled:hover:no-underline"
              disabled
            >
              + Add
            </button>
            <span className="badge">Coming soon</span>
          </span>
        </div>
      </div>

      <div className="mt-6 rounded-[var(--radius)] bg-selected-muted p-4">
        <div className="mb-1 font-semibold">Connection and permissions</div>
        <div className="text-[length:var(--fs-sm)] text-fg-muted">
          Patchwork will ask before accessing protected resources or running sensitive actions.
        </div>
      </div>
    </>
  );
}
