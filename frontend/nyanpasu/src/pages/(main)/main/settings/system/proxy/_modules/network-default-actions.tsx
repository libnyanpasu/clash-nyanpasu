import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useNetworkAutomation } from '@nyanpasu/query'
import type { MechanismActions } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'
import MechanismActionsFields from './mechanism-actions-fields'

/** What a network that no rule matches does to each mechanism. */
export default function NetworkDefaultActions() {
  const { policy, save } = useNetworkAutomation()

  const handleChange = async (default_actions: MechanismActions) => {
    if (!policy) {
      return
    }

    try {
      await save({ ...policy, default_actions })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  }

  return (
    <div
      className="flex flex-col gap-3"
      data-slot="network-default-actions-container"
    >
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_network_automation_default_label()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_system_proxy_network_automation_default_description()}
        </ItemLabelDescription>
      </ItemLabel>

      <MechanismActionsFields
        value={policy?.default_actions ?? {}}
        onChange={handleChange}
      />
    </div>
  )
}
