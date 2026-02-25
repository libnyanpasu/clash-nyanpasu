import { useCallback, useMemo } from 'react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { useClashConfig } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const LOG_LEVEL_OPTIONS = {
  debug: 'Debug',
  info: 'Info',
  warning: 'Warn',
  error: 'Error',
  silent: 'Silent',
} as const

export default function LogLevelSelector() {
  const { query, upsert } = useClashConfig()

  const value = useMemo(
    () => query.data?.['log-level'] as keyof typeof LOG_LEVEL_OPTIONS,
    [query.data],
  )

  const handleLogLevelChange = useCallback(
    async (value: string) => {
      await upsert.mutateAsync({
        'log-level': value as string,
      })
    },
    [upsert],
  )

  return (
    <SettingsCard data-slot="log-level-selector-card">
      <SettingsCardContent
        className="px-2"
        data-slot="log-level-selector-card-content"
      >
        <Select
          variant="outlined"
          value={value}
          onValueChange={handleLogLevelChange}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={m.settings_clash_settings_log_level_label()}
            >
              {value ? LOG_LEVEL_OPTIONS[value] : null}
            </SelectValue>
          </SelectTrigger>

          <SelectContent>
            {Object.entries(LOG_LEVEL_OPTIONS).map(([key, value]) => (
              <SelectItem key={key} value={key}>
                {value}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsCardContent>
    </SettingsCard>
  )
}
