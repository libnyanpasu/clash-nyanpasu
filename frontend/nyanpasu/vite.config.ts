import path from 'node:path'
import { NodePackageImporter } from 'sass-embedded'
import AutoImport from 'unplugin-auto-import/vite'
import IconsResolver from 'unplugin-icons/resolver'
import Icons from 'unplugin-icons/vite'
import { defineConfig, UserConfig } from 'vite'
import { createHtmlPlugin } from 'vite-plugin-html'
import sassDts from 'vite-plugin-sass-dts'
import svgr from 'vite-plugin-svgr'
import tsconfigPaths from 'vite-tsconfig-paths'
import tailwindPlugin from '@tailwindcss/vite'
// import react from "@vitejs/plugin-react";
import { TanStackRouterVite } from '@tanstack/router-plugin/vite'
import legacy from '@vitejs/plugin-legacy'
import react from '@vitejs/plugin-react-swc'

const devtools = () => {
  return {
    name: 'react-devtools',
    transformIndexHtml(html: string) {
      return html.replace(
        /<\/head>/,
        `<script src="http://localhost:8097"></script></head>`,
      )
    },
  }
}

const IS_NIGHTLY = process.env.NIGHTLY?.toLowerCase() === 'true'

// https://vitejs.dev/config/
export default defineConfig(({ command, mode }) => {
  const isDev = command === 'serve'

  const config = {
    // root: "/",
    server: { port: 3000 },
    css: {
      preprocessorOptions: {
        scss: {
          api: 'modern-compiler',
          // @ts-expect-error fucking vite why embedded their own sass types definition????
          importer: [
            new NodePackageImporter(),
            // TODO: fix this when vite-sass-dts support it, or fix it when we use `@alias`
            // (...args: string[]) => {
            //   if (args[0] !== '@/styles') {
            //     return
            //   }

            //   return {
            //     file: `${path.resolve(__dirname, './src/assets/styles')}`,
            //   }
            // },
          ],
        },
      },
    },
    plugins: [
      tailwindPlugin(),
      tsconfigPaths(),
      legacy({
        renderLegacyChunks: false,
        modernTargets: ['edge>=109', 'safari>=13'],
        modernPolyfills: true,
        additionalModernPolyfills: [
          'core-js/modules/es.object.has-own.js',
          'core-js/modules/web.structured-clone.js',
          'core-js/modules/es.array.at.js',
        ],
      }),
      createHtmlPlugin({
        inject: {
          data: {
            title: 'Clash Nyanpasu',
            injectScript:
              mode === 'development'
                ? '<script src="https://unpkg.com/react-scan/dist/auto.global.js"></script>'
                : '',
          },
        },
      }),
      TanStackRouterVite(),
      svgr(),
      react({
        // babel: {
        //   plugins: ["@emotion/babel-plugin"],
        // },
      }),
      AutoImport({
        resolvers: [
          IconsResolver({
            prefix: 'Icon',
            extension: 'jsx',
          }),
        ],
      }),
      Icons({
        compiler: 'jsx', // or 'solid'
      }),
      sassDts({ esmExport: true }),
      isDev && devtools(),
    ],
    resolve: {
      alias: {
        '@repo': path.resolve('../../'),
        '@nyanpasu/ui': path.resolve('../ui/src'),
        '@nyanpasu/interface': path.resolve('../interface/src'),
      },
    },
    optimizeDeps: {
      entries: ['./src/main.tsx'],
      include: ['@emotion/styled'],
    },
    esbuild: {
      drop: isDev ? undefined : ['debugger'],
      pure: isDev || IS_NIGHTLY ? [] : ['console.log'],
    },
    build: {
      outDir: '../../backend/tauri/tmp/dist',
      rollupOptions: {
        output: {
          manualChunks: {
            jsonWorker: [`monaco-editor/esm/vs/language/json/json.worker`],
            tsWorker: [`monaco-editor/esm/vs/language/typescript/ts.worker`],
            editorWorker: [`monaco-editor/esm/vs/editor/editor.worker`],
            yamlWorker: [`monaco-yaml/yaml.worker`],
          },
        },
      },
      emptyOutDir: true,
      sourcemap: isDev || IS_NIGHTLY ? 'inline' : false,
    },
    define: {
      OS_PLATFORM: `"${process.platform}"`,
      WIN_PORTABLE: !!process.env.VITE_WIN_PORTABLE,
    },
    html: {},
  } satisfies UserConfig
  // fucking vite why embedded their own sass types definition????
  return config as any as UserConfig
})
