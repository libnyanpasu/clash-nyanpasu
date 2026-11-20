import { Input } from '@nyanpasu/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { useClashSetting } from '@nyanpasu/query'
import type { EnhancedMode, FakeIpFilterMode } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { useDnsDraft, type DnsValues } from './dns-draft'
import { StringListField } from './list-fields'

type BooleanKey = {
  [K in keyof DnsValues]: DnsValues[K] extends boolean ? K : never
}[keyof DnsValues]

const ENHANCED_MODE_LABELS: Record<EnhancedMode, string> = {
  normal: 'Normal',
  'fake-ip': 'Fake-IP',
  'redir-host': 'Redir-Host',
}

const FAKE_IP_FILTER_MODE_LABELS: Record<FakeIpFilterMode, () => string> = {
  blacklist: m.settings_clash_dns_fake_ip_filter_mode_blacklist,
  whitelist: m.settings_clash_dns_fake_ip_filter_mode_whitelist,
  rule: m.settings_clash_dns_fake_ip_filter_mode_rule,
}

/** A `dns` switch, shown only when the selected core reads its key. */
function DnsSwitch({
  field,
  label,
  description,
  locked,
}: {
  field: BooleanKey
  label: string
  description?: string
  /** Shown on and not editable, such as `enable` while TUN is on. */
  locked?: boolean
}) {
  const { draft, updateDns, reads } = useDnsDraft()

  if (!reads(field)) {
    return null
  }

  return (
    <SettingsCard data-slot="dns-switch-card">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>{label}</ItemLabelText>

            {description && (
              <ItemLabelDescription>{description}</ItemLabelDescription>
            )}
          </ItemLabel>

          <Switch
            aria-label={label}
            checked={locked || draft.dns[field]}
            disabled={locked}
            onCheckedChange={(checked) => updateDns({ [field]: checked })}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}

/** A text `dns` key that may be left unset. */
function DnsTextField({
  field,
  label,
  placeholder,
}: {
  field: 'listen' | 'fake-ip-range' | 'fake-ip-range6'
  label: string
  placeholder: string
}) {
  const { draft, updateDns, reads } = useDnsDraft()

  if (!reads(field)) {
    return null
  }

  return (
    <SettingsCard data-slot="dns-text-field-card">
      <SettingsCardContent>
        <Input
          label={label}
          value={draft.dns[field] ?? ''}
          placeholder={placeholder}
          onChange={(event) =>
            updateDns({ [field]: event.target.value.trim() || null })
          }
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}

function EnhancedModeSelect() {
  const { draft, updateDns, features } = useDnsDraft()

  const modes =
    features.enhanced_mode.kind === 'values' ? features.enhanced_mode.value : []

  return (
    <SettingsCard data-slot="dns-enhanced-mode-card">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>
              {m.settings_clash_dns_enhanced_mode_label()}
            </ItemLabelText>
          </ItemLabel>

          <Select
            variant="chip"
            value={draft.dns['enhanced-mode']}
            onValueChange={(value) =>
              updateDns({ 'enhanced-mode': value as EnhancedMode })
            }
          >
            <SelectTrigger
              aria-label={m.settings_clash_dns_enhanced_mode_label()}
            >
              <SelectValue>
                {ENHANCED_MODE_LABELS[draft.dns['enhanced-mode']]}
              </SelectValue>
            </SelectTrigger>

            <SelectContent>
              {modes.map((mode) => (
                <SelectItem key={mode} value={mode}>
                  {ENHANCED_MODE_LABELS[mode]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}

function FakeIpSettings() {
  const { draft, updateDns, features, reads } = useDnsDraft()

  const filterModes =
    features.fake_ip_filter_mode.kind === 'values'
      ? features.fake_ip_filter_mode.value
      : []

  const filterMode = draft.dns['fake-ip-filter-mode']

  return (
    <div data-slot="dns-fake-ip-settings">
      <SettingsLabel>{m.settings_clash_dns_fake_ip_label()}</SettingsLabel>

      <SettingsGroup>
        <DnsTextField
          field="fake-ip-range"
          label={m.settings_clash_dns_fake_ip_range_label()}
          placeholder="198.18.0.1/16"
        />

        <DnsTextField
          field="fake-ip-range6"
          label={m.settings_clash_dns_fake_ip_range6_label()}
          placeholder="fdfe:dcba:9876::1/64"
        />

        {reads('fake-ip-filter-mode') && (
          <SettingsCard data-slot="dns-fake-ip-filter-mode-card">
            <SettingsCardContent>
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_dns_fake_ip_filter_mode_label()}
                  </ItemLabelText>
                </ItemLabel>

                <Select
                  variant="chip"
                  value={filterMode}
                  onValueChange={(value) =>
                    updateDns({
                      'fake-ip-filter-mode': value as FakeIpFilterMode,
                    })
                  }
                >
                  <SelectTrigger
                    aria-label={m.settings_clash_dns_fake_ip_filter_mode_label()}
                  >
                    <SelectValue>
                      {FAKE_IP_FILTER_MODE_LABELS[filterMode]()}
                    </SelectValue>
                  </SelectTrigger>

                  <SelectContent>
                    {filterModes.map((mode) => (
                      <SelectItem key={mode} value={mode}>
                        {FAKE_IP_FILTER_MODE_LABELS[mode]()}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </ItemContainer>
            </SettingsCardContent>
          </SettingsCard>
        )}

        <SettingsCard data-slot="dns-fake-ip-filter-card">
          <SettingsCardContent>
            <StringListField
              label={m.settings_clash_dns_fake_ip_filter_label()}
              description={
                filterMode === 'rule'
                  ? m.settings_clash_dns_fake_ip_filter_rule_description()
                  : m.settings_clash_dns_fake_ip_filter_description()
              }
              values={draft.dns['fake-ip-filter']}
              placeholder={
                filterMode === 'rule' ? 'DOMAIN-SUFFIX,lan,real-ip' : '+.lan'
              }
              onChange={(values) => updateDns({ 'fake-ip-filter': values })}
            />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

export default function DnsGeneralSection() {
  const { draft } = useDnsDraft()

  const enableTunMode = useClashSetting('enable_tun_mode')

  return (
    <>
      <div data-slot="dns-general-settings">
        <SettingsLabel>{m.settings_clash_dns_general_label()}</SettingsLabel>

        <SettingsGroup>
          <DnsSwitch
            field="enable"
            label={m.settings_clash_dns_enable_label()}
            description={
              enableTunMode.value
                ? m.settings_clash_dns_enable_tun_description()
                : undefined
            }
            locked={Boolean(enableTunMode.value)}
          />

          <EnhancedModeSelect />

          <DnsTextField
            field="listen"
            label={m.settings_clash_dns_listen_label()}
            placeholder="127.0.0.1:1053"
          />

          <DnsSwitch field="ipv6" label={m.settings_clash_dns_ipv6_label()} />

          <DnsSwitch
            field="respect-rules"
            label={m.settings_clash_dns_respect_rules_label()}
            description={m.settings_clash_dns_respect_rules_description()}
          />

          <DnsSwitch
            field="use-hosts"
            label={m.settings_clash_dns_use_hosts_label()}
          />

          <DnsSwitch
            field="use-system-hosts"
            label={m.settings_clash_dns_use_system_hosts_label()}
          />

          <DnsSwitch
            field="prefer-h3"
            label={m.settings_clash_dns_prefer_h3_label()}
          />
        </SettingsGroup>
      </div>

      {draft.dns['enhanced-mode'] === 'fake-ip' && <FakeIpSettings />}
    </>
  )
}
