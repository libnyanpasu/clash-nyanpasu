import { expect, test } from 'vitest'
import {
  addTrafficUsage,
  createTrafficReportRequest,
  sliceTrafficRanking,
  trafficSharePercent,
  uniqueTrafficReportRequests,
  usageTotalBytes,
  zeroTrafficUsage,
} from '@/components/widgets/widget-traffic-report-model'
import type { TrafficReport, Usage, UsageGroup } from '@nyanpasu/rpc/types'

const usage = (
  upload: number,
  download: number,
  connections: number,
): Usage => ({ bytes: { upload, download }, connections })

const group = (key: string, upload: number, download: number): UsageGroup => ({
  key,
  usage: usage(upload, download, 1),
  current_rate: null,
})

const report = (): TrafficReport => ({
  total: usage(210, 210, 6),
  current_rate: null,
  topology: null,
  rankings: [
    {
      dimension: 'origin',
      distinct: 6,
      groups: [
        group('unknown', 50, 50),
        group('/apps/second', 40, 40),
        group('third', 30, 30),
        group('fourth', 20, 20),
        group('fifth', 10, 10),
      ],
      other: usage(60, 60, 1),
    },
  ],
})

test('requests all widget dimensions with a shared query and optional profile filter', () => {
  expect(createTrafficReportRequest('last_hour', null)).toEqual({
    metric: 'bytes',
    query: { range: 'last_hour', scope: 'all', filters: [] },
    rankings: ['origin', 'exit', 'target', 'rule'],
    ranking_limit: 20,
    topology: null,
  })
  expect(
    createTrafficReportRequest('last7_days', 'profile-uid').query.filters,
  ).toEqual([{ dimension: 'profile', value: 'profile-uid' }])
})

test('identical widget filters share one canonical request', () => {
  const requests = uniqueTrafficReportRequests([
    { range: 'last24_hours', profileUid: null },
    { range: 'last24_hours', profileUid: null },
    { range: 'last24_hours', profileUid: 'profile-uid' },
    { range: 'last7_days', profileUid: null },
  ])

  expect(requests).toHaveLength(3)
  expect(requests.map(({ query }) => [query.range, query.filters])).toEqual([
    ['last24_hours', []],
    ['last24_hours', [{ dimension: 'profile', value: 'profile-uid' }]],
    ['last7_days', []],
  ])
})

test('top three folds hidden server rows and server other into one exact tail', () => {
  const sliced = sliceTrafficRanking(report(), 'origin', 3)!

  expect(sliced.groups.map(({ key }) => key)).toEqual([
    'unknown',
    '/apps/second',
    'third',
  ])
  expect(sliced.other).toEqual(usage(90, 90, 3))
  expect(sliced.otherCount).toBe(3)
  expect(sliced.distinct).toBe(6)
  expect(sliced.totalBytes).toBe(420)
  expect(
    sliced.groups.reduce((sum, item) => sum + usageTotalBytes(item.usage), 0) +
      usageTotalBytes(sliced.other),
  ).toBe(sliced.totalBytes)
  expect(
    [...sliced.groups.map(({ usage }) => usage), sliced.other].reduce(
      addTrafficUsage,
      zeroTrafficUsage(),
    ),
  ).toEqual(report().total)
})

test('top five keeps the server order and uses only the server tail', () => {
  const sliced = sliceTrafficRanking(report(), 'origin', 5)!

  expect(sliced.groups.map(({ key }) => key)).toEqual([
    'unknown',
    '/apps/second',
    'third',
    'fourth',
    'fifth',
  ])
  expect(sliced.other).toEqual(usage(60, 60, 1))
  expect(sliced.otherCount).toBe(1)
  expect(
    [...sliced.groups.map(({ usage }) => usage), sliced.other].reduce(
      addTrafficUsage,
      zeroTrafficUsage(),
    ),
  ).toEqual(report().total)
})

test('rankings can return more than five rows for expanded widget limits', () => {
  const expandedReport = report()
  expandedReport.rankings[0]!.groups.push(
    group('sixth', 5, 5),
    group('seventh', 4, 4),
  )
  expandedReport.rankings[0]!.distinct = 8

  const sliced = sliceTrafficRanking(expandedReport, 'origin', 7)!

  expect(sliced.groups.map(({ key }) => key)).toEqual([
    'unknown',
    '/apps/second',
    'third',
    'fourth',
    'fifth',
    'sixth',
    'seventh',
  ])
  expect(sliced.otherCount).toBe(1)
})

test('missing dimensions return null and zero traffic has no percentage', () => {
  expect(sliceTrafficRanking(report(), 'target', 3)).toBeNull()
  expect(trafficSharePercent(usage(0, 0, 0), 0)).toBeNull()
  expect(trafficSharePercent(usage(1, 2, 4), 6)).toBe(50)
})
