import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { randomPortKind, useClashSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function RandomPortSwitch() {
  const mixedPort = useClashSetting('mixed_port')

  const isRandomPort = mixedPort.value?.kind === 'random'

  const handleRandomPort = async () => {
    try {
      await mixedPort.upsert({ kind: randomPortKind(!isRandomPort) })
    } catch (e) {
      message(formatError(e), {
        title: 'Error',
        kind: 'error',
      })
    } finally {
      message(
        isRandomPort
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
        checked={isRandomPort}
        onCheckedChange={handleRandomPort}
        loading={mixedPort.isPending}
      />
    </ItemContainer>
  )
}
