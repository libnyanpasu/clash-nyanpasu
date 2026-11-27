import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSystemSetting } from '@nyanpasu/query'

export default function AutoLaunchSwitch() {
  const autoLaunch = useSystemSetting('enable_auto_launch')

  const handleAutoLaunch = useLockFn(async (next: boolean) => {
    try {
      await autoLaunch.upsert(next)
    } catch (error) {
      message(`Activation Auto Launch failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemSwitch
      data-slot="auto-launch-switch-container"
      title={m.settings_system_proxy_auto_launch_label()}
      checked={Boolean(autoLaunch.value)}
      onCheckedChange={handleAutoLaunch}
      loading={autoLaunch.isPending}
    />
  )
}
