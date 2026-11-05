import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { ImageIcon, ProhibitIcon, XIcon } from "@phosphor-icons/react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type BackgroundImage, type ChatBackground } from "~/api";
import Spinner from "~/components/Spinner";
import { useImageSrc } from "~/hooks/useImageSrc";
import {
  BACKGROUND_PRESETS,
  backgroundImagePath,
  backgroundStyle,
  refreshBackgroundImages,
  useBackgroundImages,
} from "~/lib/chatBackgrounds";
import { errorText } from "~/lib/util";

const PICTURE_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif"];
const COLUMNS = 5;
/** How long a deleted picture can be brought back from the picker. */
const UNDO_MS = 6000;

const isPicture = (path: string) =>
  PICTURE_EXTENSIONS.includes(path.split(".").pop()?.toLowerCase() ?? "");

interface BackgroundPickerProps {
  /** The name of the group of choices. */
  label: string;
  /** The background picked; null for the one every chat shares. */
  value: ChatBackground | null;
  onChange: (background: ChatBackground | null) => void;
  /** What the clear choice picks: null for the background every chat shares, or none. */
  cleared: ChatBackground | null;
  /** The background every chat shares, shown faintly on the clear choice when it picks it. */
  shared?: ChatBackground;
  /** Deletes a picture, and moves whatever showed it back to its default. Resolves to the way to
   * take that back. */
  onRemove: (file: string) => Promise<() => Promise<void>>;
}

/**
 * The backgrounds a chat can show: the default, the presets, and the pictures picked from disk.
 * A picture also comes in dropped on it or pasted while it has focus.
 */
