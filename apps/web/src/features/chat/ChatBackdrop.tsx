import type { ChatBackground } from "~/api";
import { useImageSrc } from "~/hooks/useImageSrc";
import { backgroundStyle, effectiveBackground } from "~/lib/chatBackgrounds";

/** The chat's background, behind everything it draws: its own, else the one in Settings. */
export default function ChatBackdrop({
  own,
  shared,
}: {
  own: ChatBackground | null | undefined;
  shared: ChatBackground;
}) {
  const background = effectiveBackground(own, shared);
  const src = useImageSrc(background.kind === "image" ? background.path : null);
  const style = backgroundStyle(background, src);
  if (!style) return null;
  return <div aria-hidden className="pointer-events-none absolute inset-0 -z-10" style={style} />;
}
