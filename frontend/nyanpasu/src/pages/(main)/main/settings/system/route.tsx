import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { Separator } from '@/components/ui/separator'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../_modules/settings-card'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import AutoLaunchSwitch from './_modules/auto-launch-switch'
import CurrentSystemProxy from './_modules/current-system-proxy'
import ProxyBypassConfig from './_modules/proxy-bypass-config'
import ProxyGuardConfig from './_modules/proxy-guard-config'
import ProxyGuardSwitch from './_modules/proxy-guard-switch'
import SilentLaunchSwitch from './_modules/slient-launch-switch'
import SystemServiceCtrl from './_modules/system-service-ctrl'
import SystemServiceSwitch from './_modules/system-service-switch'

export const Route = createFileRoute('/(main)/main/settings/system')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_label_system()}</SettingsTitle>

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

      <Separator className="my-4" />

      <SystemServiceSwitch />

      <SystemServiceCtrl />

      <Separator className="my-4" />

      <AutoLaunchSwitch />

      <SilentLaunchSwitch />
    </>
  )
}
