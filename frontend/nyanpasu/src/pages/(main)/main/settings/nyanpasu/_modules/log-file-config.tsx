import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/query'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'
import { SettingsSliderRow } from '../../_modules/settings-slider-row'

const MAX_LOG_FILES = 7

// MiB
const MAX_LOG_FILE_SIZE = 100

export default function LogFileConfig() {
  const maxLogFiles = useSetting('max_log_files')

  const maxLogFileSize = useSetting('max_log_file_size')

  return (
    <SettingsCard data-slot="log-file-config-card">
      <SettingsCardContent
        data-slot="log-file-config-card-content"
        className="gap-4"
      >
        <SettingsSliderRow
          label={m.settings_nyanpasu_max_log_files_label()}
          committedValue={maxLogFiles.value ?? 1}
          max={MAX_LOG_FILES}
          onCommit={(value) => maxLogFiles.upsert(value)}
        />

        <SettingsSliderRow
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
