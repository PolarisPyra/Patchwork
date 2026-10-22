import type { ImportSource } from "~/api";
import claudeIcon from "../../../../assets/claude-color.svg";
import antigravityIcon from "../../../../assets/gemini-color.svg";
import openaiIcon from "../../../../assets/openai.svg";
import t3Icon from "../../../../assets/t3.svg";

// Sources without a mark here, such as OpenCode, show no logo.
const LOGOS: Partial<Record<ImportSource, string>> = {
  claude: claudeIcon,
  codex: openaiIcon,
  antigravity: antigravityIcon,
  t3: t3Icon,
};

/** Brand-ish marks for the supported agents (and T3 Code, an import source). */
export default function ProviderLogos({ kind, size = 20 }: { kind: ImportSource; size?: number }) {
  const src = LOGOS[kind];
  if (!src) return null;
  return <img src={src} width={size} height={size} alt="" aria-hidden="true" />;
}
