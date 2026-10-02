import { z } from 'zod'
import type {
  Dimension,
  TopologyRequest,
  TrafficQuery,
  TrafficRange,
  TrafficRetention,
} from '@nyanpasu/rpc/types'

export const RANGES = [
  'last_hour',
  'last6_hours',
  'last24_hours',
  'last7_days',
  'last30_days',
  'all',
] as const satisfies readonly TrafficRange[]

const DIMENSIONS = [
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

export const TEMPLATES = ['origin', 'source', 'inbound', 'process'] as const

export type LayerTemplate = (typeof TEMPLATES)[number]

export const DEFAULT_TEMPLATE: LayerTemplate = 'origin'

/** How many nodes a flow column shows before the rest merge into "other". */
export const LIMITS = [5, 7, 10, 20, 'all'] as const

export type Limit = (typeof LIMITS)[number]

export const DEFAULT_LIMIT: Limit = 7

/** The column layouts of the flow view, from the first column to the last. */
export const LAYER_TEMPLATES: Record<LayerTemplate, readonly Dimension[]> = {
  origin: ['origin', 'rule', 'chain', 'exit'],
  source: ['source', 'target', 'exit'],
  inbound: ['inbound', 'rule', 'exit'],
  process: ['process', 'target', 'exit'],
}

// The page state lives in the URL, so a view can be reloaded and shared.
export const trafficSearchSchema = z.object({
  range: z.enum(RANGES).default('last_hour'),
  scope: z.enum(['all', 'active', 'closed']).default('all'),
  filters: z
    .array(z.object({ d: z.enum(DIMENSIONS), v: z.string() }))
    .default([]),
  view: z.enum(['flow', 'map']).default('flow'),
  // Name of a topology layer template; the first one when absent.
  layers: z.enum(TEMPLATES).optional(),
  metric: z.enum(['bytes', 'connections']).default('bytes'),
  limit: z.literal(LIMITS).default(DEFAULT_LIMIT),
})

export type TrafficSearch = z.infer<typeof trafficSearchSchema>

export type SearchFilter = TrafficSearch['filters'][number]

export const toQuery = ({
  range,
  scope,
  filters,
}: Pick<TrafficSearch, 'range' | 'scope' | 'filters'>): TrafficQuery => ({
  range,
  scope,
  filters: filters.map(({ d, v }) => ({ dimension: d, value: v })),
})

/** The columns of the flow view are the template's; the map's are regions. */
export const toTopologyRequest = ({
  view,
  layers,
  limit,
}: Pick<TrafficSearch, 'view' | 'layers' | 'limit'>): TopologyRequest =>
  view === 'map'
    ? {
        // The third layer splits each destination region by how it was located.
        layers: ['source_region', 'destination_region', 'destination_basis'],
        // The map places every region, so none merges into "other".
        limit_per_layer: null,
      }
    : {
        layers: [...LAYER_TEMPLATES[layers ?? DEFAULT_TEMPLATE]],
        limit_per_layer: limit === 'all' ? null : limit,
      }

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

// Short ranges move fast; longer ones read from hourly records.
export const pollInterval = (range: TrafficRange) =>
  range === 'last_hour' || range === 'last6_hours' || range === 'last24_hours'
    ? 2_000
    : 10_000

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
