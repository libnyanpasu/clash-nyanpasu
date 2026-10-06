import { useState, type ReactNode } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import {
  dimensionName,
  largest,
  regionName,
  usageLabel,
} from '@/utils/traffic-usage'
import type {
  Dimension,
  ReportRequest,
  Topology,
  TopologyRequest,
  TrafficReport,
  UsageGroup,
  UsagePage,
} from '@nyanpasu/rpc/types'
import { focusManager, QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import {
  LAYER_TEMPLATES,
  trafficSearchSchema,
  type LayerTemplate,
  type TrafficSearch,
} from '../src/pages/(main)/main/topology/_modules/search'
import TrafficPage from '../src/pages/(main)/main/topology/_modules/traffic-page'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

const backend = vi.hoisted(() => ({
  retention: '7d',
  // The `refetchInterval` of every `useTrafficReport` call.
  intervals: [] as unknown[],
}))

vi.mock('@nyanpasu/query', async (importOriginal) => {
  const original = await importOriginal<typeof import('@nyanpasu/query')>()

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

const nodeId = (layer: number, key: string | null) =>
  JSON.stringify([layer, key])

// Every layer has one value and the node that merges the rest; every node
// reaches every node of the next layer.
function topologyFor({ layers }: TopologyRequest): Topology {
  const keys = (dimension: Dimension, index: number) =>
    dimension === 'source_region'
      ? ['CN', null]
      : dimension === 'destination_region'
        ? ['US', 'unknown']
        : dimension === 'destination_basis'
          ? rpc.basisKeys
          : [
              `${dimension}-${index}`,
              ...Array.from(
                {
                  length:
                    dimension === 'exit'
                      ? (rpc.extraExitNodes ?? rpc.extraNodes)
                      : rpc.extraNodes,
                },
                (_, extra) => `${dimension}-${index}-${extra}`,
              ),
              null,
            ]
  const nodes = layers.flatMap((dimension, layer) =>
    keys(dimension, layer).map((key) => ({
      id: nodeId(layer, key),
      layer,
      key,
      usage: usage(1024, 2048, 3),
    })),
  )
  const edges = nodes.flatMap((source) =>
    nodes
      .filter((target) => target.layer === source.layer + 1)
      .map((target) => ({
        source: source.id,
        target: target.id,
        usage: usage(512, 512, 1),
      })),
  )

  return { nodes, edges }
}

type RpcCall = { method: string; params: Record<string, unknown> }

const rpc = vi.hoisted(() => ({
  reports: [] as ReportRequest[],
  pages: [] as Array<Record<string, unknown>>,
  unavailable: false,
  // Pending while set: the map's report does not arrive until it resolves.
  holdMap: undefined as Promise<void> | undefined,
  emptyTopology: false,
  usageError: false,
  // Nodes besides the first and the merged one in each layer of a flow.
  extraNodes: 0,
  // The same for the exit layer; the default follows `extraNodes`.
  extraExitNodes: undefined as number | undefined,
  // How the map's destination regions were located.
  basisKeys: ['dialed', 'resolved'],
}))

function mockBackend(pages: Record<string, UsagePage> = {}) {
  mockIPC(async (command, args) => {
    if (command !== 'call_rpc') return

    const { method, params } = args as RpcCall

    if (method === 'query_traffic_report') {
      rpc.reports.push(params.request as ReportRequest)

      if (rpc.unavailable) throw new Error('traffic recording is unavailable')

      const { topology } = params.request as ReportRequest

      if (topology?.layers.includes('source_region')) await rpc.holdMap

      return {
        ...report,
        topology:
          topology &&
          (rpc.emptyTopology
            ? { nodes: [], edges: [] }
            : topologyFor(topology)),
      }
    }

    if (method === 'query_traffic_usage') {
      rpc.pages.push(params)

      if (rpc.usageError) throw new Error('traffic recording is unavailable')

      const after = params.after as { key: string } | null

      return pages[after?.key ?? '']
    }

    throw new Error(`unexpected rpc ${method}`)
  })
}

function Page({
  initial,
  onViewConnections,
  toolbarStart,
}: {
  initial?: Partial<TrafficSearch>
  onViewConnections?: () => void
  toolbarStart?: ReactNode
}) {
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
          onViewConnections={onViewConnections}
          toolbarStart={toolbarStart}
        />
      </div>
    </TooltipProvider>
  )
}

async function open(
  onTestFinished: (fn: () => void) => void,
  initial?: Partial<TrafficSearch>,
  props?: { onViewConnections?: () => void; toolbarStart?: ReactNode },
) {
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const view = await render(
    <QueryClientProvider client={queries}>
      <Page initial={initial} {...props} />
    </QueryClientProvider>,
  )

  onTestFinished(async () => {
    await view.unmount()
    queries.clear()
    clearMocks()
    rpc.reports = []
    rpc.pages = []
    rpc.unavailable = false
    rpc.holdMap = undefined
    rpc.emptyTopology = false
    rpc.usageError = false
    rpc.extraNodes = 0
    rpc.extraExitNodes = undefined
    rpc.basisKeys = ['dialed', 'resolved']
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
    metric: 'bytes',
    rankings: ['source', 'inbound', 'target', 'exit', 'process'],
    ranking_limit: 5,
    topology: {
      layers: ['origin', 'rule', 'chain', 'exit'],
      limit_per_layer: 7,
    },
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
  const cursor = { usage: usage(1, 2, 1), key: 'first.example' }
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
      metric: 'bytes',
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

const templateName = (template: LayerTemplate) =>
  LAYER_TEMPLATES[template].map(dimensionName).join(' → ')

const node = (
  view: Awaited<ReturnType<typeof open>>,
  dimension: Dimension,
  key: string,
) =>
  view.getByRole('button', {
    name: new RegExp(`^${dimensionName(dimension)}: ${key},`),
  })

// The browser tests run without the stylesheet, so the positioned nodes are not
// where they are drawn; the events go to the elements directly.
const press = (locator: { element: () => Element }) =>
  (locator.element() as HTMLElement).click()

const hover = (locator: { element: () => Element }) =>
  locator
    .element()
    .dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))

test('the template and the metric change the topology of the same request', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)

  const pick = async (template: LayerTemplate) => {
    await view.getByRole('combobox', { name: m.traffic_layers_label() }).click()
    await view.getByRole('option', { name: templateName(template) }).click()
  }

  await pick('source')
  await expect
    .poll(() => last().topology?.layers)
    .toEqual(['source', 'target', 'exit'])
  expect(last().topology?.limit_per_layer).toBe(7)
  // The cards share the one request; the topology rides along.
  expect(last().rankings).toHaveLength(5)

  await pick('inbound')
  await expect
    .poll(() => last().topology?.layers)
    .toEqual(['inbound', 'rule', 'exit'])

  await pick('process')
  await expect
    .poll(() => last().topology?.layers)
    .toEqual(['process', 'target', 'exit'])

  await view.getByRole('combobox', { name: m.traffic_metric_label() }).click()
  await view
    .getByRole('option', { name: m.traffic_metric_connections() })
    .click()
  await expect.poll(() => last().metric).toBe('connections')
  expect(last().topology?.layers).toEqual(['process', 'target', 'exit'])

  // The nodes are drawn from the ones the backend returned.
  await expect.element(node(view, 'process', 'process-0')).toBeInTheDocument()
  await expect.element(node(view, 'exit', 'exit-2')).toBeInTheDocument()
})

test('a node filters by its layer, the merged node does nothing', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.element(node(view, 'rule', 'rule-1')).toBeInTheDocument()

  // The merged node is no button, and a click on it changes nothing.
  await expect
    .element(
      view.getByRole('button', {
        name: new RegExp(`^${dimensionName('rule')}: ${m.topology_other()}`),
      }),
    )
    .not.toBeInTheDocument()
  await press(view.getByText(m.topology_other()).nth(1))

  await press(node(view, 'rule', 'rule-1'))
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'rule', value: 'rule-1' }])
  await expect
    .element(node(view, 'rule', 'rule-1'))
    .toHaveAttribute('aria-pressed', 'true')

  // Another layer adds its own, the same node again takes it off.
  await press(node(view, 'exit', 'exit-3'))
  await expect
    .poll(() => last().query.filters)
    .toEqual([
      { dimension: 'rule', value: 'rule-1' },
      { dimension: 'exit', value: 'exit-3' },
    ])
  await press(node(view, 'rule', 'rule-1'))
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'exit', value: 'exit-3' }])
})

