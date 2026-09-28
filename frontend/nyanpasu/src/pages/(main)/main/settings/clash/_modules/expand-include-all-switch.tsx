import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useClashSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function ExpandIncludeAllSwitch() {
  const { value, upsert } = useClashSetting('expand_include_all')

  const handleChange = useLockFn(async (input: boolean) => {
    try {
      await upsert(input)
    } catch (error) {
      message(
        `Update include-all expansion failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="expand-include-all-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="expand-include-all-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_clash_settings_expand_include_all_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_clash_settings_expand_include_all_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch checked={value !== false} onCheckedChange={handleChange} />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
