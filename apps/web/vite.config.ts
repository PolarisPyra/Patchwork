import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "~": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    // Mermaid's layout engine (about 1.5 MB), its parser, and the language detector's model load
    // only when a diagram or pasted code needs them, so their size is not a startup cost.
    chunkSizeWarningLimit: 1500,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: "prosemirror",
              test: /node_modules[\\/](prosemirror-|orderedmap|rope-sequence|w3c-keyname|style-mod|crelt)/,
            },
            { name: "tiptap", test: /node_modules[\\/](@tiptap|linkifyjs)/ },
            { name: "highlight", test: /node_modules[\\/]highlight\.js/ },
            {
              name: "markdown",
              test: /node_modules[\\/](react-markdown|remark-|micromark|mdast-|hast-|unist-|unified|vfile|decode-named-character-reference|character-entities|property-information|space-separated-tokens|comma-separated-tokens|devlop|bail|trough|zwitch|ccount|markdown-table|longest-streak|html-url-attributes|estree-util-|style-to-js|style-to-object|inline-style-parser|trim-lines|is-plain-obj)/,
            },
            { name: "react", test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            { name: "xterm", test: /node_modules[\\/]@xterm/ },
            { name: "vendor", test: /node_modules[\\/](@legendapp|@phosphor-icons|@tauri-apps)/ },
          ],
        },
      },
    },
  },
});
