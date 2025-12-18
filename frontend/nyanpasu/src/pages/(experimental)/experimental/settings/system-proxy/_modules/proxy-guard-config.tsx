import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ProxyGuardConfig() {
  const proxyGuardInterval = useSetting('proxy_guard_interval')

  return (
    <SettingsCard data-slot="proxy-guard-config-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="proxy-guard-config-card-content"
      >
        <span>{m.settings_system_proxy_proxy_guard_interval_label()}</span>

        {/* TODO: implement input component */}
        <div className="border-surface-variant flex w-24 items-center justify-between rounded-md border p-2">
          <span>{proxyGuardInterval.value || 0}</span>
          <span>{m.unit_seconds()}</span>
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}
