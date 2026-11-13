import { m } from '@/paraglide/messages'
import { byteUnitLabel } from '@/utils/information-unit'
import { useSetting } from '@nyanpasu/query'
import { type RpcAuditSettings } from '@nyanpasu/rpc/types'
import { SettingsSliderRow } from '../../../_modules/settings-slider-row'
import {
  ItemLabelDescription,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

// The bounds `RpcAuditSettings::validate` accepts.
const MIN_AGE_DAYS = 1
const MAX_AGE_DAYS = 90
const MIN_SIZE = 4
const MAX_SIZE = 256
const SIZE_STEP = 4

// `RpcAuditSettings::default`, only shown before the config has loaded.
const DEFAULT_SETTINGS = {
  max_age_days: 7,
  max_size_mib: 16,
} as const satisfies Required<RpcAuditSettings>

export default function RpcAuditRetentionConfig() {
  const { value, upsert } = useSetting('rpc_audit')

  const settings = { ...DEFAULT_SETTINGS, ...value }

  // The settings are replaced as a whole, so every change carries the rest.
  const handleAgeCommit = (days: number) =>
    upsert({ ...settings, max_age_days: days })

  const handleSizeCommit = (size: number) =>
    upsert({ ...settings, max_size_mib: size })

  return (
    <SettingsCard data-slot="rpc-audit-retention-config-card">
      <SettingsCardContent
        data-slot="rpc-audit-retention-config-card-content"
        className="gap-4"
      >
        <SettingsSliderRow
          label={m.settings_logs_rpc_audit_max_age_label()}
          committedValue={settings.max_age_days}
          min={MIN_AGE_DAYS}
          max={MAX_AGE_DAYS}
          onCommit={handleAgeCommit}
        />

        <SettingsSliderRow
          label={m.settings_logs_rpc_audit_max_size_label()}
          committedValue={settings.max_size_mib}
          min={MIN_SIZE}
          max={MAX_SIZE}
          step={SIZE_STEP}
          unit={` ${byteUnitLabel('MiB')}`}
          onCommit={handleSizeCommit}
        />

        <ItemLabelDescription>
          {m.settings_logs_rpc_audit_hint()}
        </ItemLabelDescription>
      </SettingsCardContent>
    </SettingsCard>
  )
}
