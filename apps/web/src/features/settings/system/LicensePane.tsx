import { useState } from "react";
import type { LicenseStatus } from "~/api";
import { removeLicenseKey, setLicenseKey, startTrial, useLicense } from "~/hooks/useLicense";
import { TEXTAREA, TokenRow } from "~/features/settings/controls";
import { errorText } from "~/lib/util";

const PLAN_NAMES: Record<LicenseStatus["plan"], string> = { free: "Free", basic: "Basic" };

/** When a key runs out, or ran out, as "Renews Nov 4, 2027" or "Expired Nov 4, 2026". */
function expiryText(status: LicenseStatus): string | undefined {
  if (status.expiresAt === null) return undefined;
  const date = new Date(status.expiresAt).toLocaleDateString(undefined, { dateStyle: "medium" });
  return `${status.expired ? "Expired" : "Renews"} ${date}`;
}

/** The plan in effect, and the license key that unlocks a paid one. */
export default function LicensePane() {
  const status = useLicense();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      setKey("");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  if (!status) return null;
  const note = status.paused ? "Paused: sign in with this email to use it" : expiryText(status);
  const desc = [status.email, note].filter(Boolean).join(" · ") || undefined;
  return (
    <>
      <TokenRow title={`${PLAN_NAMES[status.plan]} plan`} desc={desc}>
        {status.email !== null ? (
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void run(removeLicenseKey)}
          >
            Remove key
          </button>
        ) : (
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void run(startTrial)}
          >
            Activate 14-day trial
          </button>
        )}
      </TokenRow>
      <form
        className="flex flex-col gap-2 border-b border-border py-2.5"
        onSubmit={(e) => {
          e.preventDefault();
          if (key.trim()) void run(() => setLicenseKey(key));
        }}
      >
        <label htmlFor="license-key" className="font-semibold">
          License key
        </label>
        <textarea
          id="license-key"
          className={`${TEXTAREA} font-[family-name:var(--font-mono)]`}
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        {error && (
          <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
            {error}
          </div>
        )}
        <button type="submit" className="btn btn-primary self-start" disabled={busy || !key.trim()}>
          Activate
        </button>
      </form>
    </>
  );
}
