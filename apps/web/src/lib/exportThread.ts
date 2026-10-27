import { save } from "@tauri-apps/plugin-dialog";
import { AGENT_LABELS, api, type Message, type Thread } from "~/api";
import { displayTitle } from "~/lib/util";

/** A reply as Markdown: its text and plans, with each tool call as a list line. */
function replyMarkdown(msg: Message): string {
  if (msg.parts.length === 0) return msg.text.trim();
  const blocks: string[] = [];
  for (const part of msg.parts) {
    if (part.type === "text" || part.type === "plan") {
      if (part.text.trim()) blocks.push(part.text.trim());
    } else if (part.type === "tool") {
      const tool = msg.tools.find((t) => t.id === part.id);
      if (!tool) continue;
      const line = `- \`${tool.name}\`${tool.summary ? ` ${tool.summary}` : ""}`;
      // Consecutive tool calls form one list.
      const last = blocks.length - 1;
      if (last >= 0 && blocks[last].startsWith("- `")) blocks[last] += `\n${line}`;
      else blocks.push(line);
    }
  }
  return blocks.join("\n\n");
}

/** The thread's transcript as Markdown. */
function threadMarkdown(thread: Thread): string {
  const agent = AGENT_LABELS[thread.agent];
  const out = [`# ${displayTitle(thread.title)}`];
  for (const msg of thread.messages) {
    const body = msg.role === "user" ? msg.text.trim() : replyMarkdown(msg);
    const error = msg.error ? `> ${msg.error.trim().replace(/\n/g, "\n> ")}` : "";
    if (!body && !error) continue;
    out.push(`## ${msg.role === "user" ? "You" : agent}`, ...[body, error].filter(Boolean));
  }
  return `${out.join("\n\n")}\n`;
}

/** A file name from the title: lowercase words joined by dashes. */
function fileName(title: string): string {
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || "chat"}.md`;
}

/** Asks where to save a thread, then writes its transcript there as Markdown. */
export async function exportThread(id: string): Promise<void> {
  const thread = await api.getThread(id);
  const path = await save({
    defaultPath: fileName(displayTitle(thread.title)),
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (path) await api.saveTextFile(path, threadMarkdown(thread));
}
