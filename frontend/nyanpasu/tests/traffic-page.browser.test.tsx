import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import type {
  ReportRequest,
  TrafficReport,
  UsageGroup,
  UsagePage,
} from '@nyanpasu/interface'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import {
  trafficSearchSchema,
  type TrafficSearch,
} from '../src/pages/(main)/main/topology/_modules/search'
import TrafficPage from '../src/pages/(main)/main/topology/_modules/traffic-page'

// The topology card keeps its own data source until it moves to the report.
vi.mock('../src/pages/(main)/main/topology/_modules/topology-view', () => ({
  default: () => null,
}))

const backend = vi.hoisted(() => ({
  retention: '7d',
  // The `refetchInterval` of every `useTrafficReport` call.
  intervals: [] as unknown[],
}))

vi.mock('@nyanpasu/interface', async (importOriginal) => {
  const original = await importOriginal<typeof import('@nyanpasu/interface')>()

  return {
    ...original,
    useTrafficReport: (
      ...args: Parameters<typeof original.useTrafficReport>
    ) => {
      backend.intervals.push(args[1]?.refetchInterval)
      return original.useTrafficReport(...args)
    },
    useSetting: () => ({ value: backend.retention }),
    useProfile: () => ({
      query: { data: { items: [{ uid: 'main', name: 'Main profile' }] } },
    }),
    useClashConnectionDetails: () => ({
      data: { connections: [] },
      isLoading: false,
    }),
    useClashWSStatus: () => ({ error: null }),
  }
})

const usage = (upload: number, download: number, connections: number) => ({
  bytes: { upload, download },
  connections,
})

const group = (key: string, bytes = 1024): UsageGroup => ({
  key,
  usage: usage(bytes, bytes * 2, 3),
  current_rate: null,
})

const ranking = (
  dimension: TrafficReport['rankings'][number]['dimension'],
  groups: UsageGroup[],
  distinct = groups.length,
) => ({ dimension, distinct, groups, other: usage(512, 512, 1) })

const report: TrafficReport = {
  total: usage(1024, 4096, 12),
  current_rate: null,
  rankings: [
    ranking('source', [group('192.168.1.2', 4096), group('unknown', 512)], 7),
    ranking('inbound', [group('mixed')]),
    ranking('target', [group('example.com', 4096), group('api.example.com')]),
    ranking('exit', [group('Node-A')]),
    ranking('process', [group('C:/Tools/curl.exe')]),
  ],
  topology: null,
}

type RpcCall = { method: string; params: Record<string, unknown> }

const rpc = vi.hoisted(() => ({
  reports: [] as ReportRequest[],
  pages: [] as Array<Record<string, unknown>>,
  unavailable: false,
}))

function mockBackend(pages: Record<string, UsagePage> = {}) {
  mockIPC((command, args) => {
    if (command !== 'call_rpc') return

    const { method, params } = args as RpcCall

    if (method === 'query_traffic_report') {
      rpc.reports.push(params.request as ReportRequest)

      if (rpc.unavailable) throw new Error('traffic recording is unavailable')

      return report
    }

    if (method === 'query_traffic_usage') {
      rpc.pages.push(params)

      const after = params.after as { key: string } | null

      return pages[after?.key ?? '']
    }

    throw new Error(`unexpected rpc ${method}`)
  })
}

function Page({ initial }: { initial?: Partial<TrafficSearch> }) {
  const [search, setSearch] = useState(() =>
    trafficSearchSchema.parse(initial ?? {}),
  )

  return (
    <TooltipProvider>
      <div className="flex h-screen flex-col">
        <TrafficPage
          search={search}
          onSearchChange={(update) =>
            setSearch((previous) => ({ ...previous, ...update }))
          }
        />
      </div>
    </TooltipProvider>
  )
}

async function open(
  onTestFinished: (fn: () => void) => void,
  initial?: Partial<TrafficSearch>,
) {
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider client={queries}>
      <Page initial={initial} />
    </QueryClientProvider>,
  )

  onTestFinished(async () => {
    await view.unmount()
    queries.clear()
    clearMocks()
    rpc.reports = []
    rpc.pages = []
    rpc.unavailable = false
    backend.retention = '7d'
    backend.intervals = []
  })

  return view
}

const last = () => rpc.reports.at(-1)!

