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

export default function IPv6Switch() {
  const { query, upsert } = useClashConfig()

  const value = useMemo(() => query.data?.['ipv6'], [query.data])

  const handleIPv6 = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({
        ipv6: input,
      })
    } catch (error) {
      message(`Activation IPv6 failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <ItemContainer data-slot="ipv6-switch-container">
      <ItemLabel>
        <ItemLabelText>{m.settings_clash_settings_ipv6_label()}</ItemLabelText>
      </ItemLabel>

      <Switch
        checked={Boolean(value)}
        onCheckedChange={handleIPv6}
        loading={upsert.isPending}
      />
    </ItemContainer>
  )
}
