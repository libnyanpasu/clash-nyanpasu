import generouted from "@generouted/react-router/plugin";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { defineConfig } from "vite";
import monaco from "vite-plugin-monaco-editor";
import sassDts from "vite-plugin-sass-dts";
import svgr from "vite-plugin-svgr";
import tsconfigPaths from "vite-tsconfig-paths";

// https://vitejs.dev/config/
export default defineConfig(({ command }) => {
  const isDev = command === "serve";

  return {
    // root: "/",
    server: { port: 3000 },
    css: {
      preprocessorOptions: {
        scss: {
          additionalData: `@use "@/styles" as common;`,
          importer(...args) {
            if (args[0] !== "@/styles") {
              return;
            }

            return {
              file: `${path.resolve(__dirname, "./src/assets/styles")}`,
            };
          },
        },
      },
    },
    plugins: [
      tsconfigPaths(),
      svgr(),
      react(),
      generouted(),
      sassDts({
        enabledMode: ["development", "production"],
        global: {
          generate: true,
          outputFilePath: path.resolve(__dirname, "./src/style.d.ts"),
        },
        sourceDir: path.resolve(__dirname, "./src"),
        outputDir: path.resolve(__dirname, "./dist"),
      }),
      monaco({ languageWorkers: ["editorWorkerService", "typescript"] }),
    ],
    esbuild: {
      drop: isDev ? undefined : ["console", "debugger"],
    },
    build: {
      outDir: "dist",
      emptyOutDir: true,
    },
    define: {
      OS_PLATFORM: `"${process.platform}"`,
      WIN_PORTABLE: !!process.env.VITE_WIN_PORTABLE,
    },
  };
});
