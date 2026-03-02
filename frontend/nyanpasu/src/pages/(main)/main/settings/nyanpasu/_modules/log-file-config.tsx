import { useEffect, useState } from 'react'
import { Slider } from '@/components/ui/slider'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const MAX_LOG_FILES = 7

export default function LogFileConfig() {
  const { value, upsert } = useSetting('max_log_files')

  const committedValue = value ?? 1

  const [cachedValue, setCachedValue] = useState(committedValue)

  // sync the cached value with the committed value
  useEffect(() => {
    setCachedValue(committedValue)
  }, [committedValue])

  return (
    <SettingsCard data-slot="log-file-config-card">
      <SettingsCardContent
        data-slot="log-file-config-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <div className="flex items-center justify-between">
          <span>{m.settings_nyanpasu_max_log_files_label()}</span>

          <span>{cachedValue}</span>
        </div>

        <Slider
          value={cachedValue}
          min={1}
          max={MAX_LOG_FILES}
          step={1}
          onValueChange={(value) => {
            setCachedValue(value)
          }}
          onValueCommit={(value) => {
            if (value !== committedValue) {
              upsert(value)
            }
          }}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
