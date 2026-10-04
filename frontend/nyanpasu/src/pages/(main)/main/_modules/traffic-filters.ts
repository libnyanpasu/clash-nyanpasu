import { z } from 'zod'
import { m } from '@/paraglide/messages'
import type {
  Dimension,
  TrafficFilter,
  TrafficRange,
} from '@nyanpasu/rpc/types'

export const RANGES = [
  'last_hour',
  'last6_hours',
  'last24_hours',
  'last7_days',
  'last30_days',
  'all',
] as const satisfies readonly TrafficRange[]

export const DIMENSIONS = [
  'origin',
  'process',
  'source',
  'inbound',
  'target',
  'protocol',
  'rule',
  'chain',
  'exit',
  'profile',
  'source_region',
  'destination_region',
  'destination_basis',
] as const satisfies readonly Dimension[]

// A filter as the URL keeps it, in short keys.
export const searchFilterSchema = z.object({
  d: z.enum(DIMENSIONS),
  v: z.string(),
})

export type SearchFilter = z.infer<typeof searchFilterSchema>

/** Adds a filter; another value of the same dimension is replaced. */
export function setFilter(
  filters: SearchFilter[],
  dimension: Dimension,
  value: string,
): SearchFilter[] {
  const next = { d: dimension, v: value }

  return filters.some((filter) => filter.d === dimension)
    ? filters.map((filter) => (filter.d === dimension ? next : filter))
    : [...filters, next]
}

/** The filter the row stands for: set it, or remove it if it is already set. */
export function toggleFilter(
  filters: SearchFilter[],
  dimension: Dimension,
  value: string,
): SearchFilter[] {
  return filters.some((f) => f.d === dimension && f.v === value)
    ? filters.filter((f) => f.d !== dimension)
    : setFilter(filters, dimension, value)
}

/** The URL's short filters as the backend's filters. */
export const toTrafficFilters = (
  filters: readonly SearchFilter[],
): TrafficFilter[] => filters.map(({ d, v }) => ({ dimension: d, value: v }))

export const rangeName = (range: TrafficRange) =>
  ({
    last_hour: m.traffic_range_last_hour,
    last6_hours: m.traffic_range_last6_hours,
    last24_hours: m.traffic_range_last24_hours,
    last7_days: m.traffic_range_last7_days,
    last30_days: m.traffic_range_last30_days,
    all: m.traffic_range_all,
  })[range]()

/**
 * Identifies a rule the way a connection's `rule` / `rulePayload` and the
 * traffic store's rule key (`RuleKey::label`) do.
 */
export const ruleLabel = (type: string, payload: string) =>
  payload ? `${type},${payload}` : type
