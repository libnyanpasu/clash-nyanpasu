import { expect, test } from 'vitest'
import type {
  Dimension,
  ReportRequest,
  TrafficFilter,
  TrafficScope,
  Usage,
} from '@nyanpasu/rpc/types'
import {
  mockTrafficGroups,
  mockTrafficReport,
} from '../src/pages/(main)/main/topology/_modules/mock-traffic'

const NOW = 1_700_000_000_000

const request = (
  overrides: {
    filters?: TrafficFilter[]
    scope?: TrafficScope
    rankings?: Dimension[]
    topology?: ReportRequest['topology']
  } = {},
): ReportRequest => ({
  query: {
    range: 'last_hour',
    scope: overrides.scope ?? 'all',
    filters: overrides.filters ?? [],
  },
  rankings: overrides.rankings ?? ['destination_region'],
  ranking_limit: 5,
  topology: overrides.topology ?? null,
})

const map = request({
  topology: {
    layers: ['source_region', 'destination_region', 'destination_basis'],
    metric: 'bytes',
    limit_per_layer: null,
  },
})

const sum = (usages: Usage[]) =>
  usages.reduce(
    (total, { bytes, connections }) => ({
      bytes: {
        upload: total.bytes.upload + bytes.upload,
        download: total.bytes.download + bytes.download,
      },
      connections: total.connections + connections,
    }),
    { bytes: { upload: 0, download: 0 }, connections: 0 },
  )

test('the same moment generates the same report', () => {
  expect(mockTrafficReport(map, NOW)).toEqual(mockTrafficReport(map, NOW))
})

test('a ranking and its other add up to the total', () => {
  const report = mockTrafficReport(request(), NOW)
  const [ranking] = report.rankings

  expect(ranking.groups).toHaveLength(5)
  expect(ranking.distinct).toBeGreaterThan(5)
  expect(sum([...ranking.groups.map((g) => g.usage), ranking.other])).toEqual(
    report.total,
  )
  expect(
    mockTrafficGroups(request().query, 'destination_region', NOW),
  ).toHaveLength(ranking.distinct)
})

test('the map splits regions by basis and leaves unknown regions without one', () => {
  const { topology } = mockTrafficReport(map, NOW)
  const keys = (layer: number) =>
    new Set(topology!.nodes.filter((n) => n.layer === layer).map((n) => n.key))

  expect(keys(2)).toEqual(new Set(['dialed', 'resolved', '']))
  expect(keys(1).has('unknown')).toBe(true)
  // Known sources exist, so the map has routes to draw.
  expect(keys(0).has('CN')).toBe(true)
  for (const edge of topology!.edges) {
    const layer = (id: string) => JSON.parse(id)[0] as number
    expect(layer(edge.target)).toBe(layer(edge.source) + 1)
  }
})

test('filters, including the basis, select every report part alike', () => {
  const report = mockTrafficReport(
    request({
      filters: [{ dimension: 'destination_basis', value: 'dialed' }],
      rankings: ['destination_basis'],
      topology: map.topology,
    }),
    NOW,
  )

  expect(report.rankings[0].groups.map((g) => g.key)).toEqual(['dialed'])
  expect(
    report.topology!.nodes.filter((n) => n.layer === 2).map((n) => n.key),
  ).toEqual(['dialed'])
})

test('only live connections have a rate, and closed ones none', () => {
  expect(
    mockTrafficReport(request({ scope: 'closed' }), NOW).current_rate,
  ).toBe(null)
  const active = mockTrafficReport(
    request({ scope: 'active', rankings: ['exit'] }),
    NOW,
  )
  expect(active.current_rate).not.toBe(null)
  expect(active.rankings[0].groups.every((g) => g.current_rate)).toBe(true)
})

test('a limited flow merges the rest of each layer into one node', () => {
  const { topology, total } = mockTrafficReport(
    request({
      topology: {
        layers: ['origin', 'target'],
        metric: 'connections',
        limit_per_layer: 2,
      },
    }),
    NOW,
  )
  const layer = topology!.nodes.filter((n) => n.layer === 1)

  expect(layer.map((n) => n.key === null)).toEqual([false, false, true])
  expect(sum(layer.map((n) => n.usage))).toEqual(total)
})
