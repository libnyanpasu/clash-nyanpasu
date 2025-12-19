import { useMemo } from 'react'
import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { useClashConfig } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function AllowLanSwitch() {
  const { query, upsert } = useClashConfig()

  const value = useMemo(() => query.data?.['allow-lan'], [query.data])

  const handleAllowLan = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({
        'allow-lan': input,
      })
    } catch (error) {
      message(`Activation Allow LAN failed!`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <SettingsCard data-slot="allow-lan-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="allow-lan-card-content"
      >
        <div>{m.settings_clash_settings_allow_lan_label()}</div>

        <Switch
          checked={Boolean(value)}
          onCheckedChange={handleAllowLan}
          loading={upsert.isPending}
        />
      </SettingsCardContent>
    </SettingsCard>
  )
}
