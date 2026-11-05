import { useEffect, useState, type InputHTMLAttributes } from "react";
import {
  AppleLogoIcon,
  DesktopIcon,
  EyeIcon,
  EyeSlashIcon,
  LinuxLogoIcon,
  TrashIcon,
  WindowsLogoIcon,
  type Icon,
} from "@phosphor-icons/react";
import { api, type Account, type AccountDevice } from "~/api";
import Spinner from "~/components/Spinner";
import LicensePane from "~/features/settings/system/LicensePane";
import { refreshLicense } from "~/hooks/useLicense";
import {
  Segmented,
  SectionHeading,
  TextField,
  TokenRow,
  TokenSwitch,
} from "~/features/settings/controls";
import { errorText } from "~/lib/util";

const PLATFORMS: Record<string, { name: string; Icon: Icon }> = {
  linux: { name: "Linux", Icon: LinuxLogoIcon },
  macos: { name: "macOS", Icon: AppleLogoIcon },
  windows: { name: "Windows", Icon: WindowsLogoIcon },
};

/** The settings text input, with a neutral border on focus. */
const FIELD =
  "h-7 w-[260px] min-w-0 rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2 text-[length:var(--fs-sm)] text-fg outline-none focus:border-fg-subtle";

const date = (iso: string) => new Date(iso).toLocaleDateString(undefined, { dateStyle: "medium" });

// What the pane last showed, so it opens as it was left while it reads everything again.
let last: { account: Account | null; devices: AccountDevice[] | null } | undefined;

/** The optional Patchwork account: signing in, and the account once signed in. */
export default function AccountPane() {
  // undefined while it is first read; null when signed out.
  const [account, setAccount] = useState<Account | null | undefined>(last?.account);
  const [devices, setDevices] = useState<AccountDevice[] | null>(last?.devices ?? null);
  const [error, setError] = useState<string | null>(null);
  // The account, its devices and the plan are read before any of them shows, so the pane fills in
  // at once instead of growing row by row.
  const load = () => {
    setError(null);
    api.getAccount().then(
      async (a) => {
        const [d] = await Promise.all([
          a ? api.accountDevices().catch(() => null) : null,
          refreshLicense().catch(() => {}),
        ]);
        setDevices(d);
        setAccount(a);
      },
      (e: unknown) => setError(errorText(e)),
    );
  };
  useEffect(load, []);
  useEffect(() => {
    if (account !== undefined) last = { account, devices };
  }, [account, devices]);
  // Another account's devices are not this one's.
  const changeAccount = (a: Account | null) => {
    if (a?.email !== account?.email) setDevices(null);
    setAccount(a);
  };
  // A key's plan holds only while signed in to its account, so it is read again on each change.
  const signedInAs = account === undefined ? undefined : (account?.email ?? null);
  useEffect(() => {
    if (signedInAs !== undefined) refreshLicense().catch(() => {});
  }, [signedInAs]);
  if (error)
    return (
      <TokenRow title="Account">
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
        <button type="button" className="btn" onClick={load}>
          Try again
        </button>
      </TokenRow>
    );
  if (account === undefined)
    return (
      <div role="status" aria-label="Loading the account" className="py-2.5">
        <Spinner size={13} />
      </div>
    );
  return account ? (
    <SignedIn
      account={account}
      onAccount={changeAccount}
      devices={devices}
      onDevices={setDevices}
    />
  ) : (
    <SignedOut onSignedIn={changeAccount} />
  );
}

/** Runs an action with the pane busy, keeping the reason it failed. */
function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<void>, failed?: () => void) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e));
      failed?.();
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, run, clear: () => setError(null) };
}

function ErrorText({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
      {error}
    </div>
  );
}

/** A labelled row with a text input on the right. */
function FieldRow({ label, ...input }: { label: string } & InputHTMLAttributes<HTMLInputElement>) {
  return (
    <TokenRow title={label}>
      <input aria-label={label} className={FIELD} spellCheck={false} {...input} />
    </TokenRow>
  );
}

/** The service's rule for a new password, counted in characters as it counts them. */
const PASSWORD_MIN = 12;
const PASSWORD_MAX = 128;

/** What is wrong with a new password, or null when it will be taken. */
function passwordProblem(password: string): string | null {
  const chars = [...password].length;
  if (chars < PASSWORD_MIN) {
    const more = PASSWORD_MIN - chars;
    return `At least ${PASSWORD_MIN} characters · ${more} more`;
  }
  if (chars > PASSWORD_MAX) return `At most ${PASSWORD_MAX} characters`;
  return null;
}

