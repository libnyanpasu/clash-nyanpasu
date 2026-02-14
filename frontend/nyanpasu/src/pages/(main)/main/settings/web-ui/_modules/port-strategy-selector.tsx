import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { ExternalControllerPortStrategy, useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

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
    <SettingsCard data-slot="port-strategy-selector-card">
      <SettingsCardContent
        className="px-2"
        data-slot="port-strategy-selector-card-content"
      >
        <Select
          variant="outlined"
          value={value?.external_controller_port_strategy || 'allow_fallback'}
          onValueChange={handlePortStrategyChange}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={m.settings_clash_settings_log_level_label()}
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
      </SettingsCardContent>
    </SettingsCard>
  )
}
