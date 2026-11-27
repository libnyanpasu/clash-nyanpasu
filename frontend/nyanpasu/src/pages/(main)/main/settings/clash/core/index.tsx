import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import ManagedGuardFieldSelector from '../_modules/managed-guard-field-selector'
import BreakWhenModeChangeSwitch from './_modules/break-when-mode-change-switch'
import BreakWhenProfileChangeSwitch from './_modules/break-when-profile-change-switch'
import BreakWhenProxyChangeSelector from './_modules/break-when-proxy-change-selector'
import LatencyTestConfig from './_modules/latency-test-config'

export const Route = createFileRoute('/(main)/main/settings/clash/core/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_core_settings() } },
})

const CoreBehaviorSettings = () => {
  return (
    <ItemSection data-slot="core-behavior-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_core_behavior_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <ManagedGuardFieldSelector field="unified-delay" />

        <ManagedGuardFieldSelector field="tcp-concurrent" />
      </ItemGroup>
    </ItemSection>
  )
}

const LatencyTestSettings = () => {
  return (
    <ItemSection data-slot="latency-test-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_latency_test_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <LatencyTestConfig />
      </ItemGroup>
    </ItemSection>
  )
}

const ConnectionSettings = () => {
  return (
    <ItemSection data-slot="connection-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_core_connections_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <BreakWhenProxyChangeSelector />

        <BreakWhenProfileChangeSwitch />

        <BreakWhenModeChangeSwitch />
      </ItemGroup>
    </ItemSection>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <ReadOnlyScope permissions={['settings:write']}>
        <div className="space-y-4 px-4 pb-4">
          <CoreBehaviorSettings />

          <RequireCapability permission="settings:read">
            <LatencyTestSettings />
          </RequireCapability>

          <ConnectionSettings />
        </div>
      </ReadOnlyScope>
    </>
  )
}
