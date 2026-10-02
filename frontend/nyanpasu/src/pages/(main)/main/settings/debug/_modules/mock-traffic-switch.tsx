import { Switch } from '@nyanpasu/ui/switch'
import { useMockTrafficSetting } from '@/hooks/use-mock-traffic'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function MockTrafficSwitch() {
  const [enabled, setEnabled] = useMockTrafficSetting()

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardContent>
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>Mock Traffic</ItemLabelText>

              <ItemLabelDescription>
                Show generated usage on the traffic page, including the map and
                its location bases, instead of the recorded traffic.
              </ItemLabelDescription>
            </ItemLabel>

            <Switch checked={enabled} onCheckedChange={setEnabled} />
          </ItemContainer>
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
