import { AnimatePresence } from 'motion/react'
import { PageTitle } from '@/components/page-title'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card'
import CurrentSystemProxy from './_modules/current-system-proxy'
import ProxyBypassConfig from './_modules/proxy-bypass-config'
import ProxyGuardConfig from './_modules/proxy-guard-config'
import ProxyGuardSwitch from './_modules/proxy-guard-switch'
import ProxyModeSelector from './_modules/proxy-mode-selector'

export const Route = createFileRoute('/(main)/main/settings/system/proxy/')({
  component: RouteComponent,
})

const ProxyMode = () => {
  return (
    <div data-slot="proxy-mode-container">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_mode_label()}
      </SettingsLabel>

      <SettingsGroup className="pb-4">
        <div className="grid grid-cols-2 gap-2">
          <SystemProxyButton />

          <TunModeButton />
        </div>
      </SettingsGroup>

      <ProxyModeSelector />
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

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_proxy()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <ProxyMode />

        <ProxyGuard />

        <CurrentProxy />
      </div>
    </>
  )
}
