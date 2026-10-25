import { PlugsConnectedIcon } from "@phosphor-icons/react";
import { apiProviderOf, type ImportSource } from "~/api";
import claudeIcon from "../../../../assets/claude-color.svg";
import antigravityIcon from "../../../../assets/gemini-color.svg";
import openaiIcon from "../../../../assets/openai.svg";
import t3Icon from "../../../../assets/t3.svg";
import opencodeIcon from "../../../../assets/opencode.svg";
import deepseekIcon from "../../../../assets/deepseek-color.svg";

const LOGOS: Record<ImportSource, string> = {
  claude: claudeIcon,
  codex: openaiIcon,
  antigravity: antigravityIcon,
  t3: t3Icon,
  opencode: opencodeIcon,
};

/** Marks of the API provider presets that have one; the rest get a generic mark. */
const PRESET_LOGOS: Record<string, string> = {
  deepseek: deepseekIcon,
};

/** Brand-ish marks for the supported agents (and the import sources T3 Code and OpenCode). An API
 * provider shows its own logo, else its preset's mark: give `logo` and `preset`, or the `model`
 * (`provider/model`) to look them up. */
export default function ProviderLogos({
  kind,
  size = 20,
  preset,
  logo,
  model,
}: {
  kind: ImportSource | "api";
  size?: number;
  preset?: string;
  logo?: string | null;
  model?: string | null;
}) {
  const provider = kind === "api" && preset === undefined ? apiProviderOf(model) : undefined;
  const src =
    kind === "api"
      ? (provider ? provider.logo : logo) || PRESET_LOGOS[provider?.preset ?? preset ?? ""]
      : LOGOS[kind];
  if (!src) return <PlugsConnectedIcon size={size} aria-hidden="true" className="flex-none" />;
  return (
    <img
      src={src}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      className="flex-none object-contain"
    />
  );
}
