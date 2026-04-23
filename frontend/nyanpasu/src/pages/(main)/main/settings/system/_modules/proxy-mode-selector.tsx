import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
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
    <SegmentedButton
      className="h-16"
      variant="tabs"
      value={selectedMode}
      onValueChange={(mode) => handleModeChange(mode as ProxyMode)}
    >
      {Object.keys(value).map((mode) => (
        <SegmentedButtonItem
          key={mode}
          className="text-base font-bold"
          value={mode}
        >
          {proxyModeMessages[mode as ProxyMode]}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
  )
}
