import { AnimatePresence } from 'motion/react'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsGroup,
} from '../../../../_modules/settings-card'
import ProxyBypassConfig from './_modules/proxy-bypass-config'
import ProxyGuardConfig from './_modules/proxy-guard-config'
import ProxyGuardSwitch from './_modules/proxy-guard-switch'

export const Route = createFileRoute(
  '/(main)/main/settings/system/proxy/guard/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_system_proxy_proxy_guard_label() },
  },
})

function RouteComponent() {
  const { value } = useSetting('enable_proxy_guard')

  return (
    <>
      <PageTitle />

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="proxy-guard-container">
          <ReadOnlyScope permissions={['system:write']} banner={false}>
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
          </ReadOnlyScope>
        </div>
      </div>
    </>
  )
}
