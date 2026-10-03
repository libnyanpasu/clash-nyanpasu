import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function LocalIpProbeSwitch() {
  const localIpProbe = useSetting('enable_local_ip_probe')

  const handleChange = useLockFn(async () => {
    try {
      await localIpProbe.upsert(!localIpProbe.value)
    } catch (error) {
      message(
        `Update local IP probe setting failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="local-ip-probe-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="local-ip-probe-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_nyanpasu_local_ip_probe_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_nyanpasu_local_ip_probe_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            aria-label={m.settings_nyanpasu_local_ip_probe_label()}
            checked={Boolean(localIpProbe.value)}
            disabled={localIpProbe.value === undefined}
            onCheckedChange={handleChange}
            loading={localIpProbe.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
