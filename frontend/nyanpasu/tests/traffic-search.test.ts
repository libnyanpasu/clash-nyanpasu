import { expect, test } from 'vitest'
import { beyondRetention } from '@/utils/traffic-retention'
import {
  setFilter,
  toggleFilter,
  toTrafficFilters,
} from '../src/pages/(main)/main/_modules/traffic-filters'
import {
  pollInterval,
  toQuery,
  toTopologyRequest,
  trafficSearchSchema,
} from '../src/pages/(main)/main/topology/_modules/search'

test('an empty search is the last hour of everything', () => {
  const search = trafficSearchSchema.parse({})

  expect(search).toEqual({
    range: 'last_hour',
    scope: 'all',
    filters: [],
    view: 'flow',
    metric: 'bytes',
    limit: 7,
  })
  expect(toQuery(search)).toEqual({
    range: 'last_hour',
    scope: 'all',
    filters: [],
  })
})

test('the filters of the url become the filters of the query', () => {
  const search = trafficSearchSchema.parse({
    filters: [{ d: 'target', v: 'example.com' }],
  })

  expect(toQuery(search).filters).toEqual([
    { dimension: 'target', value: 'example.com' },
  ])
})

test('the short filters of the url become the backend filters', () => {
  expect(toTrafficFilters([{ d: 'rule', v: 'Match' }])).toEqual([
    { dimension: 'rule', value: 'Match' },
  ])
})

test('the old proxy parameter and unknown dimensions are not accepted', () => {
  expect(trafficSearchSchema.parse({ proxy: 'Node-A' })).not.toHaveProperty(
    'proxy',
  )
  expect(
    trafficSearchSchema.safeParse({ filters: [{ d: 'nope', v: 'x' }] }).success,
  ).toBe(false)
})

test('a filter on a dimension that has one replaces it in place', () => {
  const filters = [
    { d: 'target' as const, v: 'a' },
    { d: 'inbound' as const, v: 'mixed' },
  ]

  expect(setFilter(filters, 'target', 'b')).toEqual([
    { d: 'target', v: 'b' },
    { d: 'inbound', v: 'mixed' },
  ])
  expect(setFilter(filters, 'exit', 'Node-A')).toEqual([
    ...filters,
    { d: 'exit', v: 'Node-A' },
  ])
})

test('toggling the filter that is set removes it', () => {
  const filters = [{ d: 'target' as const, v: 'a' }]

  expect(toggleFilter(filters, 'target', 'a')).toEqual([])
  expect(toggleFilter(filters, 'target', 'b')).toEqual([
    { d: 'target', v: 'b' },
  ])
  // The empty key of "no policy group" is a value like any other.
  expect(toggleFilter([], 'chain', '')).toEqual([{ d: 'chain', v: '' }])
})

test('ranges up to 24 hours poll every 2 s, longer ones every 10 s', () => {
  expect(
    (['last_hour', 'last6_hours', 'last24_hours'] as const).map(pollInterval),
  ).toEqual([2_000, 2_000, 2_000])
  expect(
    (['last7_days', 'last30_days', 'all'] as const).map(pollInterval),
  ).toEqual([10_000, 10_000, 10_000])
})

test('a range is beyond the retention when it reaches back further', () => {
  expect(beyondRetention('last24_hours', '1d')).toBe(false)
  expect(beyondRetention('last7_days', '1d')).toBe(true)
  expect(beyondRetention('last7_days', '7d')).toBe(false)
  expect(beyondRetention('last30_days', '7d')).toBe(true)
  expect(beyondRetention('last30_days', '30d')).toBe(false)
  expect(beyondRetention('last30_days', 'forever')).toBe(false)
  // Everything kept, whatever that is, and nothing is known before it loads.
  expect(beyondRetention('all', '1d')).toBe(false)
  expect(beyondRetention('last30_days', undefined)).toBe(false)
})

test('the flow asks for the template layers, the map for every region', () => {
  expect(toTopologyRequest(trafficSearchSchema.parse({}))).toEqual({
    layers: ['origin', 'rule', 'chain', 'exit'],
    limit_per_layer: 7,
  })
  expect(
    toTopologyRequest(trafficSearchSchema.parse({ layers: 'source' })).layers,
  ).toEqual(['source', 'target', 'exit'])
  expect(toTopologyRequest(trafficSearchSchema.parse({ view: 'map' }))).toEqual(
    {
      layers: ['source_region', 'destination_region', 'destination_basis'],
      limit_per_layer: null,
    },
  )
  expect(trafficSearchSchema.safeParse({ layers: 'nope' }).success).toBe(false)
})

test('the limit is a count or all, and all asks for no limit', () => {
  const request = (limit: unknown) =>
    toTopologyRequest(trafficSearchSchema.parse({ limit }))

  expect(request(undefined).limit_per_layer).toBe(7)
  expect(request(5).limit_per_layer).toBe(5)
  expect(request(20).limit_per_layer).toBe(20)
  expect(request('all').limit_per_layer).toBeNull()
  // The map places every region whatever the limit.
  expect(
    toTopologyRequest(trafficSearchSchema.parse({ view: 'map', limit: 5 }))
      .limit_per_layer,
  ).toBeNull()
  expect(trafficSearchSchema.safeParse({ limit: 6 }).success).toBe(false)
  expect(trafficSearchSchema.safeParse({ limit: '7' }).success).toBe(false)
})