export default function BackgroundPicker({
  label,
  value,
  onChange,
  cleared,
  shared,
  onRemove,
}: BackgroundPickerProps) {
  const [error, setError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [undo, setUndo] = useState<{ name: string; run: () => Promise<void> } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const images = useBackgroundImages();
  const sharedSrc = useImageSrc(backgroundImagePath(images, shared));

  const pick = (background: ChatBackground | null) => {
    setError(null);
    onChange(background);
  };

  /** Saves the pictures `load` brings in, and picks the last. */
  const add = async (load: () => Promise<string[]>) => {
    setError(null);
    setImporting(true);
    try {
      const files = await load();
      await refreshBackgroundImages();
      const last = files.at(-1);
      if (last) onChange({ kind: "image", file: last });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setImporting(false);
    }
  };
  const importPaths = async (paths: string[]) => {
    const files: string[] = [];
    for (const path of paths.filter(isPicture)) files.push(await api.importBackgroundImage(path));
    return files;
  };
  // The drop listener is set up once, so it reaches the current `add` through this.
  const addRef = useRef({ add, importPaths });
  addRef.current = { add, importPaths };

  const choosePicture = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "Pictures", extensions: PICTURE_EXTENSIONS }],
    }).catch((e) => {
      setError(errorText(e));
      return null;
    });
    if (typeof path === "string") await add(() => importPaths([path]));
  };

  const paste = () =>
    add(async () => {
      // A picture file copied in a file manager comes as its path; a copied image as its pixels.
      const paths = (await api.clipboardFiles()).filter(isPicture);
      if (paths.length > 0) return importPaths(paths);
      const file = await api.pasteBackgroundImage();
      if (!file) throw new Error("The clipboard holds no picture");
      return [file];
    });

  const remove = async (image: BackgroundImage, name: string) => {
    setError(null);
    try {
      const run = await onRemove(image.file);
      setUndo({ name, run });
    } catch (e) {
      setError(errorText(e));
    }
  };
  const takeBack = async () => {
    if (!undo) return;
    setUndo(null);
    try {
      await undo.run();
    } catch (e) {
      setError(errorText(e));
    }
  };
  useEffect(() => {
    if (!undo) return;
    const timer = setTimeout(() => setUndo(null), UNDO_MS);
    return () => clearTimeout(timer);
  }, [undo]);

  // Tauri hands dropped files over as paths (HTML drop events carry none), so drops are listened
  // for on the webview and kept only when they land on the picker.
  useEffect(() => {
    function over(position: { x: number; y: number }) {
      const scale = window.devicePixelRatio;
      const hit = document.elementFromPoint(position.x / scale, position.y / scale);
      return !!hit && !!root.current?.contains(hit);
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setDropping(false);
        } else if (payload.type === "drop") {
          setDropping(false);
          if (over(payload.position) && payload.paths.some(isPicture)) {
            const { add, importPaths } = addRef.current;
            void add(() => importPaths(payload.paths));
          }
        } else {
          setDropping(over(payload.position));
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // The choices are one stop for Tab; the arrow keys move between them and pick, as in any group
  // of radio buttons, with up and down moving a row of the grid.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "v") {
      e.preventDefault();
      void paste();
      return;
    }
    const radios = [...(root.current?.querySelectorAll<HTMLElement>('[role="radio"]') ?? [])];
    const at = radios.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    const step: Record<string, number> = {
      ArrowRight: 1,
      ArrowLeft: -1,
      ArrowDown: COLUMNS,
      ArrowUp: -COLUMNS,
    };
    let next: number;
    if (e.key in step) next = Math.min(radios.length - 1, Math.max(0, at + step[e.key]));
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = radios.length - 1;
    else return;
    e.preventDefault();
    radios[next].focus();
    radios[next].click();
  };

  const clearChecked = cleared === null ? value === null : value?.kind === cleared.kind;
  const checkedPicture = value?.kind === "image" ? value.file : null;
  const checkedPreset = value?.kind === "preset" ? value.id : null;
  // With nothing shown as picked, as when a chat's picture is gone, the first choice takes the
  // Tab stop.
  const anyChecked =
    clearChecked ||
    BACKGROUND_PRESETS.some((p) => p.id === checkedPreset) ||
    images.some((image) => image.file === checkedPicture);
  const clearStyle = cleared === null && shared ? backgroundStyle(shared, sharedSrc, 55) : null;
  const clearLabel = cleared ? "No background" : "Same as all chats";
  return (
    <div ref={root} onKeyDown={onKeyDown}>
      <div
        role="radiogroup"
        aria-label={label}
        className={`grid grid-cols-5 gap-2 rounded-[var(--radius-sm)] outline-2 outline-offset-4 ${dropping ? "outline-dashed outline-accent" : "outline-transparent"}`}
      >
        <button
          type="button"
          role="radio"
          aria-checked={clearChecked}
          aria-label={clearLabel}
          tabIndex={clearChecked || !anyChecked ? 0 : -1}
          style={clearStyle ?? undefined}
          className={`grid aspect-[4/3] cursor-pointer place-items-center rounded-[var(--radius-sm)] border-2 bg-hover text-fg hover:bg-selected-muted ${clearChecked ? "border-accent" : "border-transparent"}`}
          onClick={() => pick(cleared)}
        >
          <ProhibitIcon size={26} className="block" />
        </button>
        {BACKGROUND_PRESETS.map((preset) => (
          <Swatch
            key={preset.id}
            name={preset.name}
            checked={checkedPreset === preset.id}
            style={{ background: preset.css }}
            onPick={() => pick({ kind: "preset", id: preset.id })}
          />
        ))}
        {images.map((image, i) => {
          const name = image.name ?? `Picture ${i + 1}`;
          return (
            <PictureSwatch
              key={image.file}
              image={image}
              name={name}
              checked={checkedPicture === image.file}
              onPick={() => pick({ kind: "image", file: image.file })}
              onRemove={() => void remove(image, name)}
            />
          );
        })}
        {importing && (
          <div
            role="status"
            aria-label="Adding the picture"
            className="grid aspect-[4/3] place-items-center rounded-[var(--radius-sm)] border-2 border-border text-fg-muted"
          >
            <Spinner size={18} />
          </div>
        )}
        <button
          type="button"
          aria-label="Choose a picture"
          title="Choose a picture"
          disabled={importing}
          className="grid aspect-[4/3] cursor-pointer place-items-center rounded-[var(--radius-sm)] border border-dashed border-border-strong bg-transparent text-fg-muted hover:bg-hover disabled:cursor-default"
          onClick={() => void choosePicture()}
        >
          <ImageIcon size={18} />
        </button>
      </div>
      {undo && (
        <div
          role="status"
          className="flex items-center gap-2 pt-2 text-[length:var(--fs-sm)] text-fg-muted"
        >
          <span className="min-w-0 truncate">Removed {undo.name}</span>
          <button
            type="button"
            className="cursor-pointer border-none bg-transparent p-0 font-semibold text-accent-text hover:underline"
            onClick={() => void takeBack()}
          >
            Undo
          </button>
        </div>
      )}
      {error && (
        <div role="alert" className="pt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </div>
  );
}

/** A picture picked before, with the button that deletes it. */
function PictureSwatch({
  image,
  name,
  checked,
  onPick,
  onRemove,
}: {
  image: BackgroundImage;
  name: string;
  checked: boolean;
  onPick: () => void;
  onRemove: () => void;
}) {
  const src = useImageSrc(image.path);
  return (
    <div className="group relative grid">
      <Swatch
        name={name}
        checked={checked}
        style={backgroundStyle({ kind: "image", file: image.file }, src, 0)}
        onPick={onPick}
      />
      <button
        type="button"
        className="hover-reveal absolute -top-1.5 -right-1.5 flex size-4 cursor-pointer items-center justify-center rounded-full border border-border-strong bg-elevated p-0 text-fg-muted hover:text-fg"
        aria-label={`Remove ${name}`}
        title={`Remove ${name}`}
        onClick={onRemove}
      >
        <XIcon size={9} />
      </button>
    </div>
  );
}

function Swatch({
  name,
  checked,
  style,
  onPick,
}: {
  name: string;
  checked: boolean;
  style: CSSProperties | null;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-label={name}
      title={name}
      tabIndex={checked ? 0 : -1}
      style={style ?? undefined}
      className={`grid aspect-[4/3] place-items-center rounded-[var(--radius-sm)] border-2 bg-editor ${checked ? "border-accent" : "border-border"} cursor-pointer hover:border-border-strong`}
      onClick={onPick}
    />
  );
}
