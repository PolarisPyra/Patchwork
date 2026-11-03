import { useEffect, useState } from "react";
import { CaretDownIcon, PencilSimpleIcon, RobotIcon } from "@phosphor-icons/react";
import { api, type CustomAgent } from "~/api";
import Dropdown from "~/components/Dropdown";
import { CHIP_CLASS } from "~/components/chip";
import { errorText } from "~/lib/util";

/** The menu's width, so it holds its size as the rows and messages in it change. */
const MENU_WIDTH = "w-[300px]";

/** The list's width: the menu's, less the 10px scrollbar from `theme.css`. */
const LIST_WIDTH = "w-[290px]";

const ITEM_CLASS =
  "flex min-w-[200px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg";

/** The custom agent the chat is in the role of, picked from the agents it can summon. */
export default function AgentPicker({
  projectId,
  role,
  onPick,
}: {
  projectId: string | null;
  role: string | null;
  onPick: (role: string | null) => void;
}) {
  return (
    <Dropdown
      up
      label="Agent"
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={role ? `Agent: ${role}` : "Agent: none"}
          onClick={toggle}
        >
          <RobotIcon size={16} />
          <span>{role ?? "Agent"}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) => (
        <AgentOptions
          projectId={projectId}
          role={role}
          close={close}
          onPick={(next) => {
            close();
            onPick(next);
          }}
        />
      )}
    </Dropdown>
  );
}

/** The menu's items; mounted only while the menu is open, so the agents are read then. */
function AgentOptions({
  projectId,
  role,
  close,
  onPick,
}: {
  projectId: string | null;
  role: string | null;
  close: () => void;
  onPick: (role: string | null) => void;
}) {
  const [agents, setAgents] = useState<CustomAgent[]>();
  const [failed, setFailed] = useState(false);
  const [editError, setEditError] = useState<string>();
  // The editor opens in the foreground, so the menu closes once the file is handed over.
  function edit(agent: CustomAgent) {
    setEditError(undefined);
    api.openAgent(agent.path, projectId).then(close, (e: unknown) => setEditError(errorText(e)));
  }
  useEffect(() => {
    let current = true;
    api.listAgents(projectId).then(
      (list) => current && setAgents(list),
      () => current && setFailed(true),
    );
    return () => {
      current = false;
    };
  }, [projectId]);
  if (failed)
    return (
      <div
        role="alert"
        className={`px-2 py-1.5 text-[length:var(--fs-sm)] text-danger ${MENU_WIDTH}`}
      >
        Could not load agents.
      </div>
    );
  if (!agents)
    return (
      <div
        role="status"
        className={`px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted ${MENU_WIDTH}`}
      >
        Loading agents…
      </div>
    );
  // The list scrolls within the menu's height, so a long list stays inside the window. The
  // scrollbar's space is held from the start, so the text does not shift once the bar shows. The
  // rows sit in a column of fixed width: WebKit sizes rows that fill the scroller before its
  // scrollbar shows and fits them beside it only on the first hover, which moved every ellipsis.
  return (
    <div
      className={`max-h-[min(50vh,24rem)] overflow-y-auto overscroll-contain overflow-x-hidden [scrollbar-gutter:stable] ${MENU_WIDTH}`}
    >
      <div className={`flex flex-col ${LIST_WIDTH}`}>
        {editError && (
          <div role="alert" className="px-2 py-1.5 text-[length:var(--fs-sm)] text-danger">
            {editError}
          </div>
        )}
        <button
          role="menuitemradio"
          aria-checked={!role}
          className={`${ITEM_CLASS} hover:bg-hover ${role ? "" : "bg-selected-muted"}`}
          onClick={() => onPick(null)}
        >
          No agent
        </button>
        {agents.map((agent) => (
          // The row is the hover and selection target, so the pencil lights up with its name. The
          // pencil sits beside the name button, not inside it, since a button cannot hold a button.
          <div
            key={agent.name}
            className={`group flex items-center rounded-[var(--radius-sm)] hover:bg-hover ${agent.name === role ? "bg-selected-muted" : ""}`}
          >
            <button
              role="menuitemradio"
              aria-checked={agent.name === role}
              title={agent.description ? `${agent.name}: ${agent.description}` : agent.name}
              className={`${ITEM_CLASS} min-w-0 flex-1 flex-col items-start! gap-0.5`}
              onClick={() => onPick(agent.name)}
            >
              <span className="w-full min-w-0 truncate">{agent.name}</span>
              {agent.description && (
                <span className="w-full min-w-0 truncate text-[length:var(--fs-sm)] text-fg-muted">
                  {agent.description}
                </span>
              )}
            </button>
            {/* Its space is kept while the pencil is hidden, so the menu does not resize on hover. */}
            <span className="relative mr-1 grid h-6 w-6 flex-none place-items-center">
              <button
                type="button"
                className="hover-reveal grid h-6 w-6 cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent p-0 text-fg-muted hover:text-fg"
                aria-label={`Edit ${agent.name}`}
                onClick={() => edit(agent)}
              >
                <PencilSimpleIcon size={14} />
              </button>
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
