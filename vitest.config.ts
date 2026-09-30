import path from 'node:path'
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { playwright } from '@vitest/browser-playwright'

const browserTests = ['frontend/*/tests/**/*.browser.test.{ts,tsx}']

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          environment: 'node',
          include: ['frontend/*/tests/**/*.test.{ts,tsx}'],
          exclude: browserTests,
        },
      },
      {
        plugins: [react()],
        resolve: {
          alias: {
            '@': path.resolve('frontend/nyanpasu/src'),
            '@interface': path.resolve('frontend/interface/src'),
            '@nyanpasu/interface': path.resolve(
              'frontend/interface/src/index.ts',
            ),
            '@nyanpasu/utils': path.resolve('frontend/utils/src/index.ts'),
            '@tauri-apps/api': path.resolve(
              'frontend/interface/node_modules/@tauri-apps/api',
            ),
            '@tanstack/react-query': path.resolve(
              'frontend/interface/node_modules/@tanstack/react-query',
            ),
            clsx: path.resolve('frontend/utils/node_modules/clsx'),
            'lodash-es': path.resolve(
              'frontend/interface/node_modules/lodash-es',
            ),
            'react-use': path.resolve(
              'frontend/interface/node_modules/react-use',
            ),
            'tailwind-merge': path.resolve(
              'frontend/utils/node_modules/tailwind-merge',
            ),
            'vitest-browser-react': path.resolve(
              'frontend/interface/node_modules/vitest-browser-react',
            ),
          },
        },
        optimizeDeps: {
          include: [
            'react',
            'react-dom/client',
            'react/jsx-runtime',
            '@tanstack/react-query',
            '@tauri-apps/api/core',
            '@tauri-apps/api/event',
            '@tauri-apps/api/webviewWindow',
            'clsx',
            'lodash-es',
            'react-use/esm/factory/createBreakpoint',
            'react-use/esm/useUpdateEffect',
            'tailwind-merge',
            'vitest-browser-react',
          ],
          exclude: ['@nyanpasu/interface', '@nyanpasu/utils'],
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
