import type { BackgroundStrength, ChatBackground } from "~/api";
import { useImageSrc } from "~/hooks/useImageSrc";
import {
  backgroundImagePath,
  backgroundStyle,
  effectiveBackground,
  STRENGTH_VEIL,
  useBackgroundImages,
} from "~/lib/chatBackgrounds";

/** The chat's background, behind everything it draws: its own, else the one in Settings. */
export default function ChatBackdrop({
  own,
  shared,
  strength,
}: {
  own: ChatBackground | null | undefined;
  shared: ChatBackground;
  strength: BackgroundStrength;
}) {
  const background = effectiveBackground(own, shared);
  const src = useImageSrc(backgroundImagePath(useBackgroundImages(), background));
  const style = backgroundStyle(background, src, STRENGTH_VEIL[strength]);
  if (!style) return null;
  return <div aria-hidden className="pointer-events-none absolute inset-0 -z-10" style={style} />;
}
