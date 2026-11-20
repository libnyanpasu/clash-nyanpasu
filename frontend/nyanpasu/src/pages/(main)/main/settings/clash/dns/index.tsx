import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useClashSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card'
import DnsControlSwitches from './_modules/dns-control-switches'
import { DnsDraftProvider, useSavedDns } from './_modules/dns-draft'
import DnsGeneralSection from './_modules/dns-general-section'
import DnsSaveBar from './_modules/dns-save-bar'
import DnsTools from './_modules/dns-tools'
import DnsUpstreamSection from './_modules/dns-upstream-section'
import HostsSection from './_modules/hosts-section'

export const Route = createFileRoute('/(main)/main/settings/clash/dns/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_dns() } },
})

function DnsSettingsForm() {
  const { saved, features } = useSavedDns()

  const dnsControl = useClashSetting('dns_control')

  if (!saved || !features) {
    return null
  }

  // A core without a `dns` table Nyanpasu writes has nothing to set here.
  if (features.keys.kind === 'ignored') {
    return (
      <SettingsCard data-slot="dns-unsupported-card">
        <SettingsCardContent>
          {m.settings_clash_dns_unsupported()}
        </SettingsCardContent>
      </SettingsCard>
    )
  }

  return (
    <DnsDraftProvider saved={saved} features={features}>
      {dnsControl.value !== 'nyanpasu' && (
        <SettingsCard data-slot="dns-profile-control-card">
          <SettingsCardContent className="text-on-surface-variant text-sm">
            {m.settings_clash_dns_profile_control_notice()}
          </SettingsCardContent>
        </SettingsCard>
      )}

      <DnsGeneralSection />

      <DnsUpstreamSection />

      <HostsSection />

      <DnsSaveBar />
    </DnsDraftProvider>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <RequireCapability
        permission="settings:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['system:write']}>
          <div className="space-y-4 px-4 pb-4" data-slot="dns-settings-page">
            <DnsControlSwitches />

            <DnsSettingsForm />
          </div>
        </ReadOnlyScope>
      </RequireCapability>

      {/* The tools act on the running core, under `proxy`, not the settings. */}
      <RequireCapability command="clash_api_query_dns">
        <div className="px-4 pb-4">
          <DnsTools />
        </div>
      </RequireCapability>
    </>
  )
}
