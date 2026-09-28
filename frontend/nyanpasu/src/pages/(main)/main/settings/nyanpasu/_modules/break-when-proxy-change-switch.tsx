import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  breaksOnProxyChange,
  proxyChangeBreakMode,
  useClashSetting,
} from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function BreakWhenProxyChangeSwitch() {
  const breakConnection = useClashSetting('break_connection')

  const checked = breakConnection.value
    ? breaksOnProxyChange(breakConnection.value)
    : false

  const handleChange = useLockFn(async () => {
    try {
      await breakConnection.upsert({
        on_proxy_change: proxyChangeBreakMode(!checked),
      })
    } catch (error) {
      message(
        `Update break when proxy change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="break-when-proxy-change-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="break-when-proxy-change-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_nyanpasu_enhance_break_when_proxy_change_description()}
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
