import { expect, test } from 'vitest'
import type {
  ClashConnectionItem,
  ClashConnectionMetadata,
} from '../../interface/src/ipc/use-clash-connections'
import { buildTopology } from '../src/pages/(main)/main/topology/_modules/topology.ts'

const connection = (
  id: string,
  overrides: Omit<Partial<ClashConnectionItem>, 'metadata'> & {
    metadata?: Partial<ClashConnectionMetadata> | null
  } = {},
): ClashConnectionItem => ({
  id,
  start: '',
  upload: 20,
  download: 80,
  chains: ['Exit', 'Nested', 'Select'],
  rule: 'Domain',
  rulePayload: 'example.com',
  ...overrides,
  // The null case exercises malformed runtime data despite the wire type.
  metadata: (overrides.metadata === null
    ? null
    : {
        network: 'tcp',
        type: 'HTTP',
        host: '',
        sourcePort: '',
        destinationPort: '',
        process: 'Browser',
        sourceIP: '127.0.0.1',
        ...overrides.metadata,
      }) as ClashConnectionMetadata,
})

test('preserves logical chain order and isolates identical names across layers', () => {
  const graph = buildTopology(
    [
      connection('1', {
        metadata: { process: 'Exit' },
        chains: ['Exit', 'Nested', 'Select'],
      }),
    ],
    'connections',
  )
  expect(graph.layers[2][0].label).toBe('Select → Nested')
  expect(graph.layers[0][0].id).not.toBe(graph.layers[3][0].id)
  expect(graph.links.length).toBe(3)
  expect(new Set(graph.layers.flat().map((node) => node.id)).size).toBe(4)
})

test('other buckets preserve every connection and byte at every layer', () => {
  const connections = Array.from({ length: 80 }, (_, i) =>
    connection(String(i), {
      metadata: { process: `app-${i}` },
      chains: [`exit-${i}`, `group-${i}`],
      rulePayload: `domain-${i}`,
      upload: i,
      download: i * 2,
    }),
  )
  for (const metric of ['bytes', 'connections'] as const) {
    const graph = buildTopology(connections, metric)
    const expectedBytes = connections.reduce(
      (sum, c) => sum + c.upload + c.download,
      0,
    )
    for (const layer of graph.layers) {
      expect(layer.length).toBe(7)
      expect(layer.reduce((sum, n) => sum + n.count, 0)).toBe(80)
      expect(layer.reduce((sum, n) => sum + n.bytes, 0)).toBe(expectedBytes)
      expect(new Set(layer.flatMap((n) => [...n.connectionIds])).size).toBe(80)
    }
    for (let i = 0; i < 3; i++) {
      const ids = new Set(graph.layers[i].map((n) => n.id))
      const links = graph.links.filter((link) => ids.has(link.source))
      expect(links.reduce((sum, link) => sum + link.bytes, 0)).toBe(
        expectedBytes,
      )
      expect(links.reduce((sum, link) => sum + link.count, 0)).toBe(80)
    }
  }
})

test('traffic ranking differs from count ranking without changing totals', () => {
  const connections = [
    connection('a'),
    connection('b'),
    connection('c', {
      metadata: { process: 'Download' },
      download: 10000,
    }),
  ]
  expect(buildTopology(connections, 'connections').layers[0][0].label).toBe(
    'Browser',
  )
  expect(buildTopology(connections, 'bytes').layers[0][0].label).toBe(
    'Download',
  )
})

test('missing metadata, direct routes, and zero traffic remain representable', () => {
  const graph = buildTopology(
    [
      connection('a', {
        metadata: null,
        chains: ['DIRECT'],
        rule: '',
        upload: -5,
        download: 0,
      }),
    ],
    'bytes',
  )
  expect(graph.layers[0][0].label).toBe(null)
  expect(graph.layers[2][0].label).toBe(null)
  expect(graph.layers[3][0].label).toBe('DIRECT')
  expect(graph.links[0].bytes).toBe(0)
  expect(graph.links[0].count).toBe(1)
  expect(buildTopology([], 'bytes')).toEqual({
    layers: [[], [], [], []],
    links: [],
  })
})

