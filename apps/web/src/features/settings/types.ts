import type { Settings } from "~/api";
import type { UpdateSettings } from "~/api";

/** What every settings pane is given: the settings, and the way to save a change to them. */
export interface PaneProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}
