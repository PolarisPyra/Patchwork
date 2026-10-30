import { type ReactNode } from "react";
import {
  BooksIcon,
  EyeIcon,
  GlobeIcon,
  MagnifyingGlassIcon,
  NotePencilIcon,
  RobotIcon,
  TerminalIcon,
  WarningCircleIcon,
  WrenchIcon,
} from "@phosphor-icons/react";
import type { Entry } from "~/features/chat/response/lib/model";
import { actionOf, type Action } from "~/features/chat/response/lib/tools";

/** The glyph for each kind of work. One size, so icons line up down the column. */
const GLYPH: Record<Action, ReactNode> = {
  edit: <NotePencilIcon size={15} />,
  command: <TerminalIcon size={15} />,
  read: <EyeIcon size={15} />,
  search: <MagnifyingGlassIcon size={15} />,
  web: <GlobeIcon size={15} />,
  agent: <RobotIcon size={15} />,
  skill: <BooksIcon size={15} />,
  other: <WrenchIcon size={15} />,
};

/** The glyph for a kind of work. */
export function actionIcon(action: Action): ReactNode {
  return GLYPH[action];
}

/** The glyph for a row: its kind, or its failure if it failed. */
export function entryIcon(entry: Entry): ReactNode {
  if (entry.tool.status === "error") return <WarningCircleIcon size={15} className="text-danger" />;
  return actionIcon(actionOf(entry.tool));
}
