import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import CoreSecretConfig from './_modules/core-secret-config'
import ExternalControllerConfig from './_modules/external-controller-config'
import PortStrategySelector from './_modules/port-strategy-selector'
import WebUI from './_modules/web-ui'

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
})

const ExternalController = () => {
  return (
    <div data-slot="theme-mode-settings-container">
      <SettingsLabel>{m.settings_label_external_controll()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ExternalControllerConfig />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <PortStrategySelector />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <CoreSecretConfig />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

const WebUISettings = () => {
  return (
    <div data-slot="theme-mode-settings-container">
      <SettingsLabel>{m.settings_web_ui_title()}</SettingsLabel>

      <WebUI />
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_external_controll()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <ExternalController />

        <WebUISettings />
      </div>
    </>
  )
}
