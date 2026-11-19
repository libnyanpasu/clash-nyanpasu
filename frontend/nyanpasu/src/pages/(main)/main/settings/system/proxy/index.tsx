import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { m } from '@/paraglide/messages'
import { PlatformDesktop, PlatformLinux } from '@nyanpasu/platform/react'
import { useNetworkAutomation, useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLinkRow } from '../../_modules/settings-link-row'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card'
import CurrentSystemProxy from './_modules/current-system-proxy'
import ProxyModeSelector from './_modules/proxy-mode-selector'
import TransparentProxyButton from './_modules/transparent-proxy-button'
import TransparentProxyDiagnostics from './_modules/transparent-proxy-diagnostics'

export const Route = createFileRoute('/(main)/main/settings/system/proxy/')({
  component: RouteComponent,
})

const ProxyMode = () => {
  return (
    <div data-slot="proxy-mode-container">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_mode_label()}
      </SettingsLabel>

      <ReadOnlyScope permissions={['system:write']}>
        <SettingsGroup className="pb-4">
          <div className="grid grid-cols-2 gap-2">
            <SystemProxyButton />

            <TunModeButton />

            <PlatformLinux>
              <TransparentProxyButton />
            </PlatformLinux>
          </div>
        </SettingsGroup>
      </ReadOnlyScope>

      <ProxyModeSelector />
    </div>
  )
}

const ProxyGuardEntry = () => {
  const { value: enabled } = useSetting('enable_proxy_guard')
  const { value: interval } = useSetting('proxy_guard_interval')

  const description = enabled
    ? `${m.common_enabled()} · ${m.settings_system_proxy_proxy_guard_entry_interval({ interval: interval || 1 })}`
    : m.common_disabled()

  return (
    <div data-slot="proxy-guard-section">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_guard_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsLinkRow
          to="/main/settings/system/proxy/guard"
          title={m.settings_system_proxy_proxy_guard_switch_label()}
          description={description}
          data-slot="proxy-guard-entry"
        />
      </SettingsGroup>
    </div>
  )
}

const NetworkRulesEntry = () => {
  const { policy } = useNetworkAutomation()

  const enabled = Boolean(policy?.enabled)
  const ruleCount = policy?.rules?.length ?? 0

  const description = enabled
    ? `${m.common_enabled()} · ${m.settings_system_proxy_network_automation_entry_rules({ count: ruleCount })}`
    : m.common_disabled()

  return (
    <div data-slot="network-automation-section">
      <SettingsLabel>
        {m.settings_system_proxy_network_automation_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsLinkRow
          to="/main/settings/system/proxy/network-rules"
          title={m.settings_system_proxy_network_automation_switch_label()}
          description={description}
          data-slot="network-automation-entry"
        />
      </SettingsGroup>
    </div>
  )
}

const CurrentProxy = () => {
  return (
    <div data-slot="current-system-proxy-container">
      <SettingsLabel>
        {m.settings_system_proxy_current_system_proxy_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent className="py-4">
            <CurrentSystemProxy />
            <PlatformLinux>
              <TransparentProxyDiagnostics />
            </PlatformLinux>
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <div className="space-y-4 px-4 pb-4">
        <ProxyMode />

        <ProxyGuardEntry />

        <PlatformDesktop>
          <NetworkRulesEntry />
        </PlatformDesktop>

        <CurrentProxy />
      </div>
    </>
  )
}
