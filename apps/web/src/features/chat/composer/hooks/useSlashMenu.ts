import { useEffect, useRef, useState } from "react";
import type { Editor } from "@tiptap/react";
import {
  api,
  type AgentKind,
  type CustomAgent,
  type ProviderSlashCommand,
  type Skill,
} from "~/api";
import { commandScore, LOCAL_COMMANDS, SLASH_COMMANDS } from "~/lib/composerActions";
import { useAgents } from "~/lib/agents";
import { commandToken, type CommandToken } from "~/features/chat/composer/lib/commandToken";

/** What the slash menu lists: commands for "/", skills for "$" and agents for "&". */
export type SlashMenuKind = "command" | "skill" | "agent";

export interface SlashMatch {
  name: string;
  description: string;
}

/**
 * The slash menu, which lists commands for the "/word", "$word" or "&word" being typed, anywhere
 * in the draft, and the names the editor turns into chips. Escape hides it until the draft
 * changes again.
 */
export function useSlashMenu({
  agent,
  projectId,
  providerSlashCommands,
  onChange,
  onSelectSlashCommand,
}: {
  agent: AgentKind;
  projectId: string | null;
  providerSlashCommands: ProviderSlashCommand[];
  onChange: (text: string) => void;
  onSelectSlashCommand: (name: string) => boolean;
}) {
  const [menuClosed, setMenuClosed] = useState(false);
  const [commandIndex, setCommandIndex] = useState(0);
  const [token, setToken] = useState<CommandToken | null>(null);
  // Antigravity reads the message as a prompt, so there are no commands to offer, but an agent
  // summoned with "&" goes with any message.
  const slash =
    token && (agent !== "antigravity" || token.text.startsWith("&"))
      ? token.text.toLowerCase()
      : null;
  const skillMenu = slash?.startsWith("$") ?? false;
  const agentMenu = slash?.startsWith("&") ?? false;
  // The agents folder's agents are watched; the project's own are read up front, so a typed
  // `&name` becomes its chip, and again, with them, each time the menu opens.
  const folderAgents = useAgents().agents;
  const [listed, setListed] = useState<{
    projectId: string | null;
    folder: CustomAgent[];
    agents: CustomAgent[];
  }>();
  useEffect(() => {
    api.listAgents(projectId).then(
      (agents) => setListed({ projectId, folder: folderAgents, agents }),
      () => undefined,
    );
  }, [agentMenu, projectId, folderAgents]);
  // A list read for another project, or before the folder last changed, is stale.
  const customAgents =
    listed?.projectId === projectId && listed.folder === folderAgents
      ? listed.agents
      : folderAgents;
  // The editor's `&name ` rule reads the agents through this.
  const customAgentsRef = useRef(customAgents);
  customAgentsRef.current = customAgents;
  const agentCommands = customAgents.map((custom) => ({
    name: `&${custom.name}`,
    description: custom.description ?? "Agent",
  }));
  // Skills are read up front, and again each time the menu opens, so ones just added to the
  // folder show up.
  const [skills, setSkills] = useState<Skill[]>([]);
  // The editor's `$name ` rule reads the skills through this.
  const skillsRef = useRef(skills);
  skillsRef.current = skills;
  useEffect(() => {
    api.listSkills().then(
      (view) => setSkills(view.skills.filter((skill) => skill.enabled)),
      () => undefined,
    );
  }, [skillMenu]);
  const builtInNames = new Set(SLASH_COMMANDS.map((command) => command.name.slice(1)));
  const skillCommands = skills.map((skill) => ({
    name: `$${skill.name}`,
    description: skill.description ?? "Skill",
  }));
  const slashCommands = [
    // Patchwork does not run /goal in OpenCode.
    ...SLASH_COMMANDS.filter((command) => agent !== "opencode" || command.name !== "/goal"),
    ...providerSlashCommands
      // Claude lists Patchwork's skills again as the "claude" plugin's; they are under "$".
      .filter((command) => {
        const name = command.name.replace(/^\//, "");
        return !builtInNames.has(name) && !name.startsWith("claude:");
      })
      .map((command) => ({
        name: command.name.startsWith("/") ? command.name : `/${command.name}`,
        description:
          command.description ??
          (agent === "opencode" ? "OpenCode command" : "Claude Code command"),
      })),
  ];
  // The editor's `/name ` rule reads the commands the agent runs itself through this.
  const agentCommandNames = useRef(new Set<string>());
  agentCommandNames.current = new Set(
    agent === "antigravity"
      ? []
      : slashCommands
          .filter((command) => !LOCAL_COMMANDS.has(command.name))
          .map((command) => command.name.slice(1)),
  );
  const matches: SlashMatch[] =
    slash === null
      ? []
      : (skillMenu ? skillCommands : agentMenu ? agentCommands : slashCommands)
          .flatMap((c) => {
            const score = commandScore(c.name.toLowerCase(), slash.slice(1));
            return score === null ? [] : [{ ...c, score }];
          })
          .sort((a, b) => a.score - b.score);
  // Later in a draft, a "/" that matches nothing is a path, so no menu shows for it.
  const menuOpen = !menuClosed && slash !== null && (token?.whole || matches.length > 0);
  const activeCommand = matches[Math.min(commandIndex, matches.length - 1)];
  const optionRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (menuOpen) optionRef.current?.scrollIntoView({ block: "nearest" });
  }, [menuOpen, commandIndex, activeCommand?.name]);

  /** The draft changed: the menu shows again, from the top, for the word now typed. */
  function draftChanged(editor: Editor) {
    setMenuClosed(false);
    setCommandIndex(0);
    setToken(commandToken(editor));
  }

  function selectionChanged(editor: Editor) {
    setToken(commandToken(editor));
  }

  function complete(editor: Editor, name: string) {
    if (!token) return;
    // A skill or an agent goes in as its chip, and so does a command the agent runs, at the start
    // of the draft where the agent reads it.
    const leading =
      name.startsWith("/") && !LOCAL_COMMANDS.has(name) && (token.whole || token.from === 1);
    const chip = name.startsWith("$")
      ? "skillCommand"
      : name.startsWith("&")
        ? "agentCommand"
        : leading
          ? name === "/goal"
            ? "goalCommand"
            : "slashCommand"
          : null;
    const picked = chip
      ? [
          { type: chip, attrs: { name: name.slice(1) } },
          { type: "text", text: " " },
        ]
      : `${name} `;
    // Later in the draft, the pick replaces just the word typed. A local command only runs as the
    // whole message, so it replaces the draft wherever it was typed.
    if (!token.whole && !LOCAL_COMMANDS.has(name)) {
      editor.chain().focus().insertContentAt({ from: token.from, to: token.to }, picked).run();
      return;
    }
    // A pick for a draft that is a bare "/word", "$word" or "&word" replaces the whole draft.
    editor.chain().focus().selectAll().deleteSelection().insertContent(picked).run();
    onChange(editor.getMarkdown());
  }

  function select(editor: Editor | null, name: string) {
    if ((token?.whole || LOCAL_COMMANDS.has(name)) && onSelectSlashCommand(name)) {
      editor?.chain().focus().clearContent().run();
      onChange("");
      return;
    }
    if (editor) complete(editor, name);
  }

  /** The open menu's keys; false for a key it leaves to the composer. */
  function handleKey(editor: Editor, event: KeyboardEvent): boolean {
    if (event.key === "Escape") {
      setMenuClosed(true);
      return true;
    }
    if (event.key === "ArrowDown" && matches.length > 0) {
      event.preventDefault();
      setCommandIndex((i) => (i + 1) % matches.length);
      return true;
    }
    if (event.key === "ArrowUp" && matches.length > 0) {
      event.preventDefault();
      setCommandIndex((i) => (i + matches.length - 1) % matches.length);
      return true;
    }
    // Enter on a command typed in full sends it (a bare "/goal" shows the goal).
    const typed = activeCommand !== undefined && token?.raw.toLowerCase() === activeCommand.name;
    if (
      activeCommand &&
      (event.key === "Tab" || (event.key === "Enter" && !typed && !event.shiftKey))
    ) {
      complete(editor, activeCommand.name);
      return true;
    }
    return false;
  }

  const kind: SlashMenuKind = skillMenu ? "skill" : agentMenu ? "agent" : "command";
  return {
    kind,
    menuOpen,
    matches,
    activeCommand,
    optionRef,
    token,
    skills,
    customAgents,
    skillsRef,
    customAgentsRef,
    agentCommandNames,
    draftChanged,
    selectionChanged,
    select,
    handleKey,
  };
}
