import { ItemGroup } from '@nyanpasu/ui/item-group'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import ProxyBypassConfig from './_modules/proxy-bypass-config'
import ProxyGuardConfig from './_modules/proxy-guard-config'

export const Route = createFileRoute(
  '/(main)/main/settings/system/proxy/guard/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_system_proxy_proxy_guard_label() },
  },
})

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="proxy-guard-container">
          <ReadOnlyScope permissions={['system:write']} banner={false}>
            <ItemGroup>
              <ProxyGuardConfig />

              <ProxyBypassConfig />
            </ItemGroup>
          </ReadOnlyScope>
        </div>
      </div>
    </>
  )
}
