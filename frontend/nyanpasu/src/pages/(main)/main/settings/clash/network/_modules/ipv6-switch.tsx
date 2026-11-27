import { useMemo } from 'react'
import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashConfig } from '@nyanpasu/query'

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
        error,
      })
    }
  })

  return (
    <ItemSwitch
      data-slot="ipv6-switch-container"
      title={m.settings_clash_settings_ipv6_label()}
      checked={Boolean(value)}
      onCheckedChange={handleIPv6}
      loading={upsert.isPending}
    />
  )
}
