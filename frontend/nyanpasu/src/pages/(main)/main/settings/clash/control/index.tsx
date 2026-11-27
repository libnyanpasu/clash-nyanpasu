import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import ControlChannelSettings from './_modules/control-channel-settings'
import CoreSecretConfig from './_modules/core-secret-config'
import ExternalControllerConfig from './_modules/external-controller-config'
import PortStrategySelector from './_modules/port-strategy-selector'
import WebUI from './_modules/web-ui'

export const Route = createFileRoute('/(main)/main/settings/clash/control/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_control() } },
})

const ExternalControllerSettings = () => {
  return (
    <ItemSection data-slot="external-controller-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_external_controller_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <ExternalControllerConfig />

        <PortStrategySelector />

        <CoreSecretConfig />
      </ItemGroup>
    </ItemSection>
  )
}

const WebUISettings = () => {
  return (
    <ItemSection data-slot="web-ui-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>{m.settings_web_ui_title()}</ItemSectionTitle>
      </ItemSectionHeader>

      <WebUI />
    </ItemSection>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <RequireCapability
        permission="settings:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['system:write']}>
          <div className="space-y-4 px-4 pb-4">
            <ControlChannelSettings />

            <ExternalControllerSettings />

            <WebUISettings />
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}
