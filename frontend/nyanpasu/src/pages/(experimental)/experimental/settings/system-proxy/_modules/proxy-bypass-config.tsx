import { m } from '@/paraglide/messages'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ProxyBypassConfig() {
  return (
    <SettingsCard data-slot="proxy-guard-config-card">
      <SettingsCardContent
        className="pr-3 pl-2"
        data-slot="proxy-guard-config-card-content"
      >
        {/* TODO: implement input component */}
        <div className="border-surface-variant flex h-14 w-full items-center justify-between rounded-md border p-4">
          <span>{m.settings_system_proxy_proxy_bypass_label()}</span>
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}