function PasswordRow({
  label,
  problem,
  ...input
}: {
  label: string;
  /** Shown under the field as the password is typed. */
  problem?: string | null;
} & InputHTMLAttributes<HTMLInputElement>) {
  const [shown, setShown] = useState(false);
  const hintId = input.id && `${input.id}-problem`;
  return (
    <TokenRow title={label}>
      <div className="flex flex-col items-end">
        <div className="relative">
          <input
            aria-label={label}
            aria-invalid={problem ? true : undefined}
            aria-describedby={problem ? hintId : undefined}
            className={`${FIELD} pr-8`}
            spellCheck={false}
            {...input}
            type={shown ? "text" : "password"}
          />
          <button
            type="button"
            className="btn-icon absolute top-0 right-0 text-fg-muted hover:text-fg"
            aria-label="Show password"
            aria-pressed={shown}
            onClick={() => setShown(!shown)}
          >
            {shown ? <EyeSlashIcon size={14} /> : <EyeIcon size={14} />}
          </button>
        </div>
        <div
          id={hintId}
          role="status"
          className="text-[length:var(--fs-xs)] text-fg-muted not-empty:mt-1"
        >
          {problem}
        </div>
      </div>
    </TokenRow>
  );
}

const CODE_LENGTH = 6;

/**
 * The emailed code as one box per digit. Typing moves to the next box, Backspace to the one
 * before, and pasting or the system's autofill spreads the digits over the boxes from the one in
 * focus. `value` holds a space for an empty box; `onComplete` is called once every box is filled.
 */
function CodeInput({
  value,
  onChange,
  onComplete,
}: {
  value: string;
  onChange: (value: string) => void;
  onComplete: (code: string) => void;
}) {
  const boxes = Array.from({ length: CODE_LENGTH }, (_, i) => value[i]?.trim() ?? "");
  const refs: (HTMLInputElement | null)[] = [];
  const focus = (i: number) => refs[Math.max(0, Math.min(i, CODE_LENGTH - 1))]?.focus();
  const fill = (at: number, digits: string) => {
    const next = [...boxes];
    for (const [k, d] of [...digits].entries()) if (at + k < CODE_LENGTH) next[at + k] = d;
    const code = next.map((d) => d || " ").join("");
    onChange(code.trimEnd());
    focus(at + digits.length);
    if (/^\d+$/.test(code) && code.length === CODE_LENGTH) onComplete(code);
  };
  return (
    <div role="group" aria-label="Code" className="flex gap-1.5">
      {boxes.map((digit, i) => (
        <input
          key={i}
          ref={(el) => {
            refs[i] = el;
          }}
          aria-label={`Digit ${i + 1} of ${CODE_LENGTH}`}
          className={`h-10 w-9 rounded-[var(--radius-sm)] border border-border-strong bg-editor p-0 text-center font-[family-name:var(--font-mono)] text-[length:var(--fs-lg)] text-fg outline-none focus:border-fg-subtle ${i === CODE_LENGTH / 2 ? "ml-2" : ""}`}
          inputMode="numeric"
          autoComplete={i === 0 ? "one-time-code" : "off"}
          autoFocus={i === 0}
          value={digit}
          onChange={(e) => {
            const digits = e.target.value.replace(/\D/g, "");
            if (digits) fill(i, digits.length > 1 && digit ? digits.replace(digit, "") : digits);
          }}
          onPaste={(e) => {
            e.preventDefault();
            const digits = e.clipboardData.getData("text").replace(/\D/g, "");
            if (digits) fill(i, digits.slice(0, CODE_LENGTH - i));
          }}
          onKeyDown={(e) => {
            if (/^\d$/.test(e.key)) {
              e.preventDefault();
              fill(i, e.key);
            } else if (e.key === "Backspace") {
              e.preventDefault();
              const at = digit ? i : i - 1;
              if (at < 0) return;
              const next = [...boxes];
              next[at] = "";
              onChange(
                next
                  .map((d) => d || " ")
                  .join("")
                  .trimEnd(),
              );
              focus(at);
            } else if (e.key === "ArrowLeft") {
              e.preventDefault();
              focus(i - 1);
            } else if (e.key === "ArrowRight") {
              e.preventDefault();
              focus(i + 1);
            }
          }}
        />
      ))}
    </div>
  );
}

type Step = "signIn" | "signInCode" | "register" | "registerCode" | "reset" | "resetCode";

const STEPS: Record<Step, { title: string; submit: string }> = {
  signIn: { title: "Sign in", submit: "Sign in" },
  signInCode: { title: "Sign in", submit: "Verify" },
  register: { title: "Create account", submit: "Send code" },
  registerCode: { title: "Create account", submit: "Create account" },
  reset: { title: "Reset password", submit: "Send code" },
  resetCode: { title: "Reset password", submit: "Set password" },
};

