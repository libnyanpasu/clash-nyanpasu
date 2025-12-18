import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../_modules/settings-card'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import CurrentSystemProxy from './_modules/current-system-proxy'
import ProxyBypassConfig from './_modules/proxy-bypass-config'
import ProxyGuardConfig from './_modules/proxy-guard-config'
import ProxyGuardSwitch from './_modules/proxy-guard-switch'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/system-proxy',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_system_proxy_title()}</SettingsTitle>

      <SettingsCard>
        <SettingsCardContent>
          <div className="grid grid-cols-2 gap-2">
            <SystemProxyButton />

            <TunModeButton />
          </div>
        </SettingsCardContent>
      </SettingsCard>

      <ProxyGuardSwitch />

      <ProxyGuardConfig />

      <ProxyBypassConfig />

      <CurrentSystemProxy />
    </>
  )
}
