import RouterRounded from '~icons/material-symbols/router-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
  ItemSplitSwitch,
} from '@nyanpasu/ui/item-group'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { PlatformDesktop, PlatformLinux } from '@nyanpasu/platform/react'
import {
  useNetworkAutomation,
  useSetting,
  useSystemSetting,
} from '@nyanpasu/query'
import { createFileRoute, Link } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
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
    <ItemSection data-slot="proxy-mode-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_proxy_mode_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ReadOnlyScope permissions={['system:write']}>
        <ItemGroup className="pb-4">
          <div className="grid grid-cols-2 gap-2">
            <SystemProxyButton />

            <TunModeButton />

            <PlatformLinux>
              <TransparentProxyButton />
            </PlatformLinux>
          </div>
        </ItemGroup>
      </ReadOnlyScope>

      <ProxyModeSelector />
    </ItemSection>
  )
}

const ProxyGuardEntry = () => {
  const proxyGuard = useSystemSetting('enable_proxy_guard')
  const { value: interval } = useSetting('proxy_guard_interval')

  const enabled = Boolean(proxyGuard.value)
  const description = enabled
    ? `${m.common_enabled()} · ${m.settings_system_proxy_proxy_guard_entry_interval({ interval: interval || 1 })}`
    : m.common_disabled()

  const handleToggle = useLockFn(async (next: boolean) => {
    try {
      await proxyGuard.upsert(next)
    } catch (error) {
      message(`Activation Proxy Guard failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemSection data-slot="proxy-guard-section">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_proxy_guard_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ReadOnlyScope permissions={['system:write']} banner={false}>
        <ItemGroup>
          <ItemSplitSwitch
            leading={<ShieldRounded />}
            title={m.settings_system_proxy_proxy_guard_switch_label()}
            description={description}
            checked={enabled}
            onCheckedChange={handleToggle}
            loading={proxyGuard.isPending}
            actionAsChild
            data-slot="proxy-guard-entry"
          >
            <Link to="/main/settings/system/proxy/guard" />
          </ItemSplitSwitch>
        </ItemGroup>
      </ReadOnlyScope>
    </ItemSection>
  )
}

const NetworkRulesEntry = () => {
  const { policy, save, isPending } = useNetworkAutomation()

  const enabled = Boolean(policy?.enabled)
  const ruleCount = policy?.rules?.length ?? 0

  const description = enabled
    ? `${m.common_enabled()} · ${m.settings_system_proxy_network_automation_entry_rules({ count: ruleCount })}`
    : m.common_disabled()

  const handleToggle = useLockFn(async (next: boolean) => {
    if (!policy) {
      return
    }

    try {
      await save({ ...policy, enabled: next })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemSection data-slot="network-automation-section">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_network_automation_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ReadOnlyScope permissions={['system:write']} banner={false}>
        <ItemGroup>
          <ItemSplitSwitch
            leading={<RouterRounded />}
            title={m.settings_system_proxy_network_automation_switch_label()}
            description={description}
            checked={enabled}
            onCheckedChange={handleToggle}
            loading={isPending}
            actionAsChild
            data-slot="network-automation-entry"
          >
            <Link to="/main/settings/system/proxy/network-rules" />
          </ItemSplitSwitch>
        </ItemGroup>
      </ReadOnlyScope>
    </ItemSection>
  )
}

const CurrentProxy = () => {
  return (
    <ItemSection data-slot="current-system-proxy-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_current_system_proxy_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <SettingsCard>
          <SettingsCardContent className="py-4">
            <CurrentSystemProxy />
            <PlatformLinux>
              <TransparentProxyDiagnostics />
            </PlatformLinux>
          </SettingsCardContent>
        </SettingsCard>
      </ItemGroup>
    </ItemSection>
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
