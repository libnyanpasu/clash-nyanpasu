import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup } from '../../../_modules/settings-card'
import IpLookupProviderList from './_modules/ip-lookup-provider-list'
import LocalIpProbeSwitch from './_modules/local-ip-probe-switch'
import StunServerList from './_modules/stun-server-list'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/public-ip/',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_public_ip()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4" data-slot="public-ip-settings">
        <SettingsGroup>
          <LocalIpProbeSwitch />
        </SettingsGroup>

        <StunServerList />

        <IpLookupProviderList />
      </div>
    </>
  )
}
