import { TokenRow, TokenSwitch } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";

/** How a chat is answered: whether picking an offered answer sends it or only fills the composer,
 *  and what Enter does with a message written while a reply is running. */
export default function ChatPane({ settings, updateSettings }: PaneProps) {
  return (
    <>
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
    </>
  );
}
