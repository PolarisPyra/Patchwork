import { useEffect, useState } from "react";
import { HardDrivesIcon } from "@phosphor-icons/react";
import { api, type SshHost, type SshTest } from "~/api";
import Spinner from "~/components/Spinner";
import { errorText } from "~/lib/util";

// Kept across mounts so reopening the pane still shows the last result of each test.
const testCache = new Map<string, SshTest>();

/** `user@host:port`, leaving out what the config does not set. */
function address(h: SshHost): string {
  const host = h.hostname ?? h.name;
  return `${h.user ? `${h.user}@` : ""}${host}${h.port ? `:${h.port}` : ""}`;
}

function HostRow({ host }: { host: SshHost }) {
  const [result, setResult] = useState<SshTest | undefined>(testCache.get(host.name));
  const [busy, setBusy] = useState(false);
  const test = () => {
    setBusy(true);
    const apply = (next: SshTest) => {
      testCache.set(host.name, next);
      setResult(next);
      setBusy(false);
    };
    api
      .testSshHost(host.name)
      .then(apply, (e) => apply({ ok: false, latencyMs: null, error: errorText(e) }));
  };
  const line = result
    ? result.ok
      ? `${address(host)} · ${result.latencyMs} ms`
      : (result.error ?? "Failed")
    : address(host);
  return (
    <div className="flex items-center gap-3 border-b border-border py-2.5">
      <HardDrivesIcon size={20} className="flex-none text-fg-muted" />
      <div className="min-w-0 flex-1">
        <div className="font-semibold">{host.name}</div>
        <div
          className={`truncate text-[length:var(--fs-sm)] ${result && !result.ok ? "text-danger" : "font-mono text-fg-muted"}`}
          role={result ? "status" : undefined}
        >
          {line}
        </div>
      </div>
      {result && (
        <span className={`badge ${result.ok ? "badge-success" : "badge-danger"}`}>
          {result.ok ? "Connected" : "Failed"}
        </span>
      )}
      <button className="btn" aria-label={`Test ${host.name}`} disabled={busy} onClick={test}>
        {busy && <Spinner size={13} />}
        Test
      </button>
    </div>
  );
}

/**
 * Settings → SSH: the hosts in `~/.ssh/config`, which the Add Project dialog offers as machines.
 * Test logs in with keys only, so it shows whether Patchwork could reach a host unattended. The
 * config is the user's own: it is opened in their editor, and Patchwork never writes it.
 */
export default function SshPane() {
  const [hosts, setHosts] = useState<SshHost[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.listSshHosts().then(setHosts, (e) => setError(errorText(e)));
  }, []);
  const openConfig = () => {
    setError(null);
    api.openSshConfig().catch((e) => setError(errorText(e)));
  };
  return (
    <>
      {hosts === null && !error && (
        <div className="flex items-center gap-2 py-2.5 text-fg-muted">
          <Spinner /> Reading ~/.ssh/config…
        </div>
      )}
      {hosts?.length === 0 && (
        <div className="border-b border-border py-2.5 text-fg-muted">No hosts in ~/.ssh/config</div>
      )}
      {hosts?.map((h) => (
        <HostRow key={h.name} host={h} />
      ))}
      <div className="py-2.5">
        <button className="btn" onClick={openConfig}>
          Open ~/.ssh/config
        </button>
      </div>
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
