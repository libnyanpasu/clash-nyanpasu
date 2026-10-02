import type {
  Dimension,
  Dimensions,
  Metric,
  Rate,
  ReportRequest,
  Topology,
  TopologyEdge,
  TopologyNode,
  TopologyRequest,
  TrafficQuery,
  TrafficRange,
  TrafficReport,
  Usage,
  UsageGroup,
} from '@nyanpasu/rpc/types'

// Dev builds only: generated usage for the traffic page, aggregated the way the
// backend's report is, to preview the page without recorded traffic.

type MockRow = {
  dimensions: Dimensions
  usage: Usage
  /** Only live connections have a rate. */
  rate: Rate | null
}

const ROWS = 180

const PROCESSES = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/Applications/Safari.app/Contents/MacOS/Safari',
  'C:/Program Files (x86)/Steam/steam.exe',
  'C:/Users/me/AppData/Roaming/Telegram Desktop/Telegram.exe',
  '/usr/bin/curl',
  'unknown',
]

const LAN_SOURCES = ['127.0.0.1', '192.168.1.20', '192.168.1.35']

// A gateway's public client, so the map has a known source to draw routes from.
const PUBLIC_SOURCE = { ip: '203.0.113.50', region: 'CN' }

const INBOUNDS = ['mixed', 'tun', 'alice']

const DESTINATIONS = [
  { target: 'www.google.com', region: 'US' },
  { target: 'api.github.com', region: 'US' },
  { target: 'store.steampowered.com', region: 'US' },
  { target: 'cdn.jsdelivr.net', region: 'JP' },
  { target: 'www.bilibili.com', region: 'CN' },
  { target: 'www.baidu.com', region: 'CN' },
  { target: 'api.telegram.org', region: 'NL' },
  // Reached through a proxy only, so the core's local DNS is all that places them.
  { target: 'www.bbc.co.uk', region: 'GB', proxied: true },
  { target: 'www.spiegel.de', region: 'DE' },
  { target: 'www.naver.com', region: 'KR' },
  { target: 'www.netflix.com', region: 'SG', proxied: true },
  // Addresses the client connected to itself.
  { target: '149.154.167.51', region: 'NL' },
  { target: '192.168.1.1', region: 'unknown' },
  { target: 'printer.local', region: 'unknown' },
]

const CHAINS = [
  ['DIRECT'],
  ['HK-01', 'Auto', 'Proxy'],
  ['JP-02', 'Auto', 'Proxy'],
  ['US-03', 'Proxy'],
]

const RULES = [
  { kind: 'GeoSite', payload: 'cn' },
  { kind: 'GeoIP', payload: 'CN' },
  { kind: 'DomainSuffix', payload: 'google.com' },
  { kind: 'Match', payload: '' },
]

const PROFILES = ['mock-profile-a', 'mock-profile-b']

/** Longer ranges hold more of the same traffic. */
const RANGE_SCALE: Record<TrafficRange, number> = {
  last_hour: 1,
  last6_hours: 3,
  last24_hours: 8,
  last7_days: 30,
  last30_days: 90,
  all: 120,
}

