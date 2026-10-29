import {
  CaretDownIcon,
  LockOpenIcon,
  LockSimpleIcon,
  PencilSimpleIcon,
  SparkleIcon,
  type Icon,
} from "@phosphor-icons/react";
import type { AccessMode } from "~/api";
import Dropdown from "~/components/Dropdown";
import { CHIP_CLASS } from "~/components/chip";

const MODES: { mode: AccessMode; label: string; description: string; Icon: Icon }[] = [
  {
    mode: "supervised",
    label: "Supervised",
    description: "Ask before commands and file changes.",
    Icon: LockSimpleIcon,
  },
  {
    mode: "auto-accept-edits",
    label: "Auto-accept edits",
    description: "Auto-approve edits, ask before other actions.",
    Icon: PencilSimpleIcon,
  },
  {
    mode: "auto",
    label: "Auto",
    description: "Supported providers approve routine actions; others still ask.",
    Icon: SparkleIcon,
  },
  {
    mode: "full-access",
    label: "Full access",
    description: "Allow commands and edits without prompts.",
    Icon: LockOpenIcon,
  },
];

const storageKey = "patchwork-access-mode";

/** The mode last picked, used for new threads. */
export function lastAccessMode(): AccessMode {
  try {
    const saved = localStorage.getItem(storageKey);
    return MODES.find((m) => m.mode === saved)?.mode ?? "full-access";
  } catch {
    return "full-access";
  }
}

function rememberAccessMode(mode: AccessMode) {
  try {
    localStorage.setItem(storageKey, mode);
  } catch {
    /* New threads fall back to full access. */
  }
}

export default function AccessPicker({
  mode,
  onPick,
}: {
  mode: AccessMode;
  onPick: (mode: AccessMode) => void;
}) {
  const current = MODES.find((m) => m.mode === mode) ?? MODES[3];
  return (
    <Dropdown
      up
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Access: ${current.label}`}
          onClick={toggle}
        >
          <current.Icon size={16} />
          <span>{current.label}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) =>
        MODES.map(({ mode: m, label, description, Icon }) => (
          <button
            key={m}
            role="menuitemradio"
            aria-checked={m === mode}
            className={`flex min-w-[300px] cursor-pointer flex-col items-start gap-0.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg ${m === mode ? "bg-selected-muted" : "hover:bg-hover"}`}
            onClick={() => {
              close();
              rememberAccessMode(m);
              onPick(m);
            }}
          >
            <span className="inline-flex items-center gap-1.5">
              <Icon size={14} />
              {label}
            </span>
            <span className="pl-5 text-[length:var(--fs-sm)] text-fg-muted">{description}</span>
          </button>
        ))
      }
    </Dropdown>
  );
}
