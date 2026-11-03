import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type Attachment } from "~/api";
import {
  base64Of,
  mentionFor,
  pathOfFileUri,
  withInline,
  withoutInline,
} from "~/features/chat/composer/lib/attachments";
import type { DropMentions } from "~/features/chat/composer/types";
import { errorText } from "~/lib/util";

/** A pasted text at least this long is attached as a file instead of going into the draft. */
const PASTE_ATTACH_CHARS = 10_000;

/** A queued message being edited in the composer, in place of the draft. */
export interface QueuedEdit {
  /** The chat the message is queued in. */
  chat: string;
  id: string;
  text: string;
  attachments: Attachment[];
}

// Fallbacks that keep one identity, so the memoized composer sees no change.
const NO_ATTACHMENTS: Attachment[] = [];

const DRAFTS_KEY = "patchwork.drafts";

/** The unsent drafts, by chat, kept across reloads and restarts. */
function savedDrafts(): Record<string, string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(DRAFTS_KEY) ?? "{}");
    if (!saved || typeof saved !== "object") return {};
    return Object.fromEntries(
      Object.entries(saved).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    );
  } catch {
    return {};
  }
}

/**
 * The text and files being composed for one chat, and everything that puts files into them:
 * attachments picked or dropped, files and images pasted, and the draft's per-chat memory.
 *
 * Drafts and attachments are kept per thread ("new" when nothing is selected), so switching chats
 * brings back what was half-written in each. `rootRef` is the chat's box, which the drop listener
 * hit-tests against, and `dropMentions` is how the composer places a mention where it landed.
 */
