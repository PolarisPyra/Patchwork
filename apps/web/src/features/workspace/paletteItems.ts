import type { Project, Thread } from "~/api";
import type { PaletteItem } from "~/features/workspace/components/CommandPalette";
import { keyLabel, type Rule } from "~/lib/shortcuts";
import { displayTitle } from "~/lib/util";

interface PaletteSource {
  /** The open project, or null in a chat without one. */
  project: Project | null;
  projects: Project[];
  /** Chats and tasks that are not archived. */
  threads: Thread[];
  rules: Rule[];
  newChat: () => void;
  newTask: (projectId: string) => void;
  toggleTheme: () => void;
  openSettings: () => void;
  openFolder: () => void;
  /** Shows the thread in the task list and opens it. */
  openThread: (id: string) => void;
  openProject: (id: string) => void;
  /** Runs the project's checks for the reply on screen; null when there is no reply to check. */
  runChecks: (() => void) | null;
  /** Picks the open chat's own background; null when no chat is open. */
  chatBackground: (() => void) | null;
}

/** What search lists: actions, then every chat and task by project, then the projects. */
export function paletteItems({
  project,
  projects,
  threads,
  rules,
  newChat,
  newTask,
  toggleTheme,
  openSettings,
  openFolder,
  openThread,
  openProject,
  runChecks,
  chatBackground,
}: PaletteSource): PaletteItem[] {
  return [
    {
      id: "new-chat",
      group: "Actions",
      label: "New chat",
      hint: project ? undefined : keyLabel(rules, "chat.new"),
      run: newChat,
    },
    ...(project
      ? [
          {
            id: "new-task",
            group: "Actions",
            label: `New task in ${project.name}`,
            hint: keyLabel(rules, "chat.new"),
            run: () => newTask(project.id),
          },
        ]
      : []),
    ...(runChecks
      ? [
          {
            id: "run-checks",
            group: "Actions",
            label: "Run checks for the open task",
            run: runChecks,
          },
        ]
      : []),
    ...(chatBackground
      ? [
          {
            id: "chat-background",
            group: "Actions",
            label: "Change this chat's background",
            run: chatBackground,
          },
        ]
      : []),
    { id: "toggle-theme", group: "Actions", label: "Toggle theme", run: toggleTheme },
    {
      id: "settings",
      group: "Actions",
      label: "Open settings",
      hint: keyLabel(rules, "settings.open"),
      run: openSettings,
    },
    { id: "open-folder", group: "Actions", label: "Open folder…", run: openFolder },
    // Development only: a fresh load also rebuilds the composer editor.
    ...(import.meta.env.DEV
      ? [
          {
            id: "reload-webview",
            group: "Actions",
            label: "Reload webview",
            hint: "Ctrl R",
            run: () => window.location.reload(),
          },
        ]
      : []),
    ...threads.map((t) => ({
      id: `task-${t.id}`,
      group: projects.find((p) => p.id === t.projectId)?.name ?? "Chats",
      label: displayTitle(t.title),
      run: () => openThread(t.id),
    })),
    ...projects.map((p) => ({
      id: `project-${p.id}`,
      group: "Projects",
      label: p.name,
      hint: p.path,
      run: () => openProject(p.id),
    })),
  ];
}
