import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { test } from 'vitest'
import { server } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import DiffViewer from '@/pages/(main)/main/profiles/inspect/_modules/diff-viewer'
import YamlViewer from '@/pages/(main)/main/profiles/inspect/_modules/yaml-viewer'
import { QueryClient } from '@tanstack/react-query'
import { TestQueryProvider as QueryClientProvider } from '../tests/query-provider'
import { createRuntimeDiff, createRuntimeYaml } from './fixtures/runtime-yaml'
import { measureFrames, onProfilerRender, summarize } from './measure'

// Switches the runtime inspector between its YAML and Diff views, as the
// view toggle does (each switch mounts the viewer afresh), over a large
// runtime config, and reports the frame drops each switch causes and when
// the view got highlighted (0: it stayed plain). See `perf/README.md`.

const env = import.meta.env
const PROXIES = Number(env.VITE_PERF_NODES ?? 1500)
const RULES = Number(env.VITE_PERF_RULES ?? 10000)
const SWITCHES = Number(env.VITE_PERF_SWITCHES ?? 6)
const WARMUP = 1

const yaml = createRuntimeYaml(PROXIES, RULES)
const hunks = createRuntimeDiff(yaml)

test(`switch the inspector views over ${yaml.split('\n').length} lines`, async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  onTestFinished(() => root.unmount())

  const queries = new QueryClient()
  const show = (view: 'yaml' | 'diff' | 'none') =>
    root.render(
      <QueryClientProvider client={queries}>
        <div className="flex flex-col" style={{ height: 850, width: 1100 }}>
          <Profiler id="inspect" onRender={onProfilerRender}>
            {view === 'yaml' ? (
              <YamlViewer
                key="yaml"
                code={yaml}
                label="YAML"
                cacheKey={['bench', 0]}
              />
            ) : view === 'diff' ? (
              <DiffViewer key="diff" hunks={hunks} />
            ) : null}
          </Profiler>
        </div>
      </QueryClientProvider>,
    )
  // Highlighted output carries inline token colors.
  const highlighted = () =>
    container.querySelector('[style*="--shiki-dark"]') !== null

  const samples = { yaml: [] as number[], diff: [] as number[] }
  const frames = { yaml: [], diff: [] } as Record<
    'yaml' | 'diff',
    Awaited<ReturnType<typeof measureFrames>>[]
  >

  for (let i = 0; i < SWITCHES; i++) {
    for (const view of ['yaml', 'diff'] as const) {
      show('none')
      await new Promise((resolve) => setTimeout(resolve, 200))
      const start = performance.now()
      let ready = 0
      const poll = setInterval(() => {
        if (!ready && highlighted()) ready = performance.now() - start
      }, 10)
      frames[view].push(await measureFrames(() => show(view), 3000))
      clearInterval(poll)
      samples[view].push(ready)
    }
  }

  const report = Object.fromEntries(
    (['yaml', 'diff'] as const).map((view) => [
      view,
      {
        avgTimeToHighlight: Number(
          (
            samples[view].slice(WARMUP).reduce((a, b) => a + b, 0) /
            (SWITCHES - WARMUP)
          ).toFixed(0),
        ),
        ...summarize(frames[view].slice(WARMUP)),
      },
    ]),
  )

  console.log(
    `PERF_REPORT ${JSON.stringify({
      browser: server.browser,
      lines: yaml.split('\n').length,
      hunks: hunks.length,
      ...report,
    })}`,
  )
})
