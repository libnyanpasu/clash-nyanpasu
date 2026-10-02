import { Switch } from '@nyanpasu/ui/switch'
import { useMockConnectionsSetting } from '@/hooks/use-mock-connections'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function MockConnectionsSwitch() {
  const [enabled, setEnabled] = useMockConnectionsSetting()

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardContent>
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>Mock Connections</ItemLabelText>

              <ItemLabelDescription>
                Show generated active and closed connections on the connections
                page instead of the core&apos;s.
              </ItemLabelDescription>
            </ItemLabel>

            <Switch checked={enabled} onCheckedChange={setEnabled} />
          </ItemContainer>
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
