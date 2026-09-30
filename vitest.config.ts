import { fileURLToPath } from 'node:url'
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
          // The app's aliases, for the nyanpasu modules a browser test loads.
          alias: [
            {
              find: '@',
              replacement: fileURLToPath(
                new URL('frontend/nyanpasu/src', import.meta.url),
              ),
            },
            {
              find: '@interface',
              replacement: fileURLToPath(
                new URL('frontend/interface/src', import.meta.url),
              ),
            },
            {
              find: '@nyanpasu/interface',
              replacement: fileURLToPath(
                new URL('frontend/interface/src', import.meta.url),
              ),
            },
            {
              find: /^~icons\/.*$/,
              replacement: fileURLToPath(
                new URL(
                  'frontend/nyanpasu/tests/icon-stub.tsx',
                  import.meta.url,
                ),
              ),
            },
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
