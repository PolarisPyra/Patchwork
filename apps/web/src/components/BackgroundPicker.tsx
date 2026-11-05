import { useState, type CSSProperties, type ReactNode } from "react";
import { ImageIcon, ProhibitIcon } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type ChatBackground } from "~/api";
import { useImageSrc } from "~/hooks/useImageSrc";
import { BACKGROUND_PRESETS, backgroundStyle } from "~/lib/chatBackgrounds";
import { errorText } from "~/lib/util";

const PICTURE_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif"];

interface BackgroundPickerProps {
  /** The name of the group of choices. */
  label: string;
  /** The background picked; null for the shared one, when `shared` is given. */
  value: ChatBackground | null;
  onChange: (background: ChatBackground | null) => void;
  /** Offers the background every chat shares as the first choice, picked as null. */
  shared?: ChatBackground;
}

/** The backgrounds a chat can show: none, the presets, and a picture from disk. */
export default function BackgroundPicker({
  label,
  value,
  onChange,
  shared,
}: BackgroundPickerProps) {
  const [error, setError] = useState<string | null>(null);
  const picture = value?.kind === "image" ? value.path : null;
  const pictureSrc = useImageSrc(picture);
  const sharedPicture = shared?.kind === "image" ? shared.path : null;
  const sharedSrc = useImageSrc(sharedPicture);
  const pick = (background: ChatBackground | null) => {
    setError(null);
    onChange(background);
  };
  const choosePicture = async () => {
    setError(null);
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Pictures", extensions: PICTURE_EXTENSIONS }],
      });
      if (typeof path !== "string") return;
      onChange({ kind: "image", path: await api.importBackgroundImage(path) });
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <>
      <div role="radiogroup" aria-label={label} className="grid grid-cols-5 gap-2">
        {shared && (
          <Swatch
            name="Same as all chats"
            checked={value === null}
            style={backgroundStyle(shared, sharedSrc, 0)}
            onPick={() => pick(null)}
          />
        )}
        <Swatch
          name="None"
          checked={value?.kind === "none"}
          style={null}
          onPick={() => pick({ kind: "none" })}
        >
          <ProhibitIcon size={18} className="text-fg-subtle" />
        </Swatch>
        {BACKGROUND_PRESETS.map((preset) => (
          <Swatch
            key={preset.id}
            name={preset.name}
            checked={value?.kind === "preset" && value.id === preset.id}
            style={{ background: preset.css }}
            onPick={() => pick({ kind: "preset", id: preset.id })}
          />
        ))}
        {picture && (
          <Swatch
            name="Picture"
            checked
            style={backgroundStyle({ kind: "image", path: picture }, pictureSrc, 0)}
            onPick={() => {}}
          />
        )}
        <button
          type="button"
          aria-label="Choose a picture"
          title="Choose a picture"
          className="grid aspect-[4/3] cursor-pointer place-items-center rounded-[var(--radius-sm)] border border-dashed border-border-strong bg-transparent text-fg-muted hover:bg-hover"
          onClick={() => void choosePicture()}
        >
          <ImageIcon size={18} />
        </button>
      </div>
      {error && (
        <div role="alert" className="pt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}

function Swatch({
  name,
  checked,
  style,
  onPick,
  children,
}: {
  name: string;
  checked: boolean;
  style: CSSProperties | null;
  onPick: () => void;
  children?: ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-label={name}
      title={name}
      style={style ?? undefined}
      className={`grid aspect-[4/3] place-items-center rounded-[var(--radius-sm)] border-2 bg-editor ${checked ? "border-accent" : "border-border"} cursor-pointer hover:border-border-strong`}
      onClick={onPick}
    >
      {children}
    </button>
  );
}
