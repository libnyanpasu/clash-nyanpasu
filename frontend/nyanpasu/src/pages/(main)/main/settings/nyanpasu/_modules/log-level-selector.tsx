import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { LoggingLevel, useSetting } from '@nyanpasu/interface'

export default function LogLevelSelector() {
  const { value, upsert } = useSetting('app_log_level')

  const messages = {
    trace: 'Trace',
    debug: 'Debug',
    info: 'Info',
    warn: 'Warn',
    error: 'Error',
    silent: 'Silent',
  } satisfies Record<LoggingLevel, string>

  return (
    <Select
      variant="outlined"
      value={value || 'info'}
      onValueChange={(value) => upsert(value as LoggingLevel)}
    >
      <SelectTrigger>
        <SelectValue placeholder={m.settings_nyanpasu_app_log_level_label()}>
          {value ? messages[value] : null}
        </SelectValue>
      </SelectTrigger>

      <SelectContent>
        {Object.entries(messages).map(([key, value]) => (
          <SelectItem key={key} value={key}>
            {value}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
