import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  build: {
    // CodeMirror 自己就 500KB 上下；App 从本机磁盘读，不用为了这条提示再拆。
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        // 大库各自一块：输入框编辑器（CodeMirror）、Markdown、React。App 从本机磁盘读，拆开只是让每块好查、改了界面不用整包重下。
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (/[\\/](@codemirror|@lezer|codemirror|style-mod|w3c-keyname|crelt)[\\/]/.test(id)) return "editor";
          if (/[\\/](react-markdown|remark-|mdast-|micromark|unified|hast-|unist-|vfile|property-information|space-separated|comma-separated|decode-named|character-entities|trim-lines|devlop|bail|is-plain-obj|trough|zwitch|ccount|longest-streak|markdown-table|html-url-attributes|estree-|style-to|inline-style)/.test(id)) return "markdown";
          if (/[\\/](react|react-dom|scheduler)[\\/]/.test(id)) return "react";
          return "vendor";
        },
      },
    },
  },
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1431,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
