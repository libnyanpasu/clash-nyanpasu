import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { unwrapResult } from '@nyanpasu/rpc'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function DnsCacheButton() {
  const handleFlushDnsCache = useLockFn(async () => {
    try {
      unwrapResult(await rpc.flushSystemDnsCache())
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
    <SettingsCard data-slot="dns-cache-button-card">
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleFlushDnsCache}
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>
                {m.settings_system_proxy_dns_cache_label()}
              </ItemLabelText>

              <ItemLabelDescription>
                {m.settings_system_proxy_dns_cache_description()}
              </ItemLabelDescription>
            </ItemLabel>

            <div>
              <ArrowForwardIosRounded />
            </div>
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}