export function useComposerDraft({
  threadKey,
  projectPath,
  onError,
}: {
  threadKey: string;
  projectPath: string | null;
  /** Reports a failed attach against the chat it was meant for. */
  onError: (chat: string, text: string) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>(savedDrafts);
  useEffect(() => {
    const unsent = Object.entries(drafts).filter(([, text]) => text.trim());
    localStorage.setItem(DRAFTS_KEY, JSON.stringify(Object.fromEntries(unsent)));
  }, [drafts]);
  const [pending, setPending] = useState<Record<string, Attachment[]>>({});
  const [heldEdit, setQueuedEdit] = useState<QueuedEdit | null>(null);
  // An edit shows only in its own chat, so a switch never hands it to the next chat's composer.
  const queuedEdit = heldEdit?.chat === threadKey ? heldEdit : null;
  const [dropping, setDropping] = useState(false);

  const rootRef = useRef<HTMLDivElement>(null);
  const dropMentions = useRef<DropMentions | null>(null);
  // The listeners below are set up once, so they read these as they are now.
  const keyRef = useRef(threadKey);
  keyRef.current = threadKey;
  const projectPathRef = useRef(projectPath);
  projectPathRef.current = projectPath;
  const queuedEditRef = useRef(queuedEdit);
  queuedEditRef.current = queuedEdit;
  // Files dropped into the draft as mentions. A message that still mentions one sends it as an
  // attachment too, so it shows above the message like any attached file.
  const droppedFiles = useRef<Attachment[]>([]);

  // A new chat starts with an empty edit slot; the draft and attachments are already kept apart
  // by key, so they need nothing doing.
  useEffect(() => setQueuedEdit(null), [threadKey]);

  const draft = drafts[threadKey] ?? "";
  const setDraft = (text: string) => setDrafts((d) => ({ ...d, [threadKey]: text }));
  const attachments = pending[threadKey] ?? NO_ATTACHMENTS;
  const setAttachments = (next: Attachment[]) => setPending((p) => ({ ...p, [threadKey]: next }));

  const composerDraft = queuedEdit?.text ?? draft;
  const composerAttachments = queuedEdit?.attachments ?? attachments;
  const setComposerDraft = (text: string) => {
    if (queuedEdit) setQueuedEdit((edit) => (edit ? { ...edit, text } : edit));
    else setDraft(text);
  };
  const setComposerAttachments = (next: Attachment[]) => {
    if (queuedEdit) setQueuedEdit((edit) => (edit ? { ...edit, attachments: next } : edit));
    else setAttachments(next);
  };

  /** A message being sent, with the files it still mentions attached to it. */
  const sentWith = (text: string, files: Attachment[]) =>
    withInline(text, files, droppedFiles.current, projectPathRef.current);

  /** Puts a failed send's text and files back, unless the chat has been written in since. */
  const restoreDraft = (chat: string, text: string, files: Attachment[]) => {
    setDrafts((d) => (d[chat] ? d : { ...d, [chat]: text }));
    setPending((p) => (p[chat]?.length ? p : { ...p, [chat]: files }));
  };

  /** Empties a chat's draft once its message is sent, keeping files attached to it since. */
  const clearSent = (chat: string, files: Attachment[]) => {
    setDrafts((d) => ({ ...d, [chat]: "" }));
    setPending((p) => ({ ...p, [chat]: (p[chat] ?? []).filter((f) => !files.includes(f)) }));
  };

  /** What the draft keeps once the message leaves: the files its text no longer holds. */
  const draftWithout = (text: string, files: Attachment[]) => {
    droppedFiles.current.push(...files.filter((f) => f.kind !== "image"));
    return withoutInline(text, files, projectPathRef.current);
  };

  // Adds files and folders, given as paths, to what the composer shows: the queued message being
  // edited, or else the draft. Drops and the composer's attach buttons both come through here.
  async function attach(paths: string[], mention?: ReturnType<DropMentions>) {
    const sentKey = keyRef.current;
    try {
      let found = await Promise.all(paths.map((path) => api.attachmentForPath(path)));
      const root = projectPathRef.current;
      // Whatever is dropped on the composer is written into the draft where it landed; anything
      // dropped elsewhere or picked goes in at the caret. They stay attachments only when the
      // draft cannot be written, or the composer has moved to another chat since.
      const place = keyRef.current === sentKey ? (mention ?? dropMentions.current?.(null)) : null;
      if (place) {
        place(found.map((a) => mentionFor(a, root)));
        droppedFiles.current.push(...found.filter((a) => a.kind !== "image"));
        found = [];
      }
      const withFresh = (current: Attachment[]) => [
        ...current,
        ...found.filter((a) => !current.some((c) => c.path === a.path)),
      ];
      if (queuedEditRef.current && keyRef.current === sentKey) {
        setQueuedEdit((edit) =>
          edit ? { ...edit, attachments: withFresh(edit.attachments) } : edit,
        );
      } else {
        setPending((prev) => ({ ...prev, [sentKey]: withFresh(prev[sentKey] ?? []) }));
      }
    } catch (e) {
      onError(sentKey, errorText(e));
    }
  }

  // Tauri hands dropped files over as paths (HTML drop events carry none), so drops are
  // listened for on the webview and kept only when they land on the chat.
  useEffect(() => {
    // Positions are physical pixels; the chat's rect is in CSS pixels.
    function cssPoint(position: { x: number; y: number }) {
      const scale = window.devicePixelRatio;
      return { x: position.x / scale, y: position.y / scale };
    }
    function overChat(position: { x: number; y: number }) {
      const el = rootRef.current;
      if (!el) return false;
      const r = el.getBoundingClientRect();
      const { x, y } = cssPoint(position);
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setDropping(false);
        } else if (payload.type === "drop") {
          setDropping(false);
          if (overChat(payload.position)) {
            const mention = dropMentions.current?.(cssPoint(payload.position)) ?? undefined;
            void attach(payload.paths, mention);
          }
        } else {
          setDropping(overChat(payload.position));
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
    // `attach` only reads refs and stable setters, so the listener set up once sees the current
    // draft.
  }, []);

  // Files and folders copied in a file manager go in like dropped ones, and pasted images are
  // saved to files and go in at the caret. The event stops here, so the composer does not also
  // try to paste them as text.
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    async function attachPasted(images: File[], copiedPaths: string[]) {
      const sentKey = keyRef.current;
      try {
        // Webviews give a pasted file's contents at most, never its path, so the paths are read
        // from the system clipboard.
        const clipboardPaths = await api.clipboardFiles();
        const paths = clipboardPaths.length > 0 ? clipboardPaths : copiedPaths;
        if (paths.length > 0) {
          await attach(paths);
          return;
        }
        // The Linux webview gives no file for an image copied on its own, such as a screenshot,
        // so the image is read from the system clipboard instead.
        const found =
          images.length > 0
            ? await Promise.all(
                images.map(async (image) =>
                  api.savePastedImage(image.type, await base64Of(image), image.name),
                ),
              )
            : [await api.pasteClipboardImage()].filter((image) => image !== null);
        if (found.length === 0) return;
        const caret = dropMentions.current?.(null);
        if (caret) caret(found.map((a) => mentionFor(a, null)));
        else setPending((prev) => ({ ...prev, [sentKey]: [...(prev[sentKey] ?? []), ...found] }));
      } catch (e) {
        onError(sentKey, errorText(e));
      }
    }
    async function attachPastedText(text: string) {
      const sentKey = keyRef.current;
      try {
        const found = await api.savePastedText(text);
        setPending((prev) => ({ ...prev, [sentKey]: [...(prev[sentKey] ?? []), found] }));
      } catch (e) {
        onError(sentKey, errorText(e));
      }
    }
    function onPaste(event: ClipboardEvent) {
      const files = Array.from(event.clipboardData?.files ?? []);
      const text = event.clipboardData?.getData("text/plain") ?? "";
      // A copy from a file manager lists its files as file:// URIs, which some webviews pass on
      // only as text.
      const uris = (event.clipboardData?.getData("text/uri-list") || text)
        .split(/\r?\n/)
        .filter((line) => line && !line.startsWith("#"));
      const copiedPaths =
        uris.length > 0 && uris.every((line) => line.startsWith("file://"))
          ? uris.map(pathOfFileUri).filter((path) => path !== null)
          : [];
      if (files.length > 0 || copiedPaths.length > 0 || !text) {
        event.preventDefault();
        event.stopPropagation();
        void attachPasted(
          files.filter((file) => file.type.startsWith("image/")),
          copiedPaths,
        );
        return;
      }
      // A long paste, such as a full log, is attached as a file the agent opens, rather than
      // filling the composer.
      const inComposer = event.target instanceof Element && event.target.closest(".ProseMirror");
      if (!inComposer || text.trim().length < PASTE_ATTACH_CHARS) return;
      event.preventDefault();
      event.stopPropagation();
      void attachPastedText(text);
    }
    el.addEventListener("paste", onPaste, true);
    return () => el.removeEventListener("paste", onPaste, true);
    // As above: the listener reads refs, so it is set up once.
  }, []);

  return {
    draft,
    setDraft,
    attachments,
    setAttachments,
    queuedEdit,
    setQueuedEdit,
    composerDraft,
    composerAttachments,
    setComposerDraft,
    setComposerAttachments,
    sentWith,
    draftWithout,
    restoreDraft,
    clearSent,
    attach,
    dropping,
    dropMentions,
    rootRef,
  };
}
