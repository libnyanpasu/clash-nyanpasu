import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function BreakWhenModeChangeSwitch() {
  const breakWhenModeChange = useSetting('break_when_mode_change')

  const handleChange = useLockFn(async () => {
    try {
      await breakWhenModeChange.upsert(!breakWhenModeChange.value)
    } catch (error) {
      message(
        `Update break when mode change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="break-when-mode-change-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="break-when-mode-change-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_nyanpasu_enhance_break_when_mode_change_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_nyanpasu_enhance_break_when_mode_change_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            checked={Boolean(breakWhenModeChange.value)}
            onCheckedChange={handleChange}
            loading={breakWhenModeChange.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
