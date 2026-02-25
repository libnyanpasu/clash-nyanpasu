import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

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
    <SettingsCard data-slot="silent-launch-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="silent-launch-switch-card-content"
      >
        <div>{m.settings_system_proxy_silent_start_label()}</div>

        <Switch
          checked={Boolean(silentStart.value)}
          onCheckedChange={handleSilentStart}
          loading={silentStart.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
