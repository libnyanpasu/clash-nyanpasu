import { useState } from 'react'
import {
  Dialog,
  DialogTrigger,
  FormDialogContent,
  useDialog,
} from '@nyanpasu/ui/dialog'
import { Input } from '@nyanpasu/ui/input'
import {
  Item,
  ItemAction,
  ItemContent,
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
  ItemSwitch,
  ItemTitle,
  ItemTrailing,
} from '@nyanpasu/ui/item-group'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { m } from '@/paraglide/messages'
import { useClashSetting } from '@nyanpasu/query'
import type { EnhancedMode, FakeIpFilterMode } from '@nyanpasu/rpc/types'
import {
  SettingsCard,
  SettingsCardContent,
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
    <ItemSwitch
      data-slot="dns-switch-card"
      title={label}
      description={description}
      checked={locked || draft.dns[field]}
      disabled={locked}
      onCheckedChange={(checked) => updateDns({ [field]: checked })}
    />
  )
}

function DnsTextFieldDialog({
  field,
  label,
  placeholder,
}: {
  field: 'listen' | 'fake-ip-range' | 'fake-ip-range6'
  label: string
  placeholder: string
}) {
  const { setOpen } = useDialog()
  const { draft, updateDns } = useDnsDraft()
  const current = draft.dns[field] ?? ''
  const [value, setValue] = useState(current)

  const handleSubmit = () => {
    updateDns({ [field]: value.trim() || null })
    setOpen(false)
  }

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          data-slot="dns-text-field-card"
          title={label}
          description={current || placeholder}
        />
      </DialogTrigger>

      <FormDialogContent
        title={label}
        submitLabel={m.common_apply()}
        cancelLabel={m.common_cancel()}
        onOpen={() => setValue(draft.dns[field] ?? '')}
        onSubmit={handleSubmit}
      >
        <Input
          variant="underlined"
          aria-label={label}
          placeholder={placeholder}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          autoFocus
        />
      </FormDialogContent>
    </>
  )
}

/** A text `dns` key edited in an A17 FormDialog and left unset when empty. */
function DnsTextField({
  field,
  label,
  placeholder,
}: {
  field: 'listen' | 'fake-ip-range' | 'fake-ip-range6'
  label: string
  placeholder: string
}) {
  const { reads } = useDnsDraft()

  if (!reads(field)) {
    return null
  }

  return (
    <Dialog>
      <DnsTextFieldDialog
        field={field}
        label={label}
        placeholder={placeholder}
      />
    </Dialog>
  )
}

function EnhancedModeSelect() {
  const { draft, updateDns, features } = useDnsDraft()

  const modes =
    features.enhanced_mode.kind === 'values' ? features.enhanced_mode.value : []

  return (
    <Item data-slot="dns-enhanced-mode-card">
      <ItemContent>
        <ItemTitle>{m.settings_clash_dns_enhanced_mode_label()}</ItemTitle>
      </ItemContent>

      <ItemTrailing>
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
      </ItemTrailing>
    </Item>
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
    <ItemSection data-slot="dns-fake-ip-settings">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_dns_fake_ip_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
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
          <Item data-slot="dns-fake-ip-filter-mode-card">
            <ItemContent>
              <ItemTitle>
                {m.settings_clash_dns_fake_ip_filter_mode_label()}
              </ItemTitle>
            </ItemContent>

            <ItemTrailing>
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
            </ItemTrailing>
          </Item>
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
      </ItemGroup>
    </ItemSection>
  )
}

export default function DnsGeneralSection() {
  const { draft } = useDnsDraft()

  const enableTunMode = useClashSetting('enable_tun_mode')

  return (
    <>
      <ItemSection data-slot="dns-general-settings">
        <ItemSectionHeader>
          <ItemSectionTitle>
            {m.settings_clash_dns_general_label()}
          </ItemSectionTitle>
        </ItemSectionHeader>

        <ItemGroup>
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
        </ItemGroup>
      </ItemSection>

      {draft.dns['enhanced-mode'] === 'fake-ip' && <FakeIpSettings />}
    </>
  )
}
