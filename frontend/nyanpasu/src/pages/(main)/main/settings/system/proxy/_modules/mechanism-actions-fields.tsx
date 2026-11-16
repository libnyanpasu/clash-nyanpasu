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

const ACTIONS = [
  'keep',
  'enable',
  'disable',
] as const satisfies readonly MechanismAction[]

const actionLabel = (action: MechanismAction) => {
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
  label,
  value = 'keep',
  onChange,
}: {
  label: string
  value?: MechanismAction
  onChange: (action: MechanismAction) => void
}) {
  return (
    <div
      className="flex items-center justify-between gap-4"
      data-slot="mechanism-action-select"
    >
      <span className="text-sm">{label}</span>

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
    </div>
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
    <div className="flex flex-col gap-2" data-slot="mechanism-actions-fields">
      <ActionSelect
        label={m.settings_system_proxy_system_proxy_label()}
        value={value.system_proxy}
        onChange={(system_proxy) => onChange({ ...value, system_proxy })}
      />

      <ActionSelect
        label={m.settings_system_proxy_tun_mode_label()}
        value={value.tun}
        onChange={(tun) => onChange({ ...value, tun })}
      />

      <PlatformLinux>
        <ActionSelect
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