test('hovering a node leaves only its adjacent edges lit', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  const target = node(view, 'rule', 'rule-1')
  await expect.element(target).toBeInTheDocument()

  const opacities = () =>
    [...document.querySelectorAll('[role="region"] svg path')].map((path) =>
      Number(getComputedStyle(path).opacity),
    )

  // Three gaps of two by two edges.
  await expect.poll(() => opacities().length).toBe(12)

  hover(target)
  // Two edges come in and two go out; the other eight fade as they animate.
  await expect
    .poll(() => [
      opacities().filter((o) => o > 0.5).length,
      opacities().filter((o) => o < 0.1).length,
    ])
    .toEqual([4, 8])
})

test('a large flow is drawn static, scrolls inside a capped height and still highlights', async ({
  onTestFinished,
}) => {
  mockBackend()
  rpc.extraNodes = 20
  const view = await open(onTestFinished)
  const target = node(view, 'rule', 'rule-1')
  await expect.element(target).toBeInTheDocument()

  const paths = () => [...document.querySelectorAll('[role="region"] svg path')]
  // Three gaps of 22 by 22 edges.
  await expect.poll(() => paths().length).toBe(1452)
  expect(
    document.querySelector<HTMLElement>('[role="region"]')?.style.maxHeight,
  ).toBe('40rem')

  hover(target)
  // 22 edges come in and 22 go out of the node; the rest is dimmed at once.
  await expect
    .poll(
      () =>
        paths().filter((path) => Number(getComputedStyle(path).opacity) > 0.5)
          .length,
    )
    .toBe(44)
})

