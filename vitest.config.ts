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
