import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useNetworkAutomation } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../../_modules/settings-card'

export default function NetworkAutomationSwitch() {
  const { policy, save, isPending } = useNetworkAutomation()

  const handleToggle = useLockFn(async () => {
    if (!policy) {
      return
    }

    try {
      await save({ ...policy, enabled: !policy.enabled })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemContainer data-slot="network-automation-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_network_automation_switch_label()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_system_proxy_network_automation_switch_description()}
        </ItemLabelDescription>
      </ItemLabel>

      <Switch
        checked={Boolean(policy?.enabled)}
        onCheckedChange={handleToggle}
        loading={isPending}
      />
    </ItemContainer>
  )
}
