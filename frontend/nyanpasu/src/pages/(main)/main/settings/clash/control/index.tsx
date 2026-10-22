import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../../../_modules/settings-card'
import ControlChannelSettings from './_modules/control-channel-settings'
import CoreSecretConfig from './_modules/core-secret-config'
import ExternalControllerConfig from './_modules/external-controller-config'
import PortStrategySelector from './_modules/port-strategy-selector'
import WebUI from './_modules/web-ui'

export const Route = createFileRoute('/(main)/main/settings/clash/control/')({
  component: RouteComponent,
})

const ExternalControllerSettings = () => {
  return (
    <div data-slot="external-controller-settings-container">
      <SettingsLabel>
        {m.settings_clash_external_controller_label()}
      </SettingsLabel>

      <SettingsGroup>
        <ExternalControllerConfig />

        <PortStrategySelector />

        <CoreSecretConfig />
      </SettingsGroup>
    </div>
  )
}

const WebUISettings = () => {
  return (
    <div data-slot="web-ui-settings-container">
      <SettingsLabel>{m.settings_web_ui_title()}</SettingsLabel>

      <WebUI />
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_control()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <ControlChannelSettings />

        <ExternalControllerSettings />

        <WebUISettings />
      </div>
    </>
  )
}
