import path from 'node:path'
import Icons from 'unplugin-icons/vite'
import { defineConfig } from 'vitest/config'
import type { BrowserCommand } from 'vitest/node'
import tailwindPlugin from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { playwright } from '@vitest/browser-playwright'

// Page benchmarks run the real page components against fixture data in a
// real browser. They measure frames, so they only run on demand, never in
// `pnpm test`. See `perf/README.md`.

const root = path.resolve(import.meta.dirname, '..')

// Production React is what users run; the development build is several times
// slower and skews where the time goes. `react-dom/profiling` keeps
// <Profiler> timings available in production.
const dev = process.env.PERF_DEV === '1'
if (!dev) {
  process.env.NODE_ENV = 'production'
}

const browser = (process.env.PERF_BROWSER ?? 'chromium') as
  'chromium' | 'webkit' | 'firefox'

type CdpSession = {
  send: (method: string, params?: object) => Promise<unknown>
}

let profilerSession: CdpSession | undefined

const newCdpSession: (
  ...args: Parameters<BrowserCommand<[]>>
) => Promise<CdpSession> = async (ctx) => {
  if (ctx.provider.name !== 'playwright' || browser !== 'chromium') {
    throw new Error('CDP commands need the playwright provider on chromium')
  }

  // The playwright provider's context exposes the page under test.
  const { page } = ctx as unknown as {
    page: { context: () => { newCDPSession: (p: unknown) => unknown } }
  }

  return (await page.context().newCDPSession(page)) as CdpSession
}

const throttleCpu: BrowserCommand<[rate: number]> = async (ctx, rate) => {
  const session = await newCdpSession(ctx)
  await session.send('Emulation.setCPUThrottlingRate', { rate })
}

const startCpuProfile: BrowserCommand<[]> = async (ctx) => {
  profilerSession = await newCdpSession(ctx)
  await profilerSession.send('Profiler.enable')
  await profilerSession.send('Profiler.setSamplingInterval', { interval: 100 })
  await profilerSession.send('Profiler.start')
}

const stopCpuProfile: BrowserCommand<[file: string]> = async (_ctx, file) => {
  if (!profilerSession) {
    throw new Error('startCpuProfile was not called')
  }

  const { profile } = (await profilerSession.send('Profiler.stop')) as {
    profile: unknown
  }
  const fs = await import('node:fs/promises')
  await fs.writeFile(file, JSON.stringify(profile))
  profilerSession = undefined
}

export default defineConfig({
  root,
  mode: dev ? 'development' : 'production',
  plugins: [tailwindPlugin(), react(), Icons({ compiler: 'jsx' })],
  define: {
    OS_PLATFORM: `"${process.platform}"`,
    WIN_PORTABLE: false,
    IS_NIGHTLY: false,
  },
  resolve: {
    alias: [
      ...(dev
        ? []
        : [
            { find: /^react-dom\/client$/, replacement: 'react-dom/profiling' },
          ]),
      { find: '@root', replacement: path.resolve(root, '../../') },
      { find: '@', replacement: path.resolve(root, './src') },
      {
        find: '@interface',
        replacement: path.resolve(root, '../interface/src'),
      },
      {
        find: '@nyanpasu/interface',
        replacement: path.resolve(root, '../interface/src'),
      },
      {
        find: '@nyanpasu/utils',
        replacement: path.resolve(root, '../utils/src'),
      },
      // There is no Tauri runtime in the browser; modules that read the
      // current window at import time get a stub.
      {
        find: '@tauri-apps/api/webviewWindow',
        replacement: path.resolve(root, './perf/fixtures/webview-window.ts'),
      },
    ],
    dedupe: ['react', 'react-dom'],
  },
  test: {
    include: ['perf/**/*.bench.test.tsx'],
    testTimeout: 300_000,
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser }],
      viewport: { width: 1400, height: 900 },
      commands: { throttleCpu, startCpuProfile, stopCpuProfile },
    },
  },
})
