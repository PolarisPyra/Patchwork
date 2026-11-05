import type { BackgroundStrength, NotificationSettings, Theme } from "~/api";
import BackgroundPicker from "~/components/BackgroundPicker";
import { SectionHeading, Segmented, TokenRow, TokenSwitch } from "~/features/settings/controls";
import { LOGO_URLS } from "~/features/settings/logo";
import type { PaneProps } from "~/features/settings/types";
import logoManifest from "../../../../../../assets/logos/manifest.json";

const STRENGTHS: { value: BackgroundStrength; label: string }[] = [
  { value: "subtle", label: "Subtle" },
  { value: "medium", label: "Medium" },
  { value: "strong", label: "Strong" },
];

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/**
 * The settings too few for a pane each: the theme, the app icon Patchwork wears in the dock and the
 * launcher, and the background every chat shows unless it has its own; how a chat is answered,
 * whether picking an offered answer sends it or only fills the composer and what Enter does with a
 * message written while a reply is running; and which finished work raises a desktop notification
 * while Patchwork is not focused.
 */
export default function GeneralPane({
  settings,
  updateSettings,
  onRemoveBackgroundImage,
}: PaneProps & { onRemoveBackgroundImage: (file: string) => Promise<() => Promise<void>> }) {
  const logo = settings.logo ?? logoManifest.default;
  const notifications = settings.notifications;
  const notify = (patch: Partial<NotificationSettings>) =>
    void updateSettings((s) => ({ ...s, notifications: { ...s.notifications, ...patch } }));
  return (
    <>
      <SectionHeading>Appearance</SectionHeading>
      <TokenRow title="Theme" desc="Light, dark, or follow the system setting">
        <Segmented
          label="Theme"
          value={settings.theme}
          options={THEMES}
          onChange={(theme) => void updateSettings((s) => ({ ...s, theme }))}
        />
      </TokenRow>
      <div className="border-b border-border py-2.5">
        <div className="font-semibold">App icon</div>
        <div role="radiogroup" aria-label="App icon" className="mt-2 grid grid-cols-8 gap-1">
          {logoManifest.icons.map((icon) => (
            <button
              key={icon.id}
              role="radio"
              aria-checked={logo === icon.id}
              aria-label={icon.name}
              title={icon.name}
              className={`grid aspect-square cursor-pointer place-items-center rounded-[var(--radius-sm)] border p-2 ${logo === icon.id ? "border-accent bg-accent-soft" : "border-transparent bg-transparent hover:bg-hover"}`}
              onClick={() => void updateSettings((s) => ({ ...s, logo: icon.id }))}
            >
              <img src={LOGO_URLS[icon.id]} alt="" draggable={false} className="size-full" />
            </button>
          ))}
        </div>
      </div>
      <div className="border-b border-border py-2.5">
        <div className="pb-2 font-semibold">Chat background</div>
        <BackgroundPicker
          label="Chat background"
          value={settings.chatBackground}
          cleared={{ kind: "none" }}
          onChange={(chatBackground) =>
            chatBackground && void updateSettings((s) => ({ ...s, chatBackground }))
          }
          onRemove={onRemoveBackgroundImage}
        />
      </div>
      <TokenRow title="Background strength">
        <Segmented
          label="Background strength"
          value={settings.chatBackgroundStrength}
          options={STRENGTHS}
          onChange={(chatBackgroundStrength) =>
            void updateSettings((s) => ({ ...s, chatBackgroundStrength }))
          }
        />
      </TokenRow>

      <SectionHeading>Chats</SectionHeading>
      <TokenRow title="Click answers twice to send">
        <TokenSwitch
          label="Click answers twice to send"
          on={settings.confirmAnswers}
          onChange={(confirmAnswers) => void updateSettings((st) => ({ ...st, confirmAnswers }))}
        />
      </TokenRow>
      <TokenRow title="Enter steers a running reply">
        <TokenSwitch
          label="Enter steers a running reply"
          on={settings.steerWhileWorking}
          onChange={(steerWhileWorking) =>
            void updateSettings((st) => ({ ...st, steerWhileWorking }))
          }
        />
      </TokenRow>

      <SectionHeading>Notifications</SectionHeading>
      <TokenRow
        title="Goal completed"
        desc="When a task meets its goal, while Patchwork is not the focused window"
      >
        <TokenSwitch
          label="Goal completed"
          on={notifications.goalComplete}
          onChange={(goalComplete) => notify({ goalComplete })}
        />
      </TokenRow>
      <TokenRow
        title="Task completed"
        desc="When an agent finishes replying in a task without a running goal"
      >
        <TokenSwitch
          label="Task completed"
          on={notifications.taskComplete}
          onChange={(taskComplete) => notify({ taskComplete })}
        />
      </TokenRow>
    </>
  );
}
