import { useEffect, useState } from "react";
import { CaretDownIcon } from "@phosphor-icons/react";
import { api, type ShellChoices } from "~/api";
import { CHIP_CLASS } from "~/components/chip";
import { TokenRow } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";
import Dropdown from "~/components/Dropdown";
import { baseName, errorText } from "~/lib/util";

/** How the agents' shell is chosen. Shells come from /etc/shells, or on Windows the installed Git
 * Bash, PowerShell, and cmd; a saved shell that is no
 * longer listed stays in the menu so the choice is still visible. */
export default function EnvironmentPane({ settings, updateSettings }: PaneProps) {
  const [choices, setChoices] = useState<ShellChoices>({ shells: [], default: null });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listShells().then(setChoices, (e) => setError(errorText(e)));
  }, []);

  const { shells, default: fallback } = choices;
  const shell = settings.shell ?? "";
  const options = shell && !shells.includes(shell) ? [shell, ...shells] : shells;
  const choose = (path: string | null) => void updateSettings((s) => ({ ...s, shell: path }));
  return (
    <>
      <TokenRow title="Shell" desc="The shell agents run commands in">
        <Dropdown
          alignEnd
          label="Shell"
          trigger={({ open, toggle }) => (
            <button
              className={`${CHIP_CLASS} !bg-transparent !text-fg-muted`}
              aria-haspopup="menu"
              aria-expanded={open}
              aria-label={`Shell: ${shell ? baseName(shell) : "System default"}`}
              title={shell || `Follows your login shell${fallback ? ` (${fallback})` : ""}`}
              onClick={toggle}
            >
              <span>{shell ? baseName(shell) : "System default"}</span>
              <CaretDownIcon size={14} />
            </button>
          )}
        >
          {(close) => (
            <div className="max-h-[240px] overflow-y-auto">
              {[null, ...options].map((path) => (
                <button
                  key={path ?? "default"}
                  role="menuitemradio"
                  aria-checked={path === (shell || null)}
                  title={path ?? undefined}
                  className={`flex w-full min-w-[160px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${path === (shell || null) ? "bg-selected-muted" : ""}`}
                  onClick={() => {
                    close();
                    choose(path);
                  }}
                >
                  {path
                    ? baseName(path)
                    : `System default${fallback ? ` (${baseName(fallback)})` : ""}`}
                </button>
              ))}
            </div>
          )}
        </Dropdown>
      </TokenRow>
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