test('a dense flow of few rows is drawn from the top of its columns', async ({
  onTestFinished,
}) => {
  mockBackend()
  // Three columns of 16 nodes and an exit column of 2: the 544 edges make it
  // large, and 16 rows would otherwise still centre the short exit column.
  rpc.extraNodes = 14
  rpc.extraExitNodes = 0
  const view = await open(onTestFinished)
  const target = node(view, 'exit', 'exit-3')
  await expect.element(target).toBeInTheDocument()

  const paths = () => [...document.querySelectorAll('[role="region"] svg path')]
  await expect.poll(() => paths().length).toBe(544)
  expect(target.element().style.top).toBe('0px')
})

test('the largest value does not spread the values into a call', () => {
  const values = Array.from({ length: 300_000 }, (_, index) => index)

  expect(largest(values, 1)).toBe(299_999)
  expect(largest([], 1)).toBe(1)
  expect(largest([0, 0], 1)).toBe(1)
})

test('a paused report is not refetched when the window regains focus', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)
  const refocus = () => {
    focusManager.setFocused(false)
    focusManager.setFocused(true)
  }

  // Polling, a focus refetches: the control for the paused case below.
  const polling = rpc.reports.length
  refocus()
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(polling)

  await view.getByRole('button', { name: m.topology_pause() }).click()
  await expect.poll(() => backend.intervals.at(-1)).toBe(false)
  const paused = rpc.reports.length
  refocus()
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(rpc.reports.length).toBe(paused)
})

test('the header controls keep their names', async ({ onTestFinished }) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.element(node(view, 'rule', 'rule-1')).toBeInTheDocument()

  await expect
    .element(view.getByRole('radiogroup', { name: m.topology_view() }))
    .toBeInTheDocument()
  await expect
    .element(view.getByRole('combobox', { name: m.traffic_layers_label() }))
    .toBeInTheDocument()
  await expect.element(limitSelect(view)).toBeInTheDocument()
})

test('the toolbar shows the connections on demand and starts with its slot', async ({
  onTestFinished,
}) => {
  mockBackend()
  const onViewConnections = vi.fn()
  const view = await open(onTestFinished, undefined, {
    onViewConnections,
    toolbarStart: <span data-testid="toolbar-start">back</span>,
  })

  await view.getByRole('button', { name: m.traffic_view_connections() }).click()
  expect(onViewConnections).toHaveBeenCalledTimes(1)

  // The slot leads the controls, before the scope.
  const start = view.getByTestId('toolbar-start').element()
  const scope = view
    .getByRole('radiogroup', { name: m.traffic_scope_label() })
    .element()
  expect(start.closest('[data-slot=traffic-toolbar]')).not.toBeNull()
  expect(start.parentElement!.firstElementChild).toBe(start)
  expect(start.nextElementSibling).toBe(scope)
})

test('without a handler the toolbar has no connections button', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect
    .element(view.getByRole('button', { name: m.topology_pause() }))
    .toBeInTheDocument()

  expect(
    document.querySelector('[data-slot=traffic-view-connections]'),
  ).toBeNull()
})

