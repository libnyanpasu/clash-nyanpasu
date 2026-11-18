import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { matchPlatform } from '@nyanpasu/platform'
import { usePlatform } from '@nyanpasu/platform/react'
import { useNetworkAutomationStatus } from '@nyanpasu/query'
import type { NetworkAutomationStatus, WifiName } from '@nyanpasu/rpc/types'
import { openExternal } from '../../../_modules/open-external'

/** The system pane where the user lets this app use location. */
const LOCATION_SETTINGS_URL = {
  macos:
    'x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices',
  windows: 'ms-settings:privacy-location',
} as const

/**
 * Says why a Wi-Fi name could not be read and what can be done about it.
 * Renders nothing for a name that is known, unneeded or not Wi-Fi.
 */
export default function WifiNameNotice({
  wifi,
  onRequested,
}: {
  wifi: WifiName
  /** Receives the status the request answered, with the reading it took. */
  onRequested?: (status: NetworkAutomationStatus) => void
}) {
  const platform = usePlatform()
  const { requestWifiName } = useNetworkAutomationStatus()

  const settingsUrl = matchPlatform(platform, {
    macos: () => LOCATION_SETTINGS_URL.macos,
    windows: () => LOCATION_SETTINGS_URL.windows,
    default: () => undefined,
  })

  const handleAllow = useLockFn(async () => {
    try {
      const status = await requestWifiName.mutateAsync()

      if (status) {
        onRequested?.(status)
      }
    } catch (error) {
      message(formatError(error), { title: 'Error', kind: 'error', error })
    }
  })

  if (wifi.kind !== 'unavailable') {
    return null
  }

  return (
    <div
      className="flex items-center justify-between gap-4"
      data-slot="wifi-name-notice"
    >
      <span className="text-on-surface-variant text-sm">
        {describeReason(wifi.reason)}
      </span>

      {wifi.reason === 'permission_required' && (
        <Button
          type="button"
          onClick={handleAllow}
          loading={requestWifiName.isPending}
        >
          {m.settings_system_proxy_network_automation_wifi_allow()}
        </Button>
      )}

      {wifi.reason === 'permission_denied' && settingsUrl && (
        <Button type="button" onClick={() => openExternal(settingsUrl)}>
          {m.settings_system_proxy_network_automation_wifi_open_settings()}
        </Button>
      )}
    </div>
  )
}

const describeReason = (
  reason: Extract<WifiName, { kind: 'unavailable' }>['reason'],
) => {
  switch (reason) {
    case 'permission_required':
      return m.settings_system_proxy_network_automation_wifi_permission_required()
    case 'permission_denied':
      return m.settings_system_proxy_network_automation_wifi_permission_denied()
    case 'unreadable':
      return m.settings_system_proxy_network_automation_wifi_unreadable()
    case 'unsupported':
      return m.settings_system_proxy_network_automation_wifi_unsupported()
  }
}
