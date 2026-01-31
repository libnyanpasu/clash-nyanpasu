import { useMemo } from 'react'
import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { useClashConfig } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function IPv6Switch() {
  const { query, upsert } = useClashConfig()

  const value = useMemo(() => query.data?.['ipv6'], [query.data])

  const handleIPv6 = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({
        ipv6: input,
      })
    } catch (error) {
      message(`Activation IPv6 failed!`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <SettingsCard data-slot="ipv6-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="ipv6-card-content"
      >
        <div>{m.settings_clash_settings_ipv6_label()}</div>

        <Switch
          checked={Boolean(value)}
          onCheckedChange={handleIPv6}
          loading={upsert.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
