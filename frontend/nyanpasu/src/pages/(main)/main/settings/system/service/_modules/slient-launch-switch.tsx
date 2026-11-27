import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'

export default function SilentLaunchSwitch() {
  const silentStart = useSetting('enable_silent_start')

  const handleSilentStart = useLockFn(async (next: boolean) => {
    try {
      await silentStart.upsert(next)
    } catch (error) {
      message(
        `Activation Silent Start failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
        },
      )
    }
  })

  return (
    <ItemSwitch
      data-slot="silent-launch-switch-container"
      title={m.settings_system_proxy_silent_start_label()}
      checked={Boolean(silentStart.value)}
      onCheckedChange={handleSilentStart}
      loading={silentStart.isPending}
    />
  )
}
