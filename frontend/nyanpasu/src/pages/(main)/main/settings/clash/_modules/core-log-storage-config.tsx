import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import {
  type CoreLogCompression,
  type CoreLogSettings,
} from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'
import { SettingsSliderRow } from '../../_modules/settings-slider-row'

// MiB, the bounds `CoreLogSettings::validate` accepts: a shard of at least
// 4 MiB, and a budget that holds at least two shards.
const MIN_SHARD_SIZE = 4
const MAX_SHARD_SIZE = 64
const SHARD_SIZE_STEP = 4
const MAX_BUDGET = 1024
const BUDGET_STEP = 8

// `CoreLogSettings::default`, only shown before the config has loaded.
const DEFAULT_SETTINGS = {
  shard_size_mib: 16,
  max_size_mib: 64,
  compression: 'preset',
} as const satisfies Required<CoreLogSettings>

export default function CoreLogStorageConfig() {
  const { value, upsert } = useSetting('core_logs')

  const settings = { ...DEFAULT_SETTINGS, ...value }

  const compressionMessages = {
    none: m.settings_clash_settings_core_log_compression_none(),
    preset: m.settings_clash_settings_core_log_compression_preset(),
    trained: m.settings_clash_settings_core_log_compression_trained(),
  } satisfies Record<CoreLogCompression, string>

  // The settings are replaced as a whole, so every change carries the rest.
  // A larger shard raises the budget with it to keep room for two shards.
  const handleShardSizeCommit = (shardSize: number) =>
    upsert({
      ...settings,
      shard_size_mib: shardSize,
      max_size_mib: Math.max(settings.max_size_mib, shardSize * 2),
    })

  const handleBudgetCommit = (budget: number) =>
    upsert({ ...settings, max_size_mib: budget })

  const handleCompressionChange = useLockFn(
    async (compression: CoreLogCompression) => {
      await upsert({ ...settings, compression })
    },
  )

  return (
    <>
      <SettingsCard data-slot="core-log-storage-config-card">
        <SettingsCardContent
          data-slot="core-log-storage-config-card-content"
          className="gap-4"
        >
          <SettingsSliderRow
            label={m.settings_clash_settings_core_log_shard_size_label()}
            committedValue={settings.shard_size_mib}
            min={MIN_SHARD_SIZE}
            max={MAX_SHARD_SIZE}
            step={SHARD_SIZE_STEP}
            unit=" MiB"
            onCommit={handleShardSizeCommit}
          />

          <SettingsSliderRow
            label={m.settings_clash_settings_core_log_max_size_label()}
            committedValue={settings.max_size_mib}
            min={settings.shard_size_mib * 2}
            max={MAX_BUDGET}
            step={BUDGET_STEP}
            unit=" MiB"
            onCommit={handleBudgetCommit}
          />
        </SettingsCardContent>
      </SettingsCard>

      <SettingsCard data-slot="core-log-compression-selector">
        <DropdownMenu align="end">
          <DropdownMenuTrigger asChild>
            <SettingsCardContent
              data-slot="core-log-compression-selector-trigger"
              asChild
            >
              <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
                <ItemContainer>
                  <ItemLabel>
                    <ItemLabelText>
                      {m.settings_clash_settings_core_log_compression_label()}
                    </ItemLabelText>

                    <ItemLabelDescription>
                      {compressionMessages[settings.compression]}
                    </ItemLabelDescription>
                  </ItemLabel>

                  <ArrowForwardIosRounded />
                </ItemContainer>
              </Button>
            </SettingsCardContent>
          </DropdownMenuTrigger>

          <DropdownMenuContent sideOffset={-16} alignOffset={16}>
            {Object.entries(compressionMessages).map(([key, message]) => (
              <DropdownMenuCheckboxItem
                checked={settings.compression === key}
                key={key}
                onSelect={() =>
                  handleCompressionChange(key as CoreLogCompression)
                }
              >
                {message}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </SettingsCard>
    </>
  )
}
