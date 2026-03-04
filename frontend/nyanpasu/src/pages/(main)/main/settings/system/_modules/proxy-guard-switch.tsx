import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function ProxyGuardSwitch() {
  const proxyGuard = useSetting('enable_proxy_guard')

  const handleProxyGuard = useLockFn(async () => {
    try {
      await proxyGuard.upsert(!proxyGuard.value)
    } catch (error) {
      message(`Activation Proxy Guard failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <ItemContainer data-slot="proxy-guard-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_proxy_guard_switch_label()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_system_proxy_proxy_guard_switch_description()}
        </ItemLabelDescription>
      </ItemLabel>

      <Switch
        checked={Boolean(proxyGuard.value)}
        onCheckedChange={handleProxyGuard}
        loading={proxyGuard.isPending}
      />
    </ItemContainer>
  )
}
