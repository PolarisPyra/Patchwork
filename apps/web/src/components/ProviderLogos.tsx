import { PlugsConnectedIcon } from "@phosphor-icons/react";
import { apiProviderOf, type ImportSource } from "~/api";
import claudeIcon from "../../../../assets/claude-color.svg";
import antigravityIcon from "../../../../assets/gemini-color.svg";
import openaiIcon from "../../../../assets/openai.svg";
import t3Icon from "../../../../assets/t3.svg";
import opencodeIcon from "../../../../assets/opencode.svg";
import deepseekIcon from "../../../../assets/deepseek-color.svg";
import cerebrasIcon from "../../../../assets/cerebras.svg";
import fireworksIcon from "../../../../assets/fireworks-color.svg";
import grokIcon from "../../../../assets/grok.svg";
import groqIcon from "../../../../assets/groq.svg";
import kimiIcon from "../../../../assets/kimi.svg";
import lmstudioIcon from "../../../../assets/lmstudio.svg";
import metaIcon from "../../../../assets/meta-color.svg";
import minimaxIcon from "../../../../assets/minimax-color.svg";
import mistralIcon from "../../../../assets/mistral-color.svg";
import openrouterIcon from "../../../../assets/openrouter.svg";
import perplexityIcon from "../../../../assets/perplexity-color.svg";
import qwenIcon from "../../../../assets/qwen-color.svg";
import togetherIcon from "../../../../assets/together-color.svg";
import zaiIcon from "../../../../assets/zai.svg";

const LOGOS: Record<ImportSource, string> = {
  claude: claudeIcon,
  codex: openaiIcon,
  antigravity: antigravityIcon,
  t3: t3Icon,
  opencode: opencodeIcon,
};

/** Marks of the API provider presets that have one; the rest get a generic mark. A one-colour
 * mark is `mono` and takes the text colour, so it shows on every theme. The marks come from
 * LobeHub's icon set (the MIT-licensed `@lobehub/icons-static-svg` package's `icons/` folder). */
const PRESET_LOGOS: Record<string, { src: string; mono?: true }> = {
  deepseek: { src: deepseekIcon },
  openai: { src: openaiIcon, mono: true },
  gemini: { src: antigravityIcon },
  xai: { src: grokIcon, mono: true },
  mistral: { src: mistralIcon },
  zai: { src: zaiIcon, mono: true },
  llama: { src: metaIcon },
  moonshot: { src: kimiIcon, mono: true },
  dashscope: { src: qwenIcon },
  minimax: { src: minimaxIcon },
  groq: { src: groqIcon, mono: true },
  together: { src: togetherIcon },
  fireworks: { src: fireworksIcon },
  cerebras: { src: cerebrasIcon, mono: true },
  openrouter: { src: openrouterIcon, mono: true },
  perplexity: { src: perplexityIcon },
  // A server found on this computer is `custom`, except LM Studio.
  lmstudio: { src: lmstudioIcon, mono: true },
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
  const own = kind === "api" ? (provider ? provider.logo : logo) : null;
  const mark = kind === "api" && !own ? PRESET_LOGOS[provider?.preset ?? preset ?? ""] : undefined;
  const src = kind === "api" ? own || mark?.src : LOGOS[kind];
  if (!src) return <PlugsConnectedIcon size={size} aria-hidden="true" className="flex-none" />;
  if (mark?.mono) {
    const mask = `url("${src}") center / contain no-repeat`;
    return (
      <span
        aria-hidden="true"
        className="inline-block flex-none bg-current"
        style={{ width: size, height: size, mask, WebkitMask: mask }}
      />
    );
  }
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
