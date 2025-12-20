import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

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
    <SettingsCard data-slot="random-port-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="random-port-switch-card-content"
      >
        <div>{m.settings_clash_settings_random_port_label()}</div>

        <Switch
          checked={Boolean(enableRandomPort.value)}
          onCheckedChange={handleRandomPort}
          loading={enableRandomPort.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
