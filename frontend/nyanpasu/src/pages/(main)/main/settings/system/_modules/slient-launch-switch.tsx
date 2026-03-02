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

export default function SilentLaunchSwitch() {
  const silentStart = useSetting('enable_silent_start')

  const handleSilentStart = useLockFn(async () => {
    try {
      await silentStart.upsert(!silentStart.value)
    } catch (error) {
      message(
        `Activation Silent Start failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <ItemContainer data-slot="silent-launch-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_silent_start_label()}
        </ItemLabelText>
      </ItemLabel>

      <Switch
        checked={Boolean(silentStart.value)}
        onCheckedChange={handleSilentStart}
        loading={silentStart.isPending}
      />
    </ItemContainer>
  )
}
