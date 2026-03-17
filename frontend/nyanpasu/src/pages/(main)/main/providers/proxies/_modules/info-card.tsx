import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import dayjs from 'dayjs'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { ClashProxiesProviderQueryItem } from '@nyanpasu/interface'
import { useProxiesProviderUpdate } from '../../_modules/use-proxies-provider-update'

export const InfoCard = ({ data }: { data: ClashProxiesProviderQueryItem }) => {
  const blockTask = useProxiesProviderUpdate(data)

  const handleRefreshClick = useLockFn(async () => {
    await blockTask.execute()
  })

  return (
    <Card className="col-span-2 flex flex-col justify-between">
      <CardHeader>{m.providers_info_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between px-1">
          <div className="text-secondary text-sm">
            {m.providers_proxies_proxy_count_label({
              count: data.proxies.length,
            })}
          </div>

          <div className="text-sm text-zinc-500">
            {data.vehicleType}/{data.type}
          </div>
        </div>
      </CardContent>

      <CardFooter>
        <Button
          className="flex items-center gap-2"
          onClick={handleRefreshClick}
          loading={blockTask.isPending}
        >
          <RefreshRounded />
          <span>{m.providers_update_provider()}</span>
        </Button>

        <div className="flex-1" />

        <div className="hover:bg-surface-variant text-secondary rounded-full px-3 py-2 text-xs font-semibold">
          {m.profile_subscription_updated_at({
            updated: dayjs(data.updatedAt).fromNow(),
          })}
        </div>
      </CardFooter>
    </Card>
  )
}
