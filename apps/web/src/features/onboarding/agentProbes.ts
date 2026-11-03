import { useCallback, useEffect, useState } from "react";
import { api, type AgentStatus, type CliKind } from "~/api";
import { CLI_KINDS as AGENT_KINDS } from "~/lib/util";

export { AGENT_KINDS };

export interface AgentProbe {
  checking: boolean;
  status: AgentStatus | null;
  failed: boolean;
}

export type AgentProbes = Record<CliKind, AgentProbe>;

/** An agent is usable only when it is installed and signed in. */
export function isReady(probe: AgentProbe): boolean {
  return probe.status !== null && probe.status.installed && probe.status.signedIn;
}

const CHECKING: AgentProbe = { checking: true, status: null, failed: false };

const allChecking = () => Object.fromEntries(AGENT_KINDS.map((k) => [k, CHECKING])) as AgentProbes;

/** Detects all agents in parallel on mount; `check` re-runs detection, skipping the cache, so a
 * CLI installed during setup shows at once. */
export function useAgentProbes() {
  const [probes, setProbes] = useState<AgentProbes>(allChecking);

  const check = useCallback(() => {
    setProbes(
      (p) =>
        Object.fromEntries(AGENT_KINDS.map((k) => [k, { ...p[k], checking: true }])) as AgentProbes,
    );
    for (const kind of AGENT_KINDS) {
      api.detectAgent(kind, true).then(
        (status) =>
          setProbes((p) => ({ ...p, [kind]: { checking: false, status, failed: false } })),
        () => setProbes((p) => ({ ...p, [kind]: { checking: false, status: null, failed: true } })),
      );
    }
  }, []);

  useEffect(() => {
    check();
  }, [check]);

  return { probes, check, checking: AGENT_KINDS.some((k) => probes[k].checking) };
}
