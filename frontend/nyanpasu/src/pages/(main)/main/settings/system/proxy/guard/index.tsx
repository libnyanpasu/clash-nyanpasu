import { AnimatePresence } from 'motion/react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { ItemGroup } from '@nyanpasu/ui/item-group'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
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
            <ItemGroup>
              <ProxyGuardSwitch />

              <AnimatePresence initial={false}>
                {value && (
                  <AnimatedItem
                    key="proxy-guard-interval"
                    className="rounded-extra-large *:rounded-[inherit]!"
                  >
                    <ProxyGuardConfig />
                  </AnimatedItem>
                )}

                {value && (
                  <AnimatedItem
                    key="proxy-guard-bypass"
                    className="rounded-extra-large *:rounded-[inherit]!"
                  >
                    <ProxyBypassConfig />
                  </AnimatedItem>
                )}
              </AnimatePresence>
            </ItemGroup>
          </ReadOnlyScope>
        </div>
      </div>
    </>
  )
}