test('the first request is the last hour of everything, unfiltered', async ({
  onTestFinished,
}) => {
  mockBackend()
  await open(onTestFinished)

  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)
  expect(rpc.reports[0]).toEqual({
    query: { range: 'last_hour', scope: 'all', filters: [] },
    rankings: ['source', 'inbound', 'target', 'exit', 'process'],
    ranking_limit: 5,
    topology: null,
  })
  // One report serves every card: the five rankings are one request.
  expect(rpc.reports.every((r) => r.rankings.length === 5)).toBe(true)

  // Devices counts the distinct sources, not the rows shown.
  await expect
    .poll(() =>
      [...document.querySelectorAll('[data-slot="traffic-stat"]')].map(
        (card) => card.textContent,
      ),
    )
    .toEqual([
      expect.stringContaining('5.00 KiB'),
      expect.stringContaining('12'),
      expect.stringContaining('7'),
      expect.stringContaining('1'),
      expect.stringContaining('2'),
      expect.stringContaining('1'),
    ])
})

test('the scope and the time range change the next request', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)

  await view.getByRole('radio', { name: m.traffic_scope_active() }).click()
  await expect.poll(() => last().query.scope).toBe('active')

  await view.getByRole('radio', { name: m.traffic_scope_closed() }).click()
  await expect.poll(() => last().query.scope).toBe('closed')

  await view.getByRole('combobox', { name: m.traffic_range_label() }).click()
  await view
    .getByRole('option', { name: m.traffic_range_last6_hours() })
    .click()
  await expect.poll(() => last().query.range).toBe('last6_hours')
  expect(last().query.scope).toBe('closed')
})

test('polls every 2 s up to 24 hours, every 10 s beyond, and not when paused', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.poll(() => backend.intervals.at(-1)).toBe(2_000)

  await view.getByRole('button', { name: m.topology_pause() }).click()
  await expect.poll(() => backend.intervals.at(-1)).toBe(false)
  await expect
    .element(view.getByRole('button', { name: m.topology_resume() }))
    .toHaveAttribute('aria-pressed', 'true')

  await view.getByRole('button', { name: m.topology_resume() }).click()
  await expect.poll(() => backend.intervals.at(-1)).toBe(2_000)

  await view.getByRole('combobox', { name: m.traffic_range_label() }).click()
  await view.getByRole('option', { name: m.traffic_range_last7_days() }).click()
  await expect.poll(() => backend.intervals.at(-1)).toBe(10_000)
})

test('a ranking row toggles its filter, and a chip or clear all removes it', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  const filters = view.getByRole('group', { name: m.traffic_filters_label() })
  const example = view.getByRole('button', { name: /^example\.com/ })
  await expect.element(example).toHaveAttribute('aria-pressed', 'false')

  await example.click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'target', value: 'example.com' }])
  await expect
    .element(filters.getByText(`${m.traffic_dimension_target()}: example.com`))
    .toBeInTheDocument()
  await expect.element(example).toHaveAttribute('aria-pressed', 'true')

  // Another value of the same dimension replaces the filter.
  await view.getByRole('button', { name: /^api\.example\.com/ }).click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'target', value: 'api.example.com' }])

  await view.getByRole('button', { name: /^mixed/ }).click()
  await expect.poll(() => last().query.filters).toHaveLength(2)

  await view
    .getByRole('button', {
      name: m.traffic_filter_remove({
        filter: `${m.traffic_dimension_target()}: api.example.com`,
      }),
    })
    .click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'inbound', value: 'mixed' }])

  // The same row again takes the filter off.
  await view.getByRole('button', { name: /^mixed/ }).click()
  await expect.poll(() => last().query.filters).toEqual([])

  await view.getByRole('button', { name: /^example\.com/ }).click()
  await view.getByRole('button', { name: /^mixed/ }).click()
  await expect.poll(() => last().query.filters).toHaveLength(2)
  await view.getByRole('button', { name: m.traffic_filter_clear_all() }).click()
  await expect.poll(() => last().query.filters).toEqual([])
  await expect
    .element(view.getByRole('button', { name: m.traffic_filter_clear_all() }))
    .not.toBeInTheDocument()
})

