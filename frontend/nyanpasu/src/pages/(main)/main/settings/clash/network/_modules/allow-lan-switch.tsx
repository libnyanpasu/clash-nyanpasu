import { useMemo } from 'react'
import { Switch } from '@nyanpasu/ui/switch'
import { RequireCapability } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashAccess, useClashConfig } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../../../_modules/settings-card'
import LanAddressesDialog from './lan-addresses-dialog'

export default function AllowLanSwitch() {
  const { query } = useClashConfig()
  const { upsert } = useClashAccess()

  const value = useMemo(() => query.data?.['allow-lan'], [query.data])

  const handleAllowLan = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({
        'allow-lan': input,
      })
    } catch (error) {
      message(`Activation Allow LAN failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <ItemContainer data-slot="allow-lan-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_allow_lan_label()}
        </ItemLabelText>
      </ItemLabel>

      <div
        className="flex items-center gap-3"
        data-slot="allow-lan-switch-actions"
      >
        <RequireCapability permission="remote:read">
          <LanAddressesDialog
            allowLan={Boolean(value)}
            liveConfig={query.data}
          />
        </RequireCapability>

        <Switch
          checked={Boolean(value)}
          onCheckedChange={handleAllowLan}
          loading={upsert.isPending}
        />
      </div>
    </ItemContainer>
  )
}
