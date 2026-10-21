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

type CpuProfile = {
  nodes: { id: number; children?: number[] }[]
  startTime: number
  endTime: number
  samples: number[]
  timeDeltas: number[]
}

// Pieces recorded before a pause, joined into one profile when it stops.
let profilePieces: CpuProfile[] = []

const joinProfiles = ([first, ...rest]: CpuProfile[]): CpuProfile =>
  rest.reduce((joined, piece) => {
    const offset = Math.max(...joined.nodes.map((node) => node.id))
    const id = (node: number) => node + offset

    return {
      nodes: [
        ...joined.nodes,
        ...piece.nodes.map((node) => ({
          ...node,
          id: id(node.id),
          children: node.children?.map(id),
        })),
      ],
      startTime: joined.startTime,
      endTime: piece.endTime,
      samples: [...joined.samples, ...piece.samples.map(id)],
      // The pause shows as a gap before the piece's first sample.
      timeDeltas: [
        ...joined.timeDeltas,
        ...piece.timeDeltas.map((delta, index) =>
          index === 0 ? delta + piece.startTime - joined.endTime : delta,
        ),
      ],
    }
  }, first)

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

// With `PERF_TRACE=1` the profile commands record a Chromium trace instead,
// which splits what a CPU profile shows as "(program)" into style, layout,
// paint and compositing, task by task.
const tracing = process.env.PERF_TRACE === '1'

type TracingBrowser = {
  startTracing: (
    page: unknown,
    options: { categories: string[] },
  ) => Promise<void>
  stopTracing: () => Promise<Uint8Array>
}

const tracerOf = (ctx: Parameters<BrowserCommand<[]>>[0]) => {
  if (ctx.provider.name !== 'playwright' || browser !== 'chromium') {
    throw new Error('tracing needs the playwright provider on chromium')
  }

  const { page } = ctx as unknown as {
    page: { context: () => { browser: () => TracingBrowser } }
  }

  return { page, browser: page.context().browser() }
}

let tracer: TracingBrowser | undefined
// Traces recorded before a pause, each written to a file of its own.
let tracePieces: Uint8Array[] = []

const startCpuProfile: BrowserCommand<[]> = async (ctx) => {
  if (tracing) {
    const { page, browser } = tracerOf(ctx)
    tracer = browser
    await browser.startTracing(page, {
      categories: [
        'devtools.timeline',
        'disabled-by-default-devtools.timeline',
        'blink.user_timing',
      ],
    })
    return
  }

  profilerSession ??= await newCdpSession(ctx)
  await profilerSession.send('Profiler.enable')
  await profilerSession.send('Profiler.setSamplingInterval', { interval: 100 })
  await profilerSession.send('Profiler.start')
}

const takeCpuProfile = async () => {
  if (!profilerSession) {
    throw new Error('startCpuProfile was not called')
  }

  const { profile } = (await profilerSession.send('Profiler.stop')) as {
    profile: CpuProfile
  }

  return profile
}

const takeTrace = async () => {
  if (!tracer) {
    throw new Error('startCpuProfile was not called')
  }

  return tracer.stopTracing()
}

/** Stops recording until the next `startCpuProfile`, into the same profile. */
const pauseCpuProfile: BrowserCommand<[]> = async () => {
  if (tracing) {
    tracePieces.push(await takeTrace())
  } else {
    profilePieces.push(await takeCpuProfile())
  }
}

/**
 * Writes the profile to `file`. A trace paused in between is written as one
 * file per piece, numbered before the extension: `switch-1.json`, ...
 */
const stopCpuProfile: BrowserCommand<[file: string]> = async (_ctx, file) => {
  const fs = await import('node:fs/promises')

  if (tracing) {
    const pieces = [...tracePieces, await takeTrace()]
    const { dir, name, ext } = path.parse(file)
    await Promise.all(
      pieces.map((piece, index) =>
        fs.writeFile(
          pieces.length === 1
            ? file
            : path.join(dir, `${name}-${index + 1}${ext || '.json'}`),
          piece,
        ),
      ),
    )
    tracer = undefined
    tracePieces = []
    return
  }

  const profile = joinProfiles([...profilePieces, await takeCpuProfile()])
  await fs.writeFile(file, JSON.stringify(profile))
  profilerSession = undefined
  profilePieces = []
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
      // There is no Tauri runtime in the browser; modules that read the
      // current window at import time get a stub.
      {
        find: '@tauri-apps/api/webviewWindow',
        replacement: path.resolve(root, './perf/fixtures/webview-window.ts'),
      },
    ],
    dedupe: ['react', 'react-dom', '@tanstack/react-query'],
  },
  // Dependencies only some benchmarks import. Vite otherwise discovers them
  // mid-run and reloads the page, which breaks the module mocks of the
  // benchmark that is running.
  optimizeDeps: {
    include: [
      '@dnd-kit/helpers',
      '@dnd-kit/react',
      '@dnd-kit/react/sortable',
      '@tanstack/react-table',
      '@uidotdev/usehooks',
      'change-case',
      '@nyanpasu/ui > radix-ui/internal',
    ],
  },
  test: {
    include: ['perf/**/*.bench.test.tsx'],
    // The render census hooks React before it loads.
    setupFiles: [
      'perf/fixtures/render-census.ts',
      'perf/fixtures/app-setup.ts',
    ],
    testTimeout: 300_000,
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser }],
      viewport: { width: 1400, height: 900 },
      commands: {
        throttleCpu,
        startCpuProfile,
        pauseCpuProfile,
        stopCpuProfile,
      },
    },
  },
})
