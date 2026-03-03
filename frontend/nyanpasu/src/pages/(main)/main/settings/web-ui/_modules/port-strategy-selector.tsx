import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { ExternalControllerPortStrategy, useSetting } from '@nyanpasu/interface'

export default function PortStrategySelector() {
  const { value, upsert } = useSetting('clash_strategy')

  const messages = {
    allow_fallback: m.settings_clash_settings_allow_fallback_label(),
    fixed: m.settings_clash_settings_fixed_label(),
    random: m.settings_clash_settings_random_label(),
  } as Record<ExternalControllerPortStrategy, string>

  const handlePortStrategyChange = async (
    value: ExternalControllerPortStrategy,
  ) => {
    await upsert({
      external_controller_port_strategy: value,
    })
  }

  return (
    <Select
      variant="outlined"
      value={value?.external_controller_port_strategy || 'allow_fallback'}
      onValueChange={handlePortStrategyChange}
    >
      <SelectTrigger>
        <SelectValue
          placeholder={m.settings_clash_settings_port_strategy_label()}
        >
          {
            messages[
              value?.external_controller_port_strategy || 'allow_fallback'
            ]
          }
        </SelectValue>
      </SelectTrigger>

      <SelectContent>
        {Object.entries(messages).map(([key, message]) => (
          <SelectItem key={key} value={key}>
            {message}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
