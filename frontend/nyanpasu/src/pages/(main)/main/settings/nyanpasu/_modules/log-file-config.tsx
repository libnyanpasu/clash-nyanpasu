import { useEffect, useState } from 'react'
import { Slider } from '@/components/ui/slider'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const MAX_LOG_FILES = 7

// MiB
const MAX_LOG_FILE_SIZE = 100

function SliderRow({
  label,
  committedValue,
  max,
  unit,
  onCommit,
}: {
  label: string
  committedValue: number
  max: number
  unit?: string
  onCommit: (value: number) => void
}) {
  const [cachedValue, setCachedValue] = useState(committedValue)

  // sync the cached value with the committed value
  useEffect(() => {
    setCachedValue(committedValue)
  }, [committedValue])

  return (
    <>
      <div className="flex items-center justify-between">
        <span>{label}</span>

        <span>
          {cachedValue}
          {unit}
        </span>
      </div>

      <Slider
        value={cachedValue}
        min={1}
        max={max}
        step={1}
        onValueChange={(value) => {
          setCachedValue(value)
        }}
        onValueCommit={(value) => {
          if (value !== committedValue) {
            onCommit(value)
          }
        }}
      />
    </>
  )
}

export default function LogFileConfig() {
  const maxLogFiles = useSetting('max_log_files')

  const maxLogFileSize = useSetting('max_log_file_size')

  return (
    <SettingsCard data-slot="log-file-config-card">
      <SettingsCardContent
        data-slot="log-file-config-card-content"
        className="gap-4"
      >
        <SliderRow
          label={m.settings_nyanpasu_max_log_files_label()}
          committedValue={maxLogFiles.value ?? 1}
          max={MAX_LOG_FILES}
          onCommit={(value) => maxLogFiles.upsert(value)}
        />

        <SliderRow
          label={m.settings_nyanpasu_max_log_file_size_label()}
          committedValue={maxLogFileSize.value ?? 10}
          max={MAX_LOG_FILE_SIZE}
          unit=" MiB"
          onCommit={(value) => maxLogFileSize.upsert(value)}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
