import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import type { ReactNode } from 'react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { m } from '@/paraglide/messages'
import { PlatformLinux } from '@nyanpasu/platform/react'
import type { MechanismAction, MechanismActions } from '@nyanpasu/rpc/types'
import { OptionRow, OptionRowLabel } from './option-row'

const ACTIONS = [
  'keep',
  'enable',
  'disable',
] as const satisfies readonly MechanismAction[]

export const actionLabel = (action: MechanismAction) => {
  switch (action) {
    case 'keep':
      return m.settings_system_proxy_network_automation_action_keep()
    case 'enable':
      return m.settings_system_proxy_network_automation_action_enable()
    case 'disable':
      return m.settings_system_proxy_network_automation_action_disable()
  }
}

function ActionSelect({
  icon,
  label,
  value = 'keep',
  onChange,
}: {
  icon: ReactNode
  label: string
  value?: MechanismAction
  onChange: (action: MechanismAction) => void
}) {
  return (
    <OptionRow data-slot="mechanism-action-select">
      <OptionRowLabel icon={icon}>{label}</OptionRowLabel>

      <Select
        variant="chip"
        value={value}
        onValueChange={(action) => onChange(action as MechanismAction)}
      >
        <SelectTrigger aria-label={label}>
          <SelectValue>{actionLabel(value)}</SelectValue>
        </SelectTrigger>

        <SelectContent>
          {ACTIONS.map((action) => (
            <SelectItem key={action} value={action}>
              {actionLabel(action)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </OptionRow>
  )
}

/** One action per capture mechanism, for a rule or for the default. */
export default function MechanismActionsFields({
  value,
  onChange,
}: {
  value: MechanismActions
  onChange: (next: MechanismActions) => void
}) {
  return (
    <div className="flex flex-col gap-1" data-slot="mechanism-actions-fields">
      <ActionSelect
        icon={<NetworkPing aria-hidden />}
        label={m.settings_system_proxy_system_proxy_label()}
        value={value.system_proxy}
        onChange={(system_proxy) => onChange({ ...value, system_proxy })}
      />

      <ActionSelect
        icon={<SettingsEthernet aria-hidden />}
        label={m.settings_system_proxy_tun_mode_label()}
        value={value.tun}
        onChange={(tun) => onChange({ ...value, tun })}
      />

      <PlatformLinux>
        <ActionSelect
          icon={<ShieldRounded aria-hidden />}
          label={m.settings_clash_transparent_proxy_label()}
          value={value.transparent_proxy}
          onChange={(transparent_proxy) =>
            onChange({ ...value, transparent_proxy })
          }
        />
      </PlatformLinux>
    </div>
  )
}
