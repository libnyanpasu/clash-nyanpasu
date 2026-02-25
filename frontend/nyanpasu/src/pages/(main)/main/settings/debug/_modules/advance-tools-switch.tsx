import { SwitchItem } from '@/components/ui/switch'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'
import { useDebugContext } from './debug-provider'

export default function AdvanceToolsSwitch() {
  const { advanceTools, setAdvanceTools } = useDebugContext()

  return (
    <SettingsCard data-slot="advance-tools-switch-card">
      <SettingsCardContent
        data-slot="advance-tools-switch-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <SwitchItem
          className="rounded-3xl"
          checked={advanceTools}
          onCheckedChange={setAdvanceTools}
        >
          Advance Tools
        </SwitchItem>
      </SettingsCardContent>
    </SettingsCard>
  )
}
