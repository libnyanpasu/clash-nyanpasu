import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useNetworkAutomation } from '@nyanpasu/query'

export default function NetworkAutomationSwitch() {
  const { policy, save, isPending } = useNetworkAutomation()

  const handleToggle = useLockFn(async (next: boolean) => {
    if (!policy) {
      return
    }

    try {
      await save({ ...policy, enabled: next })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemSwitch
      data-slot="network-automation-switch-container"
      title={m.settings_system_proxy_network_automation_switch_label()}
      description={m.settings_system_proxy_network_automation_switch_description()}
      checked={Boolean(policy?.enabled)}
      onCheckedChange={handleToggle}
      loading={isPending}
    />
  )
}
