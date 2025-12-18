import { Card, CardContent } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import { useSystemProxy } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function CurrentSystemProxy() {
  const { data } = useSystemProxy()

  return (
    <SettingsCard data-slot="current-system-proxy-card">
      <SettingsCardContent
        data-slot="current-system-proxy-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <div className="px-1">
          {m.settings_system_proxy_current_system_proxy_label()}
        </div>

        <Card>
          <CardContent className="gap-1 select-text">
            {Object.entries(data ?? []).map(([key, value], index) => {
              return (
                <div key={index} className="flex w-full leading-8">
                  <div className="w-28 capitalize">{key}:</div>

                  <div className="text-warp flex-1 break-all">
                    {String(value)}
                  </div>
                </div>
              )
            })}
          </CardContent>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
