import { EmptyState } from '@nyanpasu/ui/empty-state'
import { ItemGroup } from '@nyanpasu/ui/item-group'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { PlatformDesktop } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'
import NetworkAutomationStatus from './_modules/network-automation-status'
import NetworkDefaultActions from './_modules/network-default-actions'
import NetworkRuleList from './_modules/network-rule-list'

export const Route = createFileRoute(
  '/(main)/main/settings/system/proxy/network-rules/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_system_proxy_network_automation_label() },
  },
})

function NetworkRules() {
  return (
    <ItemGroup>
      <SettingsCard>
        <SettingsCardContent>
          <NetworkAutomationStatus />
        </SettingsCardContent>
      </SettingsCard>

      <NetworkRuleList />

      <SettingsCard>
        <SettingsCardContent>
          <NetworkDefaultActions />
        </SettingsCardContent>
      </SettingsCard>
    </ItemGroup>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <div
        className="flex flex-1 flex-col px-4 pb-4"
        data-slot="network-automation-container"
      >
        <PlatformDesktop
          fallback={
            <EmptyState
              className="flex-1"
              tone="normal"
              data-slot="network-automation-unsupported"
              message={m.settings_system_proxy_network_automation_unsupported()}
            />
          }
        >
          <NetworkRules />
        </PlatformDesktop>
      </div>
    </>
  )
}
