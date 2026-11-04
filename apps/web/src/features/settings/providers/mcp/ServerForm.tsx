import { useState, type ReactNode } from "react";
import { api, type McpServer } from "~/api";
import { INPUT } from "~/features/settings/controls";
import { errorText } from "~/lib/util";
import {
  type Draft,
  pairsText,
  parsePairs,
  parseServers,
  target,
  words,
} from "~/features/settings/providers/mcp/servers";

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[length:var(--fs-sm)] font-medium text-fg-muted">{label}</span>
      {children}
    </label>
  );
}

export default function ServerForm({ draft, onDone }: { draft: Draft; onDone: () => void }) {
  const [name, setName] = useState(draft.name);
  const [line, setLine] = useState(draft.previous === null ? "" : target(draft.server));
  const isUrl = /^https?:\/\//i.test(line.trim());
  const isJson = line.trim().startsWith("{");
  const sep = isUrl ? ":" : "=";
  const [pairs, setPairs] = useState(
    draft.server.url ? pairsText(draft.server.headers, ": ") : pairsText(draft.server.env, "="),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      if (isJson) {
        for (const [n, s] of parseServers(line, name)) await api.saveMcpServer(n, s);
      } else {
        const server: McpServer = isUrl
          ? { url: line.trim(), headers: parsePairs(pairs, sep) }
          : (() => {
              const [command, ...args] = words(line);
              return { command, args, env: parsePairs(pairs, sep) };
            })();
        server.disabled = draft.server.disabled;
        server.disabledTools = draft.server.disabledTools;
        server.share = draft.server.share;
        await api.saveMcpServer(name, server, draft.previous);
      }
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await api.removeMcpServer(draft.previous ?? name);
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 py-2.5">
      <Field label="Name">
        <input
          className={INPUT}
          value={name}
          autoFocus={!name}
          spellCheck={false}
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <Field label={isJson ? "JSON" : "Command or URL"}>
        {isJson ? (
          <textarea
            className={`${INPUT} h-40 resize-y py-1.5 font-mono`}
            value={line}
            autoFocus
            spellCheck={false}
            onChange={(e) => setLine(e.target.value)}
          />
        ) : (
          <input
            className={`${INPUT} font-mono`}
            value={line}
            placeholder='npx -y @playwright/mcp@latest, https://…, or {"mcpServers": …}'
            spellCheck={false}
            onChange={(e) => setLine(e.target.value)}
          />
        )}
      </Field>
      {!isJson && (
        <Field label={isUrl ? "Headers" : "Environment"}>
          <textarea
            className={`${INPUT} h-20 resize-y py-1.5 font-mono`}
            value={pairs}
            placeholder={isUrl ? "Authorization: Bearer …" : "API_KEY=…"}
            spellCheck={false}
            onChange={(e) => setPairs(e.target.value)}
          />
        </Field>
      )}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      <div className="flex items-center gap-2 border-t border-border pt-3">
        <button
          className="btn btn-primary"
          disabled={busy || !line.trim() || (!isJson && !name.trim())}
          onClick={() => void save()}
        >
          {busy ? "Saving…" : isJson ? "Import" : "Save"}
        </button>
        <button className="btn" disabled={busy} onClick={onDone}>
          Cancel
        </button>
        {draft.previous !== null && (
          <button
            className={`btn ml-auto ${confirmRemove ? "text-danger" : ""}`}
            disabled={busy}
            onClick={() => (confirmRemove ? void remove() : setConfirmRemove(true))}
          >
            {confirmRemove ? "Remove server" : "Remove"}
          </button>
        )}
      </div>
    </div>
  );
}
