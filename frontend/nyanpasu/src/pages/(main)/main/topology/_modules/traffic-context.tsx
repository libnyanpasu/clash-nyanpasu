import AppsRounded from '~icons/material-symbols/apps-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DevicesRounded from '~icons/material-symbols/devices-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import LanOutlineRounded from '~icons/material-symbols/lan-outline-rounded'
import LoginRounded from '~icons/material-symbols/login-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import { createContext, useContext } from 'react'
import { m } from '@/paraglide/messages'
import { dimensionName, type UsageLabel } from '@/utils/traffic-usage'
import type {
  Dimension,
  TrafficQuery,
  TrafficReport,
  TrafficRetention,
} from '@nyanpasu/rpc/types'
import type { TrafficSearch } from './search'

// By how often a dimension answers "what uses the traffic" on a desktop.
export const RANKINGS = [
  {
    dimension: 'process',
    icon: AppsRounded,
    title: m.traffic_rank_process,
  },
  {
    dimension: 'target',
    icon: DnsRounded,
    title: m.traffic_rank_target,
  },
  {
    dimension: 'exit',
    icon: CallSplitRounded,
    title: m.traffic_rank_exit,
  },
  {
    dimension: 'source',
    icon: DevicesRounded,
    title: m.traffic_rank_source,
  },
  {
    dimension: 'inbound',
    icon: LoginRounded,
    title: m.traffic_rank_inbound,
  },
] as const

export const ANALYSIS_VIEWS = [
  {
    view: 'flow',
    to: '/main/topology/flow',
    icon: LanOutlineRounded,
    title: m.topology_flow,
  },
  {
    view: 'map',
    to: '/main/topology/map',
    icon: PublicRounded,
    title: m.topology_geo_map,
  },
] as const

export const TRAFFIC_PAGES = [
  {
    view: 'overview',
    to: '/main/topology',
    icon: DashboardRounded,
    title: m.traffic_overview,
  },
  ...ANALYSIS_VIEWS,
  // Each dimension's full listing has its own entry.
  ...RANKINGS.map(({ dimension, icon }) => ({
    view: dimension,
    to: '/main/topology/$dimension' as const,
    params: { dimension },
    icon,
    title: () => dimensionName(dimension),
  })),
] as const

export type RankingDimension = (typeof RANKINGS)[number]['dimension']

export type TrafficView = (typeof TRAFFIC_PAGES)[number]['view']

export const withoutTrafficView = (search: TrafficSearch) => ({
  ...search,
  view: undefined,
})

export const TrafficPageContext = createContext<{
  search: TrafficSearch
  report?: TrafficReport
  query: TrafficQuery
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  onSearchChange: (update: Partial<TrafficSearch>) => void
  empty: boolean
  retention?: TrafficRetention
  paused: boolean
  onPausedChange: (paused: boolean) => void
  onViewConnections?: () => void
} | null>(null)

export function useTrafficPage() {
  const context = useContext(TrafficPageContext)

  if (!context)
    throw new Error('Traffic pages must render inside the traffic layout')

  return context
}
