import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSystemSetting } from '@nyanpasu/query'

export default function ProxyGuardSwitch() {
  const proxyGuard = useSystemSetting('enable_proxy_guard')

  const handleProxyGuard = useLockFn(async (next: boolean) => {
    try {
      await proxyGuard.upsert(next)
    } catch (error) {
      message(`Activation Proxy Guard failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemSwitch
      data-slot="proxy-guard-switch-container"
      title={m.settings_system_proxy_proxy_guard_switch_label()}
      description={m.settings_system_proxy_proxy_guard_switch_description()}
      checked={Boolean(proxyGuard.value)}
      onCheckedChange={handleProxyGuard}
      loading={proxyGuard.isPending}
    />
  )
}
