import { invoke } from "~/api/invoke";
import type { Screenshot } from "~/api/types";

/** What to do with the pointer or keyboard. */
export type ComputerAction =
  | { kind: "move"; x: number; y: number }
  | { kind: "click"; x: number; y: number; button?: "left" | "middle" | "right"; double?: boolean }
  | { kind: "type"; text: string }
  | { kind: "key"; key: string; modifiers?: ("ctrl" | "shift" | "alt" | "meta")[] }
  | { kind: "scroll"; amount: number; horizontal?: boolean };

export const computerApi = {
  /** The screen as a PNG data URL; refuses unless computer use is on. */
  computerScreenshot: () => invoke<Screenshot>("computer_screenshot"),
  computerAction: (action: ComputerAction) => invoke<void>("computer_action", { action }),
};
