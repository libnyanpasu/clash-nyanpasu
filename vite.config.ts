import react from "@vitejs/plugin-react";
import path from "path";
import { defineConfig } from "vite";
import monaco from "vite-plugin-monaco-editor";
import svgr from "vite-plugin-svgr";

// https://vitejs.dev/config/
export default defineConfig(({ command }) => {
  const isDev = command === "serve";

  return {
    root: "src",
    server: { port: 3000 },
    plugins: [
      svgr(),
      react(),
      monaco({ languageWorkers: ["editorWorkerService", "typescript"] }),
    ],
    esbuild: {
      drop: isDev ? undefined : ["console", "debugger"],
    },
    build: {
      outDir: "../dist",
      emptyOutDir: true,
    },
    resolve: {
      alias: {
        "@": path.resolve("./src"),
        "@root": path.resolve("."),
      },
    },
    define: {
      OS_PLATFORM: `"${process.platform}"`,
      WIN_PORTABLE: !!process.env.VITE_WIN_PORTABLE,
    },
  };
});
