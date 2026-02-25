import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

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
    <SettingsCard data-slot="auto-launch-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="auto-launch-switch-card-content"
      >
        <div>{m.settings_system_proxy_auto_launch_label()}</div>

        <Switch
          checked={Boolean(autoLaunch.value)}
          onCheckedChange={handleAutoLaunch}
          loading={autoLaunch.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
