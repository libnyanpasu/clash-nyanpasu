import { Input } from '@nyanpasu/ui/input'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { useDnsDraft, type DnsValues } from './dns-draft'
import { MappingField, StringListField } from './list-fields'

type ServerListKey =
  | 'default-nameserver'
  | 'nameserver'
  | 'proxy-server-nameserver'
  | 'direct-nameserver'
  | 'fallback'

/** One upstream server list, shown only when the core reads its key. */
function ServerList({
  field,
  label,
  description,
}: {
  field: ServerListKey
  label: string
  description: string
}) {
  const { draft, updateDns, reads } = useDnsDraft()

  if (!reads(field)) {
    return null
  }

  return (
    <SettingsCard data-slot="dns-server-list-card">
      <SettingsCardContent>
        <StringListField
          label={label}
          description={description}
          values={draft.dns[field]}
          placeholder="https://dns.alidns.com/dns-query"
          onChange={(values) =>
            updateDns({ [field]: values } as Partial<DnsValues>)
          }
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}

function FallbackFilterSettings() {
  const { draft, updateDns, features } = useDnsDraft()

  const filter = draft.dns['fallback-filter']

  const updateFilter = (patch: Partial<DnsValues['fallback-filter']>) =>
    updateDns({ 'fallback-filter': { ...filter, ...patch } })

  return (
    <div data-slot="dns-fallback-filter-settings">
      <SettingsLabel>
        {m.settings_clash_dns_fallback_filter_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_clash_dns_fallback_filter_geoip_label()}
                </ItemLabelText>
              </ItemLabel>

              <Switch
                aria-label={m.settings_clash_dns_fallback_filter_geoip_label()}
                checked={filter.geoip}
                onCheckedChange={(geoip) => updateFilter({ geoip })}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <Input
              label={m.settings_clash_dns_fallback_filter_geoip_code_label()}
              value={filter['geoip-code']}
              onChange={(event) =>
                updateFilter({ 'geoip-code': event.target.value.trim() })
              }
            />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <StringListField
              label={m.settings_clash_dns_fallback_filter_ipcidr_label()}
              values={filter.ipcidr}
              placeholder="240.0.0.0/4"
              onChange={(ipcidr) => updateFilter({ ipcidr })}
            />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <StringListField
              label={m.settings_clash_dns_fallback_filter_domain_label()}
              values={filter.domain}
              placeholder="+.google.com"
              onChange={(domain) => updateFilter({ domain })}
            />
          </SettingsCardContent>
        </SettingsCard>

        {features.fallback_filter_geosite && (
          <SettingsCard>
            <SettingsCardContent>
              <StringListField
                label={m.settings_clash_dns_fallback_filter_geosite_label()}
                values={filter.geosite}
                placeholder="gfw"
                onChange={(geosite) => updateFilter({ geosite })}
              />
            </SettingsCardContent>
          </SettingsCard>
        )}
      </SettingsGroup>
    </div>
  )
}

export default function DnsUpstreamSection() {
  const { draft, updateDns, features, reads } = useDnsDraft()

  return (
    <>
      <div data-slot="dns-upstream-settings">
        <SettingsLabel>{m.settings_clash_dns_upstream_label()}</SettingsLabel>

        <SettingsGroup>
          <ServerList
            field="default-nameserver"
            label={m.settings_clash_dns_default_nameserver_label()}
            description={m.settings_clash_dns_default_nameserver_description()}
          />

          <ServerList
            field="nameserver"
            label={m.settings_clash_dns_nameserver_label()}
            description={m.settings_clash_dns_nameserver_description()}
          />

          <ServerList
            field="proxy-server-nameserver"
            label={m.settings_clash_dns_proxy_server_nameserver_label()}
            description={m.settings_clash_dns_proxy_server_nameserver_description()}
          />

          <ServerList
            field="direct-nameserver"
            label={m.settings_clash_dns_direct_nameserver_label()}
            description={m.settings_clash_dns_direct_nameserver_description()}
          />

          {reads('direct-nameserver-follow-policy') && (
            <SettingsCard>
              <SettingsCardContent>
                <ItemContainer>
                  <ItemLabel>
                    <ItemLabelText>
                      {m.settings_clash_dns_direct_nameserver_follow_policy_label()}
                    </ItemLabelText>
                  </ItemLabel>

                  <Switch
                    aria-label={m.settings_clash_dns_direct_nameserver_follow_policy_label()}
                    checked={draft.dns['direct-nameserver-follow-policy']}
                    onCheckedChange={(checked) =>
                      updateDns({ 'direct-nameserver-follow-policy': checked })
                    }
                  />
                </ItemContainer>
              </SettingsCardContent>
            </SettingsCard>
          )}

          <ServerList
            field="fallback"
            label={m.settings_clash_dns_fallback_label()}
            description={m.settings_clash_dns_fallback_description()}
          />

          <SettingsCard data-slot="dns-nameserver-policy-card">
            <SettingsCardContent>
              <MappingField
                label={m.settings_clash_dns_nameserver_policy_label()}
                description={
                  features.list_values
                    ? m.settings_clash_dns_nameserver_policy_description()
                    : m.settings_clash_dns_single_value_description()
                }
                entries={draft.dns['nameserver-policy']}
                keyPlaceholder="geosite:cn"
                valuePlaceholder="223.5.5.5, 119.29.29.29"
                onChange={(entries) =>
                  updateDns({ 'nameserver-policy': entries })
                }
              />
            </SettingsCardContent>
          </SettingsCard>
        </SettingsGroup>
      </div>

      {draft.dns.fallback.length > 0 && <FallbackFilterSettings />}
    </>
  )
}
