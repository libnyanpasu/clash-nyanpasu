import {
  useClashProxiesProvider,
  useClashRulesProvider,
  useProfile,
} from '@nyanpasu/query'
import type { DashboardItem } from './consts'
import { useDashboardContext } from './provider'
import { getWidgetConfig, WidgetId } from './widget-config'

function ProfileCalibration() {
  useProfile({ refetchInterval: 60_000 })
  return null
}

function ProviderCalibration({
  proxy,
  rule,
}: {
  proxy: boolean
  rule: boolean
}) {
  useClashProxiesProvider({ refetchInterval: 60_000, enabled: proxy })
  useClashRulesProvider({ refetchInterval: 60_000, enabled: rule })
  return null
}

/** One low-frequency observer for each data source used by visible cards. */
export function WidgetDataCalibration({ items }: { items: DashboardItem[] }) {
  const { configs, configLoading, configReadError } = useDashboardContext()
  if (configLoading || configReadError) return null
  const profiles = items.some((item) =>
    [
      WidgetId.SubscriptionQuota,
      WidgetId.SubscriptionSchedule,
      WidgetId.ProfileShortcuts,
    ].includes(item.type),
  )
  const providers = items
    .filter((item) => item.type === WidgetId.ProviderUpdates)
    .map((item) => getWidgetConfig(configs, item.id, WidgetId.ProviderUpdates))
  const proxy = providers.some((config) => config.kinds !== 'rule')
  const rule = providers.some((config) => config.kinds !== 'proxy')
  return (
    <>
      {profiles && <ProfileCalibration />}
      {providers.length > 0 && (
        <ProviderCalibration proxy={proxy} rule={rule} />
      )}
    </>
  )
}
