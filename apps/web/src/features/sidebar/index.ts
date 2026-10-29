// What the sidebar offers the workspace around it.
export {
  default,
  type SidebarProps,
  type SidebarView,
  type Target,
} from "~/features/sidebar/Sidebar";
export { default as ActivityBar } from "~/features/sidebar/components/ActivityBar";
export { getSelectedRows, setSelectedRows } from "~/features/sidebar/lib/selectionState";
export { sidebarSignature } from "~/features/sidebar/lib/sidebarSignature";
