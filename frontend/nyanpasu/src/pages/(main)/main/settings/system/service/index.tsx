import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { isMacOS, isWindows } from '@nyanpasu/platform'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card'
import AutoLaunchSwitch from './_modules/auto-launch-switch'
import DnsCacheButton from './_modules/dns-cache-button'
import SilentLaunchSwitch from './_modules/slient-launch-switch'
import SystemServiceCtrl from './_modules/system-service-ctrl'
import SystemServiceSwitch from './_modules/system-service-switch'
import UwpToolsButton from './_modules/uwp-tools-button'

export const Route = createFileRoute('/(main)/main/settings/system/service/')({
  component: RouteComponent,
})

const SystemService = () => {
  return (
    <div data-slot="system-service-container">
      <SettingsLabel>
        {m.settings_system_proxy_system_service_ctrl_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <SystemServiceSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <SystemServiceCtrl />
      </SettingsGroup>
    </div>
  )
}

const SystemLaunch = () => {
  return (
    <div data-slot="system-launch-container">
      <SettingsLabel>{m.settings_system_proxy_launch_label()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AutoLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <SilentLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

const SystemTools = () => {
  return (
    <div data-slot="system-tools-container">
      <SettingsLabel>
        {m.settings_system_proxy_system_tools_label()}
      </SettingsLabel>

      <SettingsGroup>
        <DnsCacheButton />

        {isWindows && <UwpToolsButton />}
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_service()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <SystemService />

        <SystemLaunch />

        {(isWindows || isMacOS) && <SystemTools />}
      </div>
    </>
  )
}
