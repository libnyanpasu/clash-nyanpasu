import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import { useLockFn } from '@nyanpasu/hooks'
import { ClashRulesProviderQueryItem } from '@nyanpasu/query'
import { useRulesProviderUpdate } from '../../_modules/use-rules-provider-update'

export const InfoCard = ({ data }: { data: ClashRulesProviderQueryItem }) => {
  const blockTask = useRulesProviderUpdate(data)

  const handleRefreshClick = useLockFn(async () => {
    await blockTask.execute()
  })

  return (
    <Card className="col-span-2 flex flex-col justify-between">
      <CardHeader>{m.providers_info_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between px-1">
          <div className="text-secondary text-sm">
            {m.providers_rules_rule_count_label({
              count: data.ruleCount ?? 0,
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
            updated: formatRelativeTime(
              data.updatedAt,
              Date.now(),
              getLocale(),
            ),
          })}
        </div>
      </CardFooter>
    </Card>
  )
}
