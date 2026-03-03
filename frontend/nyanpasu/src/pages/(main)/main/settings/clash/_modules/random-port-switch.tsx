import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function RandomPortSwitch() {
  const enableRandomPort = useSetting('enable_random_port')

  const handleRandomPort = async () => {
    try {
      await enableRandomPort.upsert(!enableRandomPort.value)
    } catch (e) {
      message(formatError(e), {
        title: 'Error',
        kind: 'error',
      })
    } finally {
      message(
        enableRandomPort.value
          ? m.settings_clash_settings_random_port_disabled()
          : m.settings_clash_settings_random_port_enabled(),
        {
          title: 'Successful',
          kind: 'info',
        },
      )
    }
  }

  return (
    <ItemContainer data-slot="auto-launch-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_random_port_label()}
        </ItemLabelText>
      </ItemLabel>

      <Switch
        checked={Boolean(enableRandomPort.value)}
        onCheckedChange={handleRandomPort}
        loading={enableRandomPort.isPending}
      />
    </ItemContainer>
  )
}
