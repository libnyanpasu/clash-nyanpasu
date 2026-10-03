import {
  createContext,
  useContext,
  useMemo,
  type PropsWithChildren,
} from 'react'
import { useProfile, useSettings, useTrafficReports } from '@nyanpasu/query'
import type { TrafficReport, TrafficRetention } from '@nyanpasu/rpc/types'
import type { UseQueryResult } from '@tanstack/react-query'
import type { DashboardItem } from './consts'
import { useDashboardContext } from './provider'
import { getWidgetConfig, WidgetId, type WidgetConfigs } from './widget-config'
import {
  createTrafficReportRequest,
  trafficReportRequestKey,
  uniqueTrafficReportRequests,
} from './widget-traffic-report-model'

type TrafficReportWidgetId =
  | WidgetId.RecentTraffic
  | WidgetId.OriginTraffic
  | WidgetId.ExitTraffic
  | WidgetId.TargetTraffic
  | WidgetId.RuleTraffic

type TrafficReportResult = UseQueryResult<TrafficReport, Error>
type DashboardTrafficContextValue = {
  reports: ReadonlyMap<string, TrafficReportResult>
  retention: TrafficRetention | undefined
  profileNames: ReadonlyMap<string, string> | undefined
  profilesResolved: boolean
}

const DashboardTrafficContext = createContext<
  DashboardTrafficContextValue | undefined
>(undefined)

function isTrafficReportWidget(type: WidgetId): type is TrafficReportWidgetId {
  return (
    type === WidgetId.RecentTraffic ||
    type === WidgetId.OriginTraffic ||
    type === WidgetId.ExitTraffic ||
    type === WidgetId.TargetTraffic ||
    type === WidgetId.RuleTraffic
  )
}

function widgetRequest(
  item: DashboardItem,
  configs: ReturnType<typeof useDashboardContext>['configs'],
) {
  if (!isTrafficReportWidget(item.type)) return null

  const config = getWidgetConfig(configs, item.id, item.type) as
    | WidgetConfigs[WidgetId.RecentTraffic]
    | WidgetConfigs[WidgetId.OriginTraffic]
    | WidgetConfigs[WidgetId.ExitTraffic]
    | WidgetConfigs[WidgetId.TargetTraffic]
    | WidgetConfigs[WidgetId.RuleTraffic]

  return { range: config.range, profileUid: config.profileUid }
}

/** One query observer per distinct report request for the visible Dashboard. */
export function DashboardTrafficProvider({
  items,
  children,
}: PropsWithChildren<{ items: DashboardItem[] }>) {
  const { configs, configLoading, configReadError } = useDashboardContext()
  const requests = useMemo(() => {
    if (configLoading || configReadError) return []
    const widgetConfigs: {
      range: Parameters<typeof createTrafficReportRequest>[0]
      profileUid: string | null
    }[] = []
    for (const item of items) {
      const config = widgetRequest(item, configs)
      if (config) widgetConfigs.push(config)
    }
    return uniqueTrafficReportRequests(widgetConfigs)
  }, [items, configs, configLoading, configReadError])

  if (requests.length === 0) {
    return (
      <DashboardTrafficContext.Provider
        value={{
          reports: new Map(),
          retention: undefined,
          profileNames: undefined,
          profilesResolved: false,
        }}
      >
        {children}
      </DashboardTrafficContext.Provider>
    )
  }

  return (
    <DashboardTrafficDataProvider requests={requests}>
      {children}
    </DashboardTrafficDataProvider>
  )
}

function DashboardTrafficDataProvider({
  requests,
  children,
}: PropsWithChildren<{
  requests: ReturnType<typeof uniqueTrafficReportRequests>
}>) {
  const { query: settingsQuery } = useSettings()
  const { query: profilesQuery } = useProfile()
  const retention = settingsQuery.isError
    ? undefined
    : settingsQuery.data?.traffic_retention
  const profileNames = useMemo(() => {
    if (!profilesQuery.data) return undefined
    return new Map(
      profilesQuery.data.items.map((profile) => [profile.uid, profile.name]),
    )
  }, [profilesQuery.data])
  const results = useTrafficReports(requests, { refetchInterval: 10_000 })
  const byRequest = useMemo(() => {
    const map = new Map<string, TrafficReportResult>()
    requests.forEach((request, index) => {
      const result = results[index]
      if (result) map.set(trafficReportRequestKey(request), result)
    })
    return map
  }, [requests, results])

  return (
    <DashboardTrafficContext.Provider
      value={{
        reports: byRequest,
        retention,
        profileNames,
        profilesResolved: profilesQuery.isSuccess && !profilesQuery.isError,
      }}
    >
      {children}
    </DashboardTrafficContext.Provider>
  )
}

export function useDashboardTrafficReport(
  range: Parameters<typeof createTrafficReportRequest>[0],
  profileUid: string | null,
) {
  const reports = useContext(DashboardTrafficContext)
  const request = createTrafficReportRequest(range, profileUid)
  return reports?.reports.get(trafficReportRequestKey(request))
}

export function useDashboardTrafficRetention() {
  return useContext(DashboardTrafficContext)?.retention
}

export function useDashboardTrafficProfile(profileUid: string | null) {
  const context = useContext(DashboardTrafficContext)
  if (profileUid === null) return null
  const name = context?.profileNames?.get(profileUid)
  return {
    uid: profileUid,
    name,
    missing:
      context?.profilesResolved === true &&
      !context.profileNames?.has(profileUid),
  }
}