test('a selected aggregate retains exact membership and input order does not affect ranking', () => {
  const connections = [
    connection('a'),
    connection('b', { metadata: { process: 'Other' } }),
  ]
  const graph = buildTopology(connections, 'connections')
  expect([
    ...graph.layers[0].find((n) => n.label === 'Browser')!.connectionIds,
  ]).toEqual(['a'])
  expect(graph.layers.map((layer) => layer.map((n) => n.id))).toEqual(
    buildTopology([...connections].reverse(), 'connections').layers.map(
      (layer) => layer.map((n) => n.id),
    ),
  )
})

// Geography consumes only core metadata, never proxy names or host guesses.
const { buildGeography, connectionRegion } =
  await import('../src/pages/(main)/main/topology/_modules/geography.ts')
const knownRegions = new Set(['US', 'JP', 'SG', 'HK'])

test('geography conserves totals and leaves missing or ambiguous regions unknown', () => {
  const connections = [
    connection('a', { metadata: { destinationGeoIP: ['us'] } }),
    connection('b', { metadata: { destinationGeoIP: ['US', 'US'] } }),
    connection('c', { metadata: { destinationGeoIP: ['US', 'JP'] } }),
    connection('d', { metadata: { destinationGeoIP: ['LAN'] } }),
    connection('e', { metadata: null }),
  ]
  const result = buildGeography(connections, knownRegions)
  expect(result.regions.find((region) => region.code === 'US')).toEqual({
    code: 'US',
    count: 2,
    bytes: 200,
  })
  expect(result.regions.find((region) => region.code === 'unknown')).toEqual({
    code: 'unknown',
    count: 3,
    bytes: 300,
  })
  expect(
    result.regions.reduce((total, region) => total + region.bytes, 0),
  ).toBe(500)
  expect(result.routes.length).toBe(0)
})

test('geographic routes require both known endpoints and never guess a proxy exit', () => {
  const connections = [
    connection('a', {
      metadata: { sourceGeoIP: ['HK'], destinationGeoIP: ['JP'] },
    }),
    connection('b', {
      metadata: { sourceGeoIP: ['HK'], destinationGeoIP: ['JP'] },
    }),
    connection('c', {
      metadata: { sourceIP: '127.0.0.1', destinationGeoIP: ['SG'] },
      chains: ['🇺🇸 US server'],
    }),
    connection('d', {
      metadata: { sourceGeoIP: ['JP'], destinationGeoIP: ['JP'] },
    }),
  ]
  const result = buildGeography(connections, knownRegions)
  expect(result.routes).toEqual([
    { code: 'HK:JP', source: 'HK', destination: 'JP', count: 2, bytes: 200 },
  ])
  expect(connectionRegion(connections[2], 'source', knownRegions)).toBe(
    'unknown',
  )
  expect(
    connectionRegion(connection('missing'), 'destination', knownRegions),
  ).toBe('unknown')
  expect(buildGeography([], knownRegions)).toEqual({ regions: [], routes: [] })
})

test('bundled map provides finite positions including common small regions', async () => {
  const { readFile } = await import('node:fs/promises')
  const map: { centers: Record<string, [number, number]>; outline: string } =
    JSON.parse(
      await readFile(
        new URL('../src/assets/maps/world-map.json', import.meta.url),
        'utf8',
      ),
    )
  for (const code of ['US', 'JP', 'SG', 'HK', 'FR'])
    expect(map.centers[code]).toBeTruthy()
  for (const [x, y] of Object.values(map.centers)) {
    expect(Number.isFinite(x) && x >= 0 && x <= 960).toBeTruthy()
    expect(Number.isFinite(y) && y >= 0 && y <= 460).toBeTruthy()
  }
  expect(map.outline.startsWith('M')).toBeTruthy()
})
