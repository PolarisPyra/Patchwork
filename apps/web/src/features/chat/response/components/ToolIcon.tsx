import { type ReactNode } from "react";
import {
  BooksIcon,
  BrainIcon,
  EyeIcon,
  GlobeIcon,
  MagnifyingGlassIcon,
  NotePencilIcon,
  RobotIcon,
  TerminalIcon,
  WrenchIcon,
} from "@phosphor-icons/react";
import type { Entry } from "~/features/chat/response/model";
import { actionOf, type Action } from "~/features/chat/response/tools";

/** The glyph for each kind of work. One size, so icons line up down the column. */
const GLYPH: Record<Action, ReactNode> = {
  edit: <NotePencilIcon size={17} />,
  command: <TerminalIcon size={17} />,
  read: <EyeIcon size={17} />,
  search: <MagnifyingGlassIcon size={17} />,
  web: <GlobeIcon size={17} />,
  agent: <RobotIcon size={17} />,
  skill: <BooksIcon size={17} />,
  other: <WrenchIcon size={17} />,
};

/** The glyph for a kind of work. */
export function actionIcon(action: Action): ReactNode {
  return GLYPH[action];
}

/** The glyph for a row: its kind, or a brain for reasoning. A failure shows as the row's mark instead. */
export function entryIcon(entry: Entry): ReactNode {
  if (entry.kind === "thinking") return <BrainIcon size={17} />;
  return actionIcon(actionOf(entry.tool));
}
