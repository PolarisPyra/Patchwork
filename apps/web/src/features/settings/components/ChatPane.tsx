import { TokenRow, TokenSwitch } from "~/features/settings/components/controls";
import type { PaneProps } from "~/features/settings/types";

/** How a chat is answered: whether picking an offered answer sends it or only fills the composer. */
export default function ChatPane({ settings, updateSettings }: PaneProps) {
  return (
    <TokenRow title="Click answers twice to send">
      <TokenSwitch
        label="Click answers twice to send"
        on={settings.confirmAnswers}
        onChange={(confirmAnswers) => void updateSettings((st) => ({ ...st, confirmAnswers }))}
      />
    </TokenRow>
  );
}