test('labels: process file name, monospace addresses, unknown and deleted profile', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished, {
    filters: [
      { d: 'profile', v: 'main' },
      { d: 'rule', v: 'Match' },
    ],
  })

  // The path only shows in the tooltip.
  await expect
    .element(view.getByRole('button', { name: /^curl\.exe/ }))
    .toBeInTheDocument()
  expect(document.body.textContent).not.toContain('C:/Tools/curl.exe')
  await expect.element(view.getByText('192.168.1.2')).toHaveClass('font-mono')
  await expect
    .element(view.getByText(m.topology_unknown(), { exact: true }))
    .toBeInTheDocument()
  await expect
    .element(view.getByText(`${m.traffic_dimension_profile()}: Main profile`))
    .toBeInTheDocument()
})

test('a profile that is gone shows its uid and says so', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished, {
    filters: [{ d: 'profile', v: 'gone' }],
  })

  await expect
    .element(
      view.getByText(
        `${m.traffic_dimension_profile()}: ${m.traffic_profile_deleted({ uid: 'gone' })}`,
      ),
    )
    .toBeInTheDocument()
})

test('ranges longer than the retention are disabled, except all time', async ({
  onTestFinished,
}) => {
  backend.retention = '1d'
  mockBackend()
  const view = await open(onTestFinished)

  await view.getByRole('combobox', { name: m.traffic_range_label() }).click()
  const option = (name: string | RegExp) => view.getByRole('option', { name })

  await expect
    .element(option(m.traffic_range_last24_hours()))
    .not.toHaveAttribute('aria-disabled', 'true')
  await expect
    .element(option(m.traffic_range_all()))
    .not.toHaveAttribute('aria-disabled', 'true')
  for (const name of [
    m.traffic_range_last7_days(),
    m.traffic_range_last30_days(),
  ]) {
    await expect
      .element(option(new RegExp(name)))
      .toHaveAttribute('aria-disabled', 'true')
  }
  await expect
    .element(view.getByText(m.traffic_range_beyond_retention()).first())
    .toBeInTheDocument()
})

test('a 7 day retention only disables the 30 day range', async ({
  onTestFinished,
}) => {
  backend.retention = '7d'
  mockBackend()
  const view = await open(onTestFinished)

  await view.getByRole('combobox', { name: m.traffic_range_label() }).click()

  await expect
    .element(
      view.getByRole('option', {
        name: new RegExp(m.traffic_range_last30_days()),
      }),
    )
    .toHaveAttribute('aria-disabled', 'true')
  await expect
    .element(view.getByRole('option', { name: m.traffic_range_last7_days() }))
    .not.toHaveAttribute('aria-disabled', 'true')
})

test('unavailable recording shows the error card instead of the figures', async ({
  onTestFinished,
}) => {
  rpc.unavailable = true
  mockBackend()
  const view = await open(onTestFinished)

  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(m.traffic_unavailable())
  await expect
    .element(view.getByText(m.traffic_rank_view_all()))
    .not.toBeInTheDocument()
})

test('view all pages through every value and a pick filters and closes', async ({
  onTestFinished,
}) => {
  const cursor = { bytes: { upload: 1, download: 2 }, key: 'first.example' }
  const empty = usage(0, 0, 0)
  mockBackend({
    '': {
      total: empty,
      groups: [group('first.example', 4096)],
      other: empty,
      next: cursor,
    },
    'first.example': {
      total: empty,
      groups: [group('second.example')],
      other: empty,
      next: null,
    },
  })
  const view = await open(onTestFinished, {
    scope: 'closed',
    filters: [
      { d: 'target', v: 'example.com' },
      { d: 'inbound', v: 'mixed' },
    ],
  })

  // The host card is the third of the five.
  await view
    .getByRole('button', { name: m.traffic_rank_view_all() })
    .nth(2)
    .click()

  await expect
    .element(view.getByRole('button', { name: /^second\.example/ }))
    .toBeInTheDocument()
  expect(rpc.pages).toEqual([
    {
      // The dimension's own filter is off: every value is a choice.
      query: {
        range: 'last_hour',
        scope: 'closed',
        filters: [{ dimension: 'inbound', value: 'mixed' }],
      },
      groupBy: 'target',
      after: null,
      limit: 50,
    },
    expect.objectContaining({ after: cursor }),
  ])

  await view.getByRole('button', { name: /^second\.example/ }).click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([
      { dimension: 'target', value: 'second.example' },
      { dimension: 'inbound', value: 'mixed' },
    ])
  await expect
    .element(view.getByRole('button', { name: m.common_close() }))
    .not.toBeInTheDocument()
})