test('the toolbar metric orders the rankings and the full list', async ({
  onTestFinished,
}) => {
  mockBackend({
    '': {
      total: usage(0, 0, 0),
      groups: [],
      other: usage(0, 0, 0),
      next: null,
    },
  })
  const view = await open(onTestFinished)
  const toolbar = () =>
    document.querySelector<HTMLElement>('[data-slot="traffic-toolbar"]')!
  const inbound = view.getByRole('button', { name: /^mixed/ })

  await expect.element(inbound).toBeInTheDocument()
  expect(inbound.element().textContent).toContain('mixed3.00 KiB')

  const metric = view.getByRole('combobox', { name: m.traffic_metric_label() })
  expect(toolbar().contains(metric.element())).toBe(true)
  await metric.click()
  await view
    .getByRole('option', { name: m.traffic_metric_connections() })
    .click()

  await expect.poll(() => last().metric).toBe('connections')
  // A row leads with the metric and ends its second line with the other one.
  expect(inbound.element().textContent).toContain(
    `mixed${m.topology_connection_count({ count: 3 })}`,
  )
  expect(inbound.element().textContent).toMatch(/3\.00 KiB$/)

  await view
    .getByRole('button', { name: m.traffic_rank_view_all() })
    .nth(1)
    .click()
  await expect
    .poll(() => rpc.pages.at(-1))
    .toEqual(expect.objectContaining({ metric: 'connections' }))
})

test('the map asks for every region and a region filters the destination', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished, { view: 'map' })

  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)
  expect(rpc.reports.every((r) => r.topology?.limit_per_layer === null)).toBe(
    true,
  )
  expect(last().topology).toEqual({
    layers: ['source_region', 'destination_region', 'destination_basis'],
    limit_per_layer: null,
  })

  // The destination totals are the second layer's nodes.
  await view
    .getByRole('button', { name: new RegExp(`^${regionName('US')}`) })
    .first()
    .click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'destination_region', value: 'US' }])
})

test('the measured-only switch filters the map by how destinations were located', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished, { view: 'map' })
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)
  // Each region and each basis carries three connections.
  await expect
    .element(
      view.getByText(
        m.topology_geo_coverage({ dialed: 3, resolved: 3, total: 6 }),
      ),
    )
    .toBeInTheDocument()

  const measuredOnly = view.getByRole('button', {
    name: m.topology_geo_dialed_only(),
  })
  await expect.element(measuredOnly).toHaveAttribute('aria-pressed', 'false')
  await measuredOnly.click()
  await expect
    .poll(() => last().query.filters)
    .toEqual([{ dimension: 'destination_basis', value: 'dialed' }])
  await expect.element(measuredOnly).toHaveAttribute('aria-pressed', 'true')

  await measuredOnly.click()
  await expect.poll(() => last().query.filters).toEqual([])
})

test('a region placed only by local DNS says so', async ({
  onTestFinished,
}) => {
  rpc.basisKeys = ['resolved']
  mockBackend()
  const view = await open(onTestFinished, { view: 'map' })

  await expect
    .element(
      view.getByRole('button', {
        name: new RegExp(
          `^${regionName('US')} · .+ · ${m.topology_geo_basis_resolved()}$`,
        ),
      }),
    )
    .toBeInTheDocument()
  await expect
    .element(
      view.getByText(
        m.topology_geo_coverage({ dialed: 0, resolved: 3, total: 6 }),
      ),
    )
    .toBeInTheDocument()
})

const limitSelect = (view: Awaited<ReturnType<typeof open>>) =>
  view.getByRole('combobox', { name: m.traffic_limit_label() })

test('the limit of nodes per layer changes the request, all asks for none', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.poll(() => rpc.reports.length).toBeGreaterThan(0)

  const pick = async (name: string) => {
    await limitSelect(view).click()
    await view.getByRole('option', { name }).click()
  }

  await pick(m.traffic_limit_top({ count: 10 }))
  await expect.poll(() => last().topology?.limit_per_layer).toBe(10)

  await pick(m.traffic_limit_all())
  await expect.poll(() => last().topology?.limit_per_layer).toBeNull()
  expect(last().topology?.layers).toEqual(['origin', 'rule', 'chain', 'exit'])

  await pick(m.traffic_limit_top({ count: 5 }))
  await expect.poll(() => last().topology?.limit_per_layer).toBe(5)

  // The map has no limit to choose: it places every region.
  await view.getByRole('radio', { name: m.topology_geo_map() }).click()
  await expect
    .poll(() => last().topology?.layers)
    .toEqual(['source_region', 'destination_region', 'destination_basis'])
  expect(last().topology?.limit_per_layer).toBeNull()
  await expect.element(limitSelect(view)).not.toBeInTheDocument()
})

