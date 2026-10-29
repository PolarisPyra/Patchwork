import type { NotificationSettings } from "~/api";
import { TokenRow, TokenSwitch } from "~/features/settings/components/controls";
import type { PaneProps } from "~/features/settings/types";

/** Which finished work raises a desktop notification while Patchwork is not focused. */
export default function NotificationsPane({ settings, updateSettings }: PaneProps) {
  const notifications = settings.notifications;
  const update = (patch: Partial<NotificationSettings>) =>
    void updateSettings((s) => ({ ...s, notifications: { ...s.notifications, ...patch } }));
  return (
    <>
      <TokenRow
        title="Goal completed"
        desc="When a task meets its goal, while Patchwork is not the focused window"
      >
        <TokenSwitch
          label="Goal completed"
          on={notifications.goalComplete}
          onChange={(goalComplete) => update({ goalComplete })}
        />
      </TokenRow>
      <TokenRow
        title="Task completed"
        desc="When an agent finishes replying in a task without a running goal"
      >
        <TokenSwitch
          label="Task completed"
          on={notifications.taskComplete}
          onChange={(taskComplete) => update({ taskComplete })}
        />
      </TokenRow>
    </>
  );
}
