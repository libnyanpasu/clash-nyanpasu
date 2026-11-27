import { ItemAction } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'

export default function DnsCacheButton() {
  const handleFlushDnsCache = useLockFn(async () => {
    try {
      await rpc.flushSystemDnsCache()
      await message(m.settings_system_proxy_dns_cache_success(), {
        kind: 'info',
      })
    } catch (error) {
      await message(
        `${m.settings_system_proxy_dns_cache_failed()}: ${formatError(error)}`,
        { kind: 'error', error },
      )
    }
  })

  return (
    <ItemAction
      data-slot="dns-cache-button-card"
      title={m.settings_system_proxy_dns_cache_label()}
      description={m.settings_system_proxy_dns_cache_description()}
      onClick={handleFlushDnsCache}
    />
  )
}
