import path from 'node:path'
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { playwright } from '@vitest/browser-playwright'

const browserTests = ['frontend/*/tests/**/*.browser.test.{ts,tsx}']
const alias = [
  { find: '@', replacement: path.resolve('frontend/nyanpasu/src') },
  { find: '@root', replacement: path.resolve('.') },
  {
    find: /^~icons\/.*$/,
    replacement: path.resolve('frontend/nyanpasu/tests/icon-stub.tsx'),
  },
]

export default defineConfig({
  test: {
    projects: [
      {
        resolve: { alias },
        test: {
          name: 'unit',
          environment: 'node',
          include: ['frontend/*/tests/**/*.test.{ts,tsx}'],
          exclude: browserTests,
        },
      },
      {
        plugins: [react()],
        css: { postcss: path.resolve('frontend/nyanpasu') },
        resolve: {
          alias,
          dedupe: ['react', 'react-dom', '@tanstack/react-query'],
        },
        optimizeDeps: {
          // A barrel loads every re-export in native ESM, including unused controls.
          // The tests do not import the workers, so a dependency only a worker
          // has would be found mid-run, and optimizing it reloads the tests.
          entries: [
            ...browserTests,
            'frontend/ui/src/index.ts',
            'frontend/nyanpasu/src/**/*.worker.ts',
          ],
          include: [
            'react',
            'react-dom',
            'react-dom/client',
            'react/jsx-runtime',
            'react/jsx-dev-runtime',
            '@tanstack/react-query',
            '@tauri-apps/api/core',
            '@tauri-apps/api/event',
            '@tauri-apps/api/webviewWindow',
            'clsx',
            'react-use/esm/factory/createBreakpoint',
            'react-use/esm/useUpdateEffect',
            'tailwind-merge',
            'vitest-browser-react',
          ],
        },
        test: {
          name: 'browser',
          include: browserTests,
          browser: {
            enabled: true,
            headless: true,
            provider: playwright(),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
})
