import { invoke } from "~/api/invoke";
import type { AgentKind, ProviderUsage } from "~/api/types";

export const usageApi = {
  /** Subscription usage limits. Cached by the backend; `refresh` asks for new
   * numbers, which it still throttles to avoid provider rate limits. */
  getUsage: (kind: AgentKind, refresh = false) =>
    invoke<ProviderUsage>("get_usage", { kind, refresh }),
};
