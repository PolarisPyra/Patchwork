import { CaretDownIcon } from "@phosphor-icons/react";
import type { LogLevel } from "~/api";
import { CHIP_CLASS } from "~/components/chat/ModelPicker";
import Dropdown from "~/components/workspace/Dropdown";
import { LOG_LEVELS } from "~/components/panel/logs";

/** Picks a log's level, as VS Code's "Set Log Level". */
export default function LogLevelPicker({
  label,
  value,
  onChange,
  up = false,
}: {
  /** What the log is, for the button's accessible name. */
  label: string;
  value: LogLevel;
  onChange: (level: LogLevel) => void;
  up?: boolean;
}) {
  const current = LOG_LEVELS.find((l) => l.value === value)?.label ?? value;
  return (
    <Dropdown
      alignEnd
      up={up}
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`${label} log level: ${current}`}
          onClick={toggle}
        >
          <span>{current}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) =>
        LOG_LEVELS.map((level) => (
          <button
            key={level.value}
            role="menuitemradio"
            aria-checked={level.value === value}
            className={`flex w-full min-w-[140px] cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left text-fg hover:bg-hover ${level.value === value ? "bg-selected-muted" : ""}`}
            onClick={() => {
              close();
              onChange(level.value);
            }}
          >
            {level.label}
          </button>
        ))
      }
    </Dropdown>
  );
}
