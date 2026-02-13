import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import CoreSecretConfig from './_modules/core-secret-config'
import ExternalControllerConfig from './_modules/external-controller-config'
import PortStrategySelector from './_modules/port-strategy-selector'

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_label_external_controll()}</SettingsTitle>

      <ExternalControllerConfig />

      <PortStrategySelector />

      <CoreSecretConfig />
    </>
  )
}
