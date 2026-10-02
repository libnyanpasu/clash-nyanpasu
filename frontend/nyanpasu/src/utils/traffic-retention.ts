import type { TrafficRange, TrafficRetention } from '@nyanpasu/rpc/types'

const RANGE_HOURS: Record<Exclude<TrafficRange, 'all'>, number> = {
  last_hour: 1,
  last6_hours: 6,
  last24_hours: 24,
  last7_days: 7 * 24,
  last30_days: 30 * 24,
}

const RETENTION_HOURS: Record<TrafficRetention, number> = {
  '1d': 24,
  '7d': 7 * 24,
  '30d': 30 * 24,
  '90d': 90 * 24,
  forever: Infinity,
}

/** Whether `range` reaches back further than records are kept. */
export const beyondRetention = (
  range: TrafficRange,
  retention: TrafficRetention | undefined,
) =>
  range !== 'all' &&
  retention !== undefined &&
  RANGE_HOURS[range] > RETENTION_HOURS[retention]
