import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { ProxyMode, useProxyMode } from '@nyanpasu/interface'

export default function ProxyModeSelector() {
  const { value, upsert } = useProxyMode()

  const proxyModeMessages = {
    global: m.settings_system_proxy_global_mode_label(),
    direct: m.settings_system_proxy_direct_mode_label(),
    rule: m.settings_system_proxy_rule_mode_label(),
    script: m.settings_system_proxy_script_mode_label(),
  } satisfies Record<ProxyMode, string>

  const handleModeChange = useLockFn(async (mode: ProxyMode) => {
    await upsert(mode)
  })

  const selectedMode = Object.entries(value).find(([, enabled]) => enabled)?.[0]

  return (
    <AnimatedTabs
      className="h-14 w-full"
      activeTab={selectedMode}
      onChange={(mode) => handleModeChange(mode as ProxyMode)}
      variant="pill"
    >
      {Object.keys(value).map((mode) => (
        <AnimatedTabsItem key={mode} className="font-semibold" value={mode}>
          {proxyModeMessages[mode as ProxyMode]}
        </AnimatedTabsItem>
      ))}
    </AnimatedTabs>
  )
}
