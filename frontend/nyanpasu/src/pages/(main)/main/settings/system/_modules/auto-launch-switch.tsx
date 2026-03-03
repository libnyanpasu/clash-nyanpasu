import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function AutoLaunchSwitch() {
  const autoLaunch = useSetting('enable_auto_launch')

  const handleAutoLaunch = useLockFn(async () => {
    try {
      await autoLaunch.upsert(!autoLaunch.value)
    } catch (error) {
      message(`Activation Auto Launch failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <ItemContainer data-slot="auto-launch-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_auto_launch_label()}
        </ItemLabelText>
      </ItemLabel>

      <Switch
        checked={Boolean(autoLaunch.value)}
        onCheckedChange={handleAutoLaunch}
        loading={autoLaunch.isPending}
      />
    </ItemContainer>
  )
}
