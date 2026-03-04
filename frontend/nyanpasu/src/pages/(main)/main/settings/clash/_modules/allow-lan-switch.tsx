import { useMemo } from 'react'
import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useClashConfig } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function AllowLanSwitch() {
  const { query, upsert } = useClashConfig()

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

      <Switch
        checked={Boolean(value)}
        onCheckedChange={handleAllowLan}
        loading={upsert.isPending}
      />
    </ItemContainer>
  )
}