/** A seeded generator, so every tick draws the same connections. */
function random(seed: number) {
  let state = seed >>> 0

  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function mockRows(now: number): MockRow[] {
  const next = random(0x5eed)
  const pick = <T>(items: readonly T[]) =>
    items[Math.floor(next() * items.length)]

  // Bytes grow through every ten minutes, so the page moves while watched.
  const growth = 1 + ((now / 1000) % 600) / 60

  return Array.from({ length: ROWS }, () => {
    const destination = pick(DESTINATIONS)
    const chains = destination.proxied ? pick(CHAINS.slice(1)) : pick(CHAINS)
    const fromPublic = next() < 0.1
    const literal = /^\d/.test(destination.target)
    const dialed =
      !destination.proxied &&
      (chains[0] === 'DIRECT' || literal || next() < 0.2)
    const live = next() < 0.3

    return {
      dimensions: {
        process: pick(PROCESSES),
        source: fromPublic ? PUBLIC_SOURCE.ip : pick(LAN_SOURCES),
        inbound: pick(INBOUNDS),
        target: destination.target,
        protocol: next() < 0.85 ? 'tcp' : 'udp',
        rule: pick(RULES),
        chains,
        profile: pick(PROFILES),
        source_region: fromPublic ? PUBLIC_SOURCE.region : 'unknown',
        destination_region: destination.region,
        destination_basis:
          destination.region === 'unknown'
            ? null
            : dialed
              ? 'dialed'
              : 'resolved',
      },
      usage: {
        bytes: {
          upload: Math.round(next() * 2_000_000 * growth),
          download: Math.round(next() * 20_000_000 * growth),
        },
        connections: 1,
      },
      rate: live
        ? {
            upload: Math.round(next() * 50_000),
            download: Math.round(next() * 500_000),
          }
        : null,
    }
  })
}

/** The key a row has in `dimension`, as the backend's `group_key` gives it. */
export function groupKey(d: Dimensions, dimension: Dimension): string {
  switch (dimension) {
    case 'origin':
      return d.process === 'unknown' ? d.source : d.process
    case 'process':
      return d.process
    case 'source':
      return d.source
    case 'inbound':
      return d.inbound ?? 'unknown'
    case 'target':
      return d.target
    case 'protocol':
      return d.protocol
    case 'rule':
      return d.rule.payload ? `${d.rule.kind},${d.rule.payload}` : d.rule.kind
    case 'chain':
      return d.chains.slice(1).reverse().join(' → ')
    case 'exit':
      return d.chains[0] ?? 'unknown'
    case 'profile':
      return d.profile ?? ''
    case 'source_region':
      return d.source_region ?? 'unknown'
    case 'destination_region':
      return d.destination_region ?? 'unknown'
    case 'destination_basis':
      return d.destination_basis ?? ''
  }
}

const emptyUsage = (): Usage => ({
  bytes: { upload: 0, download: 0 },
  connections: 0,
})

function addUsage(into: Usage, usage: Usage) {
  into.bytes.upload += usage.bytes.upload
  into.bytes.download += usage.bytes.download
  into.connections += usage.connections
}

const addRate = (into: Rate | null, rate: Rate | null): Rate | null =>
  !rate
    ? into
    : {
        upload: (into?.upload ?? 0) + rate.upload,
        download: (into?.download ?? 0) + rate.download,
      }

const weight = (usage: Usage, metric: Metric) =>
  metric === 'bytes'
    ? usage.bytes.upload + usage.bytes.download
    : usage.connections

const byKey = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0)

/** The rows `query` selects, with the range's share of their bytes. */
function select(rows: MockRow[], query: TrafficQuery): MockRow[] {
  const scale = RANGE_SCALE[query.range]

  return rows
    .filter(
      ({ dimensions, rate }) =>
        (query.scope === 'all' ||
          (query.scope === 'active') === (rate !== null)) &&
        query.filters.every(
          (filter) => groupKey(dimensions, filter.dimension) === filter.value,
        ),
    )
    .map((row) => ({
      ...row,
      usage: {
        bytes: {
          upload: row.usage.bytes.upload * scale,
          download: row.usage.bytes.download * scale,
        },
        connections: row.usage.connections,
      },
    }))
}

/** Every group of `dimension`, heaviest by `metric` first. */
function groups(
  rows: MockRow[],
  dimension: Dimension,
  metric: Metric,
): UsageGroup[] {
  const grouped = new Map<string, UsageGroup>()

  for (const { dimensions, usage, rate } of rows) {
    const key = groupKey(dimensions, dimension)
    const group = grouped.get(key) ?? {
      key,
      usage: emptyUsage(),
      current_rate: null,
    }
    addUsage(group.usage, usage)
    group.current_rate = addRate(group.current_rate, rate)
    grouped.set(key, group)
  }

  return [...grouped.values()].sort(
    (a, b) =>
      weight(b.usage, metric) - weight(a.usage, metric) || byKey(a.key, b.key),
  )
}

