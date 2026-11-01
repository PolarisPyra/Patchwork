import type { Theme } from "~/api";
import { Segmented, TokenRow } from "~/features/settings/controls";
import { LOGO_URLS } from "~/features/settings/logo";
import type { PaneProps } from "~/features/settings/types";
import logoManifest from "../../../../../../assets/logos/manifest.json";

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/** The theme, and which app icon Patchwork wears in the dock and the launcher. */
export default function AppearancePane({ settings, updateSettings }: PaneProps) {
  const logo = settings.logo ?? logoManifest.default;
  return (
    <>
      <TokenRow title="Theme" desc="Light, dark, or follow the system setting">
        <Segmented
          label="Theme"
          value={settings.theme}
          options={THEMES}
          onChange={(theme) => void updateSettings((s) => ({ ...s, theme }))}
        />
      </TokenRow>
      <div className="border-b border-border py-2.5">
        <div className="font-semibold">App icon</div>
        <div role="radiogroup" aria-label="App icon" className="mt-2 grid grid-cols-8 gap-1">
          {logoManifest.icons.map((icon) => (
            <button
              key={icon.id}
              role="radio"
              aria-checked={logo === icon.id}
              aria-label={icon.name}
              title={icon.name}
              className={`grid aspect-square cursor-pointer place-items-center rounded-[var(--radius-sm)] border p-2 ${logo === icon.id ? "border-accent bg-accent-soft" : "border-transparent bg-transparent hover:bg-hover"}`}
              onClick={() => void updateSettings((s) => ({ ...s, logo: icon.id }))}
            >
              <img src={LOGO_URLS[icon.id]} alt="" draggable={false} className="size-full" />
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
