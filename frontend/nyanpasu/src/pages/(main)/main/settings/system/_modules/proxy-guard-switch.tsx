import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ProxyGuardSwitch() {
  const proxyGuard = useSetting('enable_proxy_guard')

  const handleProxyGuard = useLockFn(async () => {
    try {
      await proxyGuard.upsert(!proxyGuard.value)
    } catch (error) {
      message(`Activation Proxy Guard failed!`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <SettingsCard data-slot="proxy-guard-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="proxy-guard-switch-card-content"
      >
        <div>{m.settings_system_proxy_proxy_guard_label()}</div>

        <Switch
          checked={Boolean(proxyGuard.value)}
          onCheckedChange={handleProxyGuard}
          loading={proxyGuard.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