function topology(
  rows: MockRow[],
  { layers, limit_per_layer: limit }: TopologyRequest,
  metric: Metric,
): Topology {
  const value = (usage: Usage) => weight(usage, metric)

  // An empty chain skips its layer, as in the backend.
  const paths = rows.map(({ dimensions }) =>
    layers.flatMap((dimension, layer) => {
      const key = groupKey(dimensions, dimension)
      return dimension === 'chain' && key === '' ? [] : [{ layer, key }]
    }),
  )

  // Keys beyond the limit merge into their layer's node without a key.
  const kept = layers.map((_, layer) => {
    const totals = new Map<string, Usage>()
    rows.forEach((row, index) => {
      const step = paths[index].find((entry) => entry.layer === layer)
      if (step) {
        const total = totals.get(step.key) ?? emptyUsage()
        addUsage(total, row.usage)
        totals.set(step.key, total)
      }
    })
    const ranked = [...totals.entries()]
      .sort(([a, x], [b, y]) => value(y) - value(x) || byKey(a, b))
      .map(([key]) => key)
    return new Set(limit === null ? ranked : ranked.slice(0, limit))
  })

  const id = (layer: number, key: string | null) => JSON.stringify([layer, key])
  const nodes = new Map<string, TopologyNode>()
  const edges = new Map<string, TopologyEdge>()

  rows.forEach((row, index) => {
    const steps = paths[index].map(({ layer, key }) => {
      const merged = kept[layer].has(key) ? key : null
      const nodeId = id(layer, merged)
      const node = nodes.get(nodeId) ?? {
        id: nodeId,
        layer,
        key: merged,
        usage: emptyUsage(),
      }
      addUsage(node.usage, row.usage)
      nodes.set(nodeId, node)
      return nodeId
    })

    steps.slice(1).forEach((target, step) => {
      const source = steps[step]
      const edgeId = `${source}>${target}`
      const edge = edges.get(edgeId) ?? { source, target, usage: emptyUsage() }
      addUsage(edge.usage, row.usage)
      edges.set(edgeId, edge)
    })
  })

  return {
    // By layer, heaviest first, the merged node last.
    nodes: [...nodes.values()].sort(
      (a, b) =>
        a.layer - b.layer ||
        Number(a.key === null) - Number(b.key === null) ||
        value(b.usage) - value(a.usage) ||
        byKey(a.key ?? '', b.key ?? ''),
    ),
    edges: [...edges.values()],
  }
}

export function mockTrafficReport(
  request: ReportRequest,
  now: number,
): TrafficReport {
  const rows = select(mockRows(now), request.query)
  const total = emptyUsage()
  let rate: Rate | null = null

  for (const row of rows) {
    addUsage(total, row.usage)
    rate = addRate(rate, row.rate)
  }

  return {
    total,
    current_rate: request.query.scope === 'closed' ? null : rate,
    rankings: request.rankings.map((dimension) => {
      const all = groups(rows, dimension, request.metric)
      const other = emptyUsage()
      all
        .slice(request.ranking_limit)
        .forEach((group) => addUsage(other, group.usage))

      return {
        dimension,
        distinct: all.length,
        groups: all.slice(0, request.ranking_limit),
        other,
      }
    }),
    topology:
      request.topology && topology(rows, request.topology, request.metric),
  }
}

/** Every group of `dimension` that `query` selects, for the full ranking. */
export const mockTrafficGroups = (
  query: TrafficQuery,
  dimension: Dimension,
  metric: Metric,
  now: number,
) => groups(select(mockRows(now), query), dimension, metric)
