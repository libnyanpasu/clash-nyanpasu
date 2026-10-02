import type {
  Dimension,
  ReportRequest,
  TrafficRange,
  TrafficReport,
  Usage,
  UsageGroup,
} from '@nyanpasu/rpc/types'

export const TRAFFIC_REPORT_DIMENSIONS = [
  'origin',
  'exit',
  'target',
  'rule',
] as const satisfies readonly Dimension[]

export const TRAFFIC_REPORT_LIMIT = 5

export function createTrafficReportRequest(
  range: TrafficRange,
  profileUid: string | null,
): ReportRequest {
  return {
    metric: 'bytes',
    query: {
      range,
      scope: 'all',
      filters:
        profileUid === null
          ? []
          : [{ dimension: 'profile', value: profileUid }],
    },
    rankings: [...TRAFFIC_REPORT_DIMENSIONS],
    ranking_limit: TRAFFIC_REPORT_LIMIT,
    topology: null,
  }
}

/** The report request fields have a stable construction order. */
export const trafficReportRequestKey = (request: ReportRequest) =>
  JSON.stringify(request)

export function uniqueTrafficReportRequests(
  configs: readonly { range: TrafficRange; profileUid: string | null }[],
) {
  const unique = new Map<string, ReportRequest>()
  for (const config of configs) {
    const request = createTrafficReportRequest(config.range, config.profileUid)
    unique.set(trafficReportRequestKey(request), request)
  }
  return [...unique.values()]
}

export const usageTotalBytes = ({ bytes }: Usage) =>
  bytes.upload + bytes.download

export function addTrafficUsage(left: Usage, right: Usage): Usage {
  return {
    bytes: {
      upload: left.bytes.upload + right.bytes.upload,
      download: left.bytes.download + right.bytes.download,
    },
    connections: left.connections + right.connections,
  }
}

export function zeroTrafficUsage(): Usage {
  return { bytes: { upload: 0, download: 0 }, connections: 0 }
}

export type TrafficRankingSlice = {
  dimension: Dimension
  groups: UsageGroup[]
  other: Usage
  otherCount: number
  distinct: number
  totalBytes: number
}

/** Keep backend byte order and fold any client-hidden rows into the exact tail. */
export function sliceTrafficRanking(
  report: TrafficReport,
  dimension: Dimension,
  limit: 3 | 5,
): TrafficRankingSlice | null {
  const ranking = report.rankings.find((item) => item.dimension === dimension)
  if (!ranking) return null

  const groups = ranking.groups.slice(0, limit)
  const hiddenGroups = ranking.groups.slice(groups.length)
  const other = hiddenGroups.reduce(
    (total, group) => addTrafficUsage(total, group.usage),
    ranking.other,
  )

  return {
    dimension,
    groups,
    other,
    otherCount: Math.max(0, ranking.distinct - groups.length),
    distinct: ranking.distinct,
    totalBytes: usageTotalBytes(report.total),
  }
}

/** Null means no percentage can be meaningfully computed. */
export function trafficSharePercent(usage: Usage, totalBytes: number) {
  if (totalBytes <= 0) return null
  return (usageTotalBytes(usage) / totalBytes) * 100
}
