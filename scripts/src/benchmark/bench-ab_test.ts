import assert from 'node:assert/strict'
import { test } from 'node:test'
import { compare, parseReports } from './bench-ab.ts'

const line = (action: string, render: number, busy: number) =>
  `PERF_REPORT ${JSON.stringify({
    browser: 'chromium',
    action,
    avgReactRender: render,
    avgMaxBusy: busy,
    avgSyncTime: 1,
  })}`

test('parseReports reads only the report lines', () => {
  const output = [
    'stdout | perf/traffic.bench.test.tsx',
    line('open:overview', 12.5, 20),
    ' ✓ traffic pages',
  ].join('\n')

  assert.deepStrictEqual(
    parseReports(output).map((report) => report.action),
    ['open:overview'],
  )
})

test("compare lists each variant's range and the ratio per round", () => {
  const runs = [
    { variant: 'A' as const, reports: parseReports(line('open:map', 10, 20)) },
    { variant: 'B' as const, reports: parseReports(line('open:map', 6, 9)) },
    { variant: 'A' as const, reports: parseReports(line('open:map', 12, 18)) },
    { variant: 'B' as const, reports: parseReports(line('open:map', 5, 9)) },
  ]

  assert.deepStrictEqual(compare(runs), [
    'open:map avgReactRender: A 10–12 | B 5–6 | B/A 0.51',
    'open:map avgMaxBusy: A 18–20 | B 9–9 | B/A 0.47',
    'open:map avgSyncTime: A 1–1 | B 1–1 | B/A 1.00',
  ])
})
