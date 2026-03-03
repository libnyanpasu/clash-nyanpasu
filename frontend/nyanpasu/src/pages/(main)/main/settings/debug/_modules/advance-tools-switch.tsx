import { Switch } from '@/components/ui/switch'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'
import { useDebugContext } from './debug-provider'

export default function AdvanceToolsSwitch() {
  const { advanceTools, setAdvanceTools } = useDebugContext()

  return (
    <ItemContainer data-slot="allow-lan-switch-container">
      <ItemLabel>
        <ItemLabelText>Advance Tools</ItemLabelText>
      </ItemLabel>

      <Switch
        checked={Boolean(advanceTools)}
        onCheckedChange={setAdvanceTools}
      />
    </ItemContainer>
  )
}
