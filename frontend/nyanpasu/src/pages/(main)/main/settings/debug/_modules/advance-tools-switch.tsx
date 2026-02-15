import { SwitchItem } from '@/components/ui/switch'
import { useDebugContext } from './debug-provider'

export default function AdvanceToolsSwitch() {
  const { advanceTools, setAdvanceTools } = useDebugContext()

  return (
    <SwitchItem
      className="rounded-3xl"
      checked={advanceTools}
      onCheckedChange={setAdvanceTools}
    >
      Advance Tools
    </SwitchItem>
  )
}