function SignedOut({ onSignedIn }: { onSignedIn: (account: Account) => void }) {
  const [step, setStep] = useState<Step>("signIn");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [remember, setRemember] = useState(false);
  const [challenge, setChallenge] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const { busy, error, run, clear } = useAction();

  const go = (next: Step, message: string | null = null) => {
    setStep(next);
    setCode("");
    setConfirm("");
    setNotice(message);
    clear();
  };
  const codeSent = `Code sent to ${email.trim()}`;

  // `now` is the code just completed, which `code` holds only from the next render.
  const submit = (now = code) =>
    run(async () => {
      switch (step) {
        case "signIn": {
          const next = await api.accountLogin(email, password);
          if (next.status === "signedIn") return onSignedIn(next.account);
          setChallenge(next.challenge);
          return go("signInCode", codeSent);
        }
        case "signInCode":
          return onSignedIn(await api.accountLoginVerify(challenge, now, remember));
        case "register":
          await api.accountRegister(email);
          return go("registerCode", codeSent);
        case "registerCode":
          return onSignedIn(
            await api.accountRegisterFinish(email, now, password, name.trim() || null),
          );
        case "reset":
          await api.accountResetPassword(email);
          return go("resetCode", codeSent);
        case "resetCode":
          await api.accountResetPasswordFinish(email, now, password);
          setPassword("");
          return go("signIn", "Password changed");
      }
    });

  const first = step === "signIn" || step === "register";
  const askEmail = first || step === "reset";
  const newPassword = step === "register" || step === "resetCode";
  // A new password the service would refuse keeps the button off, so it is fixed before it is sent.
  const blocked = newPassword && (passwordProblem(password) !== null || confirm !== password);
  const { title, submit: submitLabel } = STEPS[step];
  return (
    <form
      noValidate
      aria-label={title}
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <div className="flex items-center justify-between pb-1">
        <SectionHeading>{title}</SectionHeading>
        {first && (
          <Segmented
            label="Account"
            value={step}
            options={[
              { value: "signIn", label: "Sign in" },
              { value: "register", label: "Create account" },
            ]}
            onChange={(next) => go(next)}
          />
        )}
      </div>
      {notice && (
        <div role="status" className="py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          {notice}
        </div>
      )}
      {askEmail && (
        <FieldRow
          label="Email"
          type="email"
          autoComplete="email"
          autoFocus
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
      )}
      {step.endsWith("Code") && (
        <TokenRow title="Code">
          <CodeInput
            value={code}
            onChange={setCode}
            onComplete={(now) => {
              // Signing in and creating the account need nothing else, so they go on; a reset asks for
              // the new password.
              if (step === "signInCode" || step === "registerCode") {
                if (!busy) void submit(now);
              } else document.getElementById("account-password")?.focus();
            }}
          />
        </TokenRow>
      )}
      {(step === "signIn" || newPassword) && (
        <PasswordRow
          id="account-password"
          label={step === "resetCode" ? "New password" : "Password"}
          autoComplete={newPassword ? "new-password" : "current-password"}
          minLength={newPassword ? PASSWORD_MIN : undefined}
          problem={newPassword && password ? passwordProblem(password) : null}
          maxLength={PASSWORD_MAX}
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      )}
      {newPassword && (
        <PasswordRow
          id="account-confirm"
          label="Confirm password"
          autoComplete="new-password"
          maxLength={PASSWORD_MAX}
          required
          problem={confirm && confirm !== password ? "Passwords don't match" : null}
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
        />
      )}
      {step === "register" && (
        <FieldRow
          label="Name"
          autoComplete="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      )}
      {step === "signInCode" && (
        <TokenRow title="Remember this computer">
          <TokenSwitch label="Remember this computer" on={remember} onChange={setRemember} />
        </TokenRow>
      )}
      <ErrorText error={error} />
      <div className="flex items-center justify-between gap-1.5 pt-3">
        {step === "signIn" ? (
          <button
            type="button"
            className="btn btn-ghost px-0 text-fg-muted hover:bg-transparent hover:text-fg"
            onClick={() => go("reset")}
          >
            Forgot password
          </button>
        ) : first ? (
          <span />
        ) : (
          <button
            type="button"
            className="btn btn-ghost px-0 text-fg-muted hover:bg-transparent hover:text-fg"
            onClick={() => go("signIn")}
          >
            Back
          </button>
        )}
        <div className="flex items-center gap-2">
          {busy && (
            <span role="status" aria-label="Working" className="inline-flex">
              <Spinner size={13} />
            </span>
          )}
          <button type="submit" className="btn btn-primary" disabled={busy || blocked}>
            {submitLabel}
          </button>
        </div>
      </div>
    </form>
  );
}

/** The account's initials. */
function Avatar({ account }: { account: Account }) {
  const words = (account.name ?? account.email).split(/[\s@.]+/).filter(Boolean);
  const initials = words
    .slice(0, account.name ? 2 : 1)
    .map((w) => w[0])
    .join("")
    .toUpperCase();
  return (
    <span
      aria-hidden="true"
      className="grid size-9 flex-none place-items-center rounded-full bg-selected font-semibold text-fg"
    >
      {initials || "?"}
    </span>
  );
}

function SignedIn({
  account,
  onAccount,
  devices,
  onDevices: setDevices,
}: {
  account: Account;
  onAccount: (account: Account | null) => void;
  /** null until they have been read. */
  devices: AccountDevice[] | null;
  onDevices: (devices: AccountDevice[]) => void;
}) {
  const [deleting, setDeleting] = useState(false);
  const [password, setPassword] = useState("");
  const { busy, error, run } = useAction();

  // A failure may mean the session has ended elsewhere, which shows the signed-out pane.
  const checkSession = () => {
    api.getAccount().then(
      (a) => a === null && onAccount(null),
      () => {},
    );
  };
  const act = (action: () => Promise<void>) => run(action, checkSession);
  // Reading the list is not an action, so the buttons stay enabled while it loads.
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const loadDevices = () => {
    setDevicesError(null);
    api.accountDevices().then(setDevices, (e: unknown) => {
      setDevicesError(errorText(e));
      checkSession();
    });
  };
  // Once, when the pane opens signed in without them: just after signing in, or when reading them
  // with the account failed.
  useEffect(() => {
    if (devices === null) loadDevices();
  }, []);

  return (
    <>
      <section
        aria-label="Profile"
        className="flex items-center gap-3 border-b border-border py-2.5"
      >
        <Avatar account={account} />
        <div className="min-w-0 flex-1">
          <div className="truncate font-semibold">{account.name ?? account.email}</div>
          {account.name && (
            <div className="truncate text-[length:var(--fs-sm)] text-fg-muted">{account.email}</div>
          )}
        </div>
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() =>
            void act(async () => {
              await api.accountSignOut();
              onAccount(null);
            })
          }
        >
          Sign out
        </button>
      </section>
      <TokenRow title="Name">
        <TextField
          label="Name"
          value={account.name ?? ""}
          onSave={(name) =>
            void act(async () => onAccount(await api.accountRename(name.trim() || null)))
          }
        />
      </TokenRow>

      <SectionHeading>License</SectionHeading>
      <LicensePane />

      <SectionHeading>Devices</SectionHeading>
      {devices === null ? (
        <div role="status" aria-label="Loading devices" className="py-2.5">
          {devicesError ? <ErrorText error={devicesError} /> : <Spinner size={13} />}
        </div>
      ) : (
        <ul aria-label="Devices" className="m-0 list-none p-0">
          {devices.map((d) => {
            const platform = PLATFORMS[d.platform];
            const PlatformIcon = platform?.Icon ?? DesktopIcon;
            const meta = [
              platform?.name ?? d.platform,
              d.version && `v${d.version}`,
              d.current ? "This computer" : d.lastSeenAt && `Last seen ${date(d.lastSeenAt)}`,
            ]
              .filter(Boolean)
              .join(" · ");
            return (
              <li
                key={d.id}
                className="group relative flex items-center gap-3 border-b border-border py-2.5"
              >
                <PlatformIcon size={18} className="flex-none text-fg-muted" />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-semibold">{d.name}</div>
                  <div className="truncate text-[length:var(--fs-sm)] text-fg-muted">{meta}</div>
                </div>
                {!d.current && (
                  <button
                    type="button"
                    className="btn-icon hover-reveal text-fg-muted hover:text-danger"
                    aria-label={`Remove ${d.name}`}
                    disabled={busy}
                    onClick={() =>
                      void act(async () => {
                        await api.accountRemoveDevice(d.id);
                        setDevices(await api.accountDevices());
                      })
                    }
                  >
                    <TrashIcon size={14} />
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      <TokenRow title="Sign out other devices">
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() =>
            void act(async () => {
              await api.accountSignOutOthers();
              setDevices(await api.accountDevices());
            })
          }
        >
          Sign out
        </button>
      </TokenRow>

      <TokenRow title="Delete account">
        <button
          type="button"
          className="btn"
          aria-expanded={deleting}
          aria-controls="account-delete"
          onClick={() => {
            setDeleting(!deleting);
            setPassword("");
          }}
        >
          {deleting ? "Cancel" : "Delete…"}
        </button>
      </TokenRow>
      {deleting && (
        <form
          noValidate
          id="account-delete"
          aria-label="Confirm deleting the account"
          onSubmit={(e) => {
            e.preventDefault();
            void act(async () => {
              await api.accountDelete(password);
              onAccount(null);
            });
          }}
        >
          <PasswordRow
            label="Password"
            autoComplete="current-password"
            autoFocus
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <div className="flex justify-end pt-3">
            <button type="submit" className="btn text-danger" disabled={busy}>
              Delete account
            </button>
          </div>
        </form>
      )}
      <ErrorText error={error} />
    </>
  );
}
