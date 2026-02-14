import { Separator } from '@/components/ui/separator'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import CoreSecretConfig from './_modules/core-secret-config'
import ExternalControllerConfig from './_modules/external-controller-config'
import PortStrategySelector from './_modules/port-strategy-selector'
import WebUI from './_modules/web-ui'

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

      <Separator className="my-4" />

      <WebUI />
    </>
  )
}
