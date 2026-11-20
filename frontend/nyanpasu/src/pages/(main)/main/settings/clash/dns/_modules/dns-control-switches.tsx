import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSystemSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'

/** Whether Nyanpasu writes one table over the profiles. */
function ControlSwitch({
  field,
  label,
  description,
}: {
  field: 'dns_control' | 'hosts_control'
  label: string
  description: string
}) {
  const control = useClashSystemSetting(field)

  const handleChange = useLockFn(async (checked: boolean) => {
    try {
      await control.upsert(checked ? 'nyanpasu' : 'profile')
    } catch (error) {
      message(
        `${m.settings_clash_dns_control_failed()}\n${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="dns-control-switch-card">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>{label}</ItemLabelText>

            <ItemLabelDescription>{description}</ItemLabelDescription>
          </ItemLabel>

          <Switch
            aria-label={label}
            checked={control.value === 'nyanpasu'}
            loading={control.isPending}
            onCheckedChange={handleChange}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}

export default function DnsControlSwitches() {
  return (
    <div data-slot="dns-control-settings">
      <SettingsLabel>{m.settings_clash_dns_control_label()}</SettingsLabel>

      <SettingsGroup>
        <ControlSwitch
          field="dns_control"
          label={m.settings_clash_dns_control_dns_label()}
          description={m.settings_clash_dns_control_dns_description()}
        />

        <ControlSwitch
          field="hosts_control"
          label={m.settings_clash_dns_control_hosts_label()}
          description={m.settings_clash_dns_control_hosts_description()}
        />
      </SettingsGroup>
    </div>
  )
}