test('a view switch keeps the figures but never draws the other columns', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)
  await expect.element(node(view, 'rule', 'rule-1')).toBeInTheDocument()

  let release!: () => void
  rpc.holdMap = new Promise<void>((resolve) => (release = resolve))

  await view.getByRole('radio', { name: m.topology_geo_map() }).click()
  await expect
    .poll(() => last().topology?.layers)
    .toEqual(['source_region', 'destination_region', 'destination_basis'])

  // The flow's rules are no regions: the map is empty, the page still stands.
  await expect
    .element(view.getByText(m.topology_geo_title()))
    .toBeInTheDocument()
  expect(
    document.querySelectorAll(
      `[aria-label="${m.topology_geo_regions()}"] button`,
    ),
  ).toHaveLength(0)
  await expect.element(view.getByText('5.00 KiB')).toBeInTheDocument()

  release()
  await expect
    .element(
      view
        .getByRole('button', { name: new RegExp(`^${regionName('US')}`) })
        .first(),
    )
    .toBeInTheDocument()

  // And back: the regions are no columns of the flow either.
  rpc.holdMap = undefined
  await view.getByRole('radio', { name: m.topology_flow() }).click()
  await expect.element(node(view, 'rule', 'rule-1')).toBeInTheDocument()
})

test('region names are localized, anything else is shown as it is', () => {
  const names = new Intl.DisplayNames([getLocale()], { type: 'region' })

  expect(regionName('US')).toBe(names.of('US'))
  expect(regionName('419')).toBe(names.of('419'))
  expect(regionName('unknown')).toBe(m.topology_unknown())
  // Not region codes: `DisplayNames.of` would throw a RangeError for these.
  expect(regionName('rule-1')).toBe('rule-1')
  expect(regionName('Node-A')).toBe('Node-A')
  expect(regionName('')).toBe('')
})

test('region keys show the region name, and keep the code in the title', async ({
  onTestFinished,
}) => {
  expect(usageLabel('destination_region', 'US')).toEqual({
    text: regionName('US'),
    title: 'US',
    mono: false,
  })

  mockBackend()
  const view = await open(onTestFinished, {
    filters: [{ d: 'destination_region', v: 'US' }],
  })

  await expect
    .element(
      view.getByText(
        `${m.traffic_dimension_destination_region()}: ${regionName('US')}`,
      ),
    )
    .toBeInTheDocument()
})

test('a map without regions says the range is empty', async ({
  onTestFinished,
}) => {
  rpc.emptyTopology = true
  mockBackend()
  const view = await open(onTestFinished, { view: 'map' })

  await expect.element(view.getByText(m.traffic_empty())).toBeInTheDocument()
})

test('the stat card with a hint and a ranking row reveal their tooltip on focus', async ({
  onTestFinished,
}) => {
  mockBackend()
  const view = await open(onTestFinished)

  const cards = () =>
    document.querySelectorAll<HTMLElement>('[data-slot="traffic-stat"]')
  await expect.poll(() => cards().length).toBe(7)
  expect(cards()[1].tabIndex).toBe(0)
  expect(cards()[2].tabIndex).toBe(-1)

  cards()[1].focus()
  await expect
    .element(view.getByText(m.traffic_stat_connections_hint()).first())
    .toBeInTheDocument()

  // The path is more than the row shows; the keyboard reaches it too.
  const row = view.getByRole('button', { name: /^curl\.exe/ })
  await expect.element(row).toBeInTheDocument()
  ;(row.element() as HTMLElement).focus()
  await expect
    .element(view.getByText('C:/Tools/curl.exe').first())
    .toBeInTheDocument()
})

test('view all tells when the listing is unavailable or empty', async ({
  onTestFinished,
}) => {
  const empty = usage(0, 0, 0)
  rpc.usageError = true
  mockBackend({ '': { total: empty, groups: [], other: empty, next: null } })
  const view = await open(onTestFinished)

  const viewAll = (index: number) =>
    view.getByRole('button', { name: m.traffic_rank_view_all() }).nth(index)

  await viewAll(0).click()
  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(m.traffic_unavailable())
  await expect
    .element(view.getByText(m.traffic_loading_more()))
    .not.toBeInTheDocument()

  await view.getByRole('button', { name: m.common_close() }).click()
  rpc.usageError = false
  // Another dimension's listing was never fetched, so it asks again.
  await viewAll(1).click()
  await expect.element(view.getByText(m.traffic_empty())).toBeInTheDocument()
})
