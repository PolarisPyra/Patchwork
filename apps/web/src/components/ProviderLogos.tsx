import type { ImportSource } from "~/api";
import claudeIcon from "../../../../assets/claude-color.svg";
import antigravityIcon from "../../../../assets/gemini-color.svg";
import openaiIcon from "../../../../assets/openai.svg";
import t3Icon from "../../../../assets/t3.svg";
import opencodeIcon from "../../../../assets/opencode.svg";

const LOGOS: Record<ImportSource, string> = {
  claude: claudeIcon,
  codex: openaiIcon,
  antigravity: antigravityIcon,
  t3: t3Icon,
  opencode: opencodeIcon,
};

/** Brand-ish marks for the supported agents (and the import sources T3 Code and OpenCode). */
export default function ProviderLogos({ kind, size = 20 }: { kind: ImportSource; size?: number }) {
  return <img src={LOGOS[kind]} width={size} height={size} alt="" aria-hidden="true" />;
}
