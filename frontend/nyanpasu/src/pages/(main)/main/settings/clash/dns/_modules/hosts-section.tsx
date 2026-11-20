import { m } from '@/paraglide/messages'
import { useClashSetting } from '@nyanpasu/query'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { useDnsDraft } from './dns-draft'
import { MappingField } from './list-fields'

export default function HostsSection() {
  const { draft, setHosts, features } = useDnsDraft()

  const hostsControl = useClashSetting('hosts_control')

  return (
    <div data-slot="dns-hosts-settings">
      <SettingsLabel>{m.settings_clash_dns_hosts_label()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard data-slot="dns-hosts-card">
          <SettingsCardContent>
            <MappingField
              label={m.settings_clash_dns_hosts_label()}
              description={
                hostsControl.value === 'nyanpasu'
                  ? features.list_values
                    ? m.settings_clash_dns_hosts_description()
                    : m.settings_clash_dns_single_value_description()
                  : m.settings_clash_dns_hosts_profile_description()
              }
              entries={draft.hosts}
              keyPlaceholder="router.lan"
              valuePlaceholder="192.168.1.1"
              onChange={setHosts}
            />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
