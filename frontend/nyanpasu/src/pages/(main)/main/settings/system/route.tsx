import { AnimatePresence } from 'framer-motion'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { isWindows } from '@/consts'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
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
import UwpToolsButton from './_modules/uwp-tools-button'

export const Route = createFileRoute('/(main)/main/settings/system')({
  component: RouteComponent,
})

const ProxyMode = () => {
  return (
    <div data-slot="proxy-mode-container">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_mode_label()}
      </SettingsLabel>

      <SettingsGroup>
        <div className="grid grid-cols-2 gap-2">
          <SystemProxyButton />

          <TunModeButton />
        </div>
      </SettingsGroup>
    </div>
  )
}

const ProxyGuard = () => {
  const { value } = useSetting('enable_proxy_guard')

  return (
    <div data-slot="proxy-guard-container">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_guard_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ProxyGuardSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <AnimatePresence initial={false}>
          {value && (
            <SettingsCard asChild>
              <SettingsCardAnimatedItem>
                <SettingsCardContent>
                  <ProxyGuardConfig />

                  <ProxyBypassConfig />
                </SettingsCardContent>
              </SettingsCardAnimatedItem>
            </SettingsCard>
          )}
        </AnimatePresence>
      </SettingsGroup>
    </div>
  )
}

const CurrentProxy = () => {
  return (
    <div data-slot="current-system-proxy-container">
      <SettingsLabel>
        {m.settings_system_proxy_current_system_proxy_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent className="py-4">
            <CurrentSystemProxy />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

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

const WindowsTools = () => {
  return (
    <div data-slot="windows-tools-container">
      <SettingsLabel>
        {m.settings_system_proxy_windows_tools_label()}
      </SettingsLabel>

      <SettingsGroup>
        <UwpToolsButton />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_system()}</SettingsTitle>

      <div className="space-y-4 px-4">
        <ProxyMode />

        <ProxyGuard />

        <CurrentProxy />

        <SystemService />

        <SystemLaunch />

        {isWindows && <WindowsTools />}
      </div>
    </>
  )
}
