import generouted from "@generouted/react-router/plugin";
// import react from "@vitejs/plugin-react";
import react from "@vitejs/plugin-react-swc";
import path from "node:path";
import { defineConfig } from "vite";
import monaco from "vite-plugin-monaco-editor";
import sassDts from "vite-plugin-sass-dts";
import svgr from "vite-plugin-svgr";
import tsconfigPaths from "vite-tsconfig-paths";

const devtools = () => {
  return {
    name: "react-devtools",
    transformIndexHtml(html) {
      return html.replace(
        /<\/head>/,
        `<script src="http://localhost:8097"></script></head>`,
      );
    },
  };
};

// https://vitejs.dev/config/
export default defineConfig(({ command }) => {
  const isDev = command === "serve";

  return {
    // root: "/",
    server: { port: 3000 },
    css: {
      preprocessorOptions: {
        scss: {
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
      react({
        // babel: {
        //   plugins: ["@emotion/babel-plugin"],
        // },
      }),
      generouted(),
      sassDts({ esmExport: true }),
      monaco({ languageWorkers: ["editorWorkerService", "typescript"] }),
      isDev && devtools(),
    ],
    optimizeDeps: {
      include: ["@emotion/styled", "@mui/lab/*", "@mui/material/*"],
    },
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
    html: {},
  };
});
