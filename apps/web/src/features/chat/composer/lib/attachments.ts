import { type Attachment } from "~/api";
import type { MentionItem } from "~/features/chat/composer/types";
import { inlineImagePaths } from "~/features/chat/lib/composerExtensions";

/**
 * How attachments meet the draft. A file dropped on the composer is written into the text as a
 * mention; a mention that is still in the text when the message is sent is attached to it as well,
 * so it shows above the message rather than only inside it. Pure, so the rules can be read and
 * tested without a composer.
 */

/** What goes into the draft for an attachment: its path, relative to the project when inside it. */
export function mentionFor(attachment: Attachment, root: string | null): MentionItem {
  const { path, name, kind } = attachment;
  if (kind === "image") return { id: path, label: name, image: true };
  return {
    id: path,
    label: root && path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path,
  };
}

/** The local path of a file:// URI, such as one a file manager copies. */
export function pathOfFileUri(uri: string): string | null {
  let path: string;
  try {
    path = decodeURIComponent(new URL(uri).pathname);
  } catch {
    return null;
  }
  // Windows paths come as /C:/…, without the drive's leading slash.
  return /^\/[A-Za-z]:/.test(path) ? path.slice(1) : path;
}

/** Whether the draft still mentions this file. */
export function mentions(text: string, file: Attachment, root: string | null): boolean {
  return text.includes(`@${mentionFor(file, root).label}`);
}

/** A message's attachments with the images written into its text, and the dropped files it still
 * mentions, added; they show above the sent message. */
export function withInline(
  text: string,
  files: Attachment[],
  dropped: Attachment[],
  root: string | null,
): Attachment[] {
  const added = [
    ...dropped.filter((f) => mentions(text, f, root)),
    ...inlineImagePaths(text).map((path): Attachment => ({
      kind: "image",
      path,
      name: path.slice(path.lastIndexOf("/") + 1),
    })),
  ];
  return [
    ...files,
    ...added.filter(
      (a, i) =>
        !files.some((f) => f.path === a.path) && added.findIndex((b) => b.path === a.path) === i,
    ),
  ];
}

/** A message's attachments without the ones its text holds, which the draft shows in line. */
export function withoutInline(
  text: string,
  files: Attachment[],
  root: string | null,
): Attachment[] {
  const images = inlineImagePaths(text);
  return files.filter((f) =>
    f.kind === "image" ? !images.includes(f.path) : !mentions(text, f, root),
  );
}

/** The file's bytes, base64-encoded, for sending to the backend. */
export function base64Of(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1] ?? "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}
