import { filesize } from 'filesize'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { LinearProgress } from '@/components/ui/progress'
import { m } from '@/paraglide/messages'
import { ClashProxiesProviderQueryItem } from '@nyanpasu/interface'
import { useProxiesSubscription } from '../../_modules/use-proxies-subscription'

export const SubscriptionCard = ({
  data,
}: {
  data: ClashProxiesProviderQueryItem
}) => {
  const { progress, total, used, hasSubscriptionInfo } =
    useProxiesSubscription(data)

  if (!hasSubscriptionInfo) {
    return null
  }

  return (
    <Card className="col-span-2 flex flex-col justify-between">
      <CardHeader>{m.providers_subscription_title()}</CardHeader>

      <CardContent>
        <LinearProgress value={progress} />

        <div className="flex items-center justify-between pb-2">
          <div className="text-sm font-bold">{progress.toFixed(2)}%</div>

          <div className="text-sm font-bold">
            {filesize(used)} / {filesize(total)}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
