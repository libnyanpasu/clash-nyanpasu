import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function BreakWhenModeChangeSwitch() {
  const breakConnection = useClashSetting('break_connection')

  const checked = Boolean(breakConnection.value?.on_mode_change)

  const handleChange = useLockFn(async () => {
    try {
      await breakConnection.upsert({ on_mode_change: !checked })
    } catch (error) {
      message(
        `Update break when mode change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
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
            checked={checked}
            onCheckedChange={handleChange}
            loading={breakConnection.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
