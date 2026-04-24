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

export default function EnableBuiltinEnhancedSwitch() {
  const enableBuiltinEnhanced = useSetting('enable_builtin_enhanced')

  const handleChange = useLockFn(async () => {
    try {
      await enableBuiltinEnhanced.upsert(!enableBuiltinEnhanced.value)
    } catch (error) {
      message(
        `Update built-in enhanced failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="enable-builtin-enhanced-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="enable-builtin-enhanced-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_nyanpasu_enhance_enable_builtin_enhanced_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_nyanpasu_enhance_enable_builtin_enhanced_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            checked={Boolean(enableBuiltinEnhanced.value)}
            onCheckedChange={handleChange}
            loading={enableBuiltinEnhanced.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
