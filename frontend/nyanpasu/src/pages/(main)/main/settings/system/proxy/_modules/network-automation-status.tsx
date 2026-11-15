import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import {
  useNetworkAutomation,
  useNetworkAutomationStatus,
} from '@nyanpasu/query'
import type {
  NetworkObservation,
  NetworkState,
  WifiName,
} from '@nyanpasu/rpc/types'
import WifiNameNotice from './wifi-name-notice'

const describeWifi = (wifi: WifiName) => {
  switch (wifi.kind) {
    case 'known':
      return m.settings_system_proxy_network_automation_network_wifi({
        name: wifi.ssid,
      })
    case 'unavailable':
      return m.settings_system_proxy_network_automation_network_wifi_unnamed()
    default:
      // The name was not read: no rule matches on it.
      return m.settings_system_proxy_network_automation_network_wifi_kind()
  }
}

const describeNetwork = (network: NetworkObservation) => {
  const parts: string[] = []

  switch (network.interface_kind) {
    case 'wifi':
      parts.push(describeWifi(network.wifi))
      break
    case 'ethernet':
      parts.push(m.settings_system_proxy_network_automation_network_ethernet())
      break
    case 'other':
      parts.push(m.settings_system_proxy_network_automation_network_other())
  }

  if (network.gateway_mac) {
    parts.push(
      m.settings_system_proxy_network_automation_network_gateway({
        mac: network.gateway_mac,
      }),
    )
  }

  return parts.join(' · ')
}

const describeState = (state: NetworkState | null) => {
  if (!state) {
    return m.settings_system_proxy_network_automation_network_reading()
  }

  return state.state === 'offline'
    ? m.settings_system_proxy_network_automation_network_offline()
    : describeNetwork(state.network)
}

/** The network the rules see, the rule that matched it, and a way to re-apply. */
export default function NetworkAutomationStatus() {
  const { policy } = useNetworkAutomation()
  const { query, applyNow } = useNetworkAutomationStatus()

  const status = query.data
  const matched = status?.applied?.matched

  const wifi =
    status?.network?.state === 'online' &&
    status.network.network.interface_kind === 'wifi'
      ? status.network.network.wifi
      : undefined

  const matchedRule =
    matched?.kind === 'rule'
      ? policy?.rules?.find((rule) => rule.id === matched.id)
      : undefined

  const handleApply = useLockFn(async () => {
    try {
      await applyNow.mutateAsync()
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  if (status && !status.supported) {
    return (
      <p
        className="text-on-surface-variant text-sm"
        data-slot="network-automation-unsupported"
      >
        {m.settings_system_proxy_network_automation_unsupported()}
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-3" data-slot="network-automation-status">
      <div className="flex items-center justify-between gap-4">
        <div className="flex flex-col gap-0.5">
          <span className="text-on-surface-variant text-sm">
            {m.settings_system_proxy_network_automation_current_network()}
          </span>

          <span className="font-medium">
            {describeState(status?.network ?? null)}
          </span>

          {matched && (
            <span className="text-on-surface-variant text-sm">
              {matchedRule
                ? m.settings_system_proxy_network_automation_matched_rule({
                    name: matchedRule.name,
                  })
                : m.settings_system_proxy_network_automation_matched_default()}
            </span>
          )}
        </div>

        <Button onClick={handleApply} loading={applyNow.isPending}>
          {m.settings_system_proxy_network_automation_apply_now()}
        </Button>
      </div>

      {wifi && <WifiNameNotice wifi={wifi} />}
    </div>
  )
}
