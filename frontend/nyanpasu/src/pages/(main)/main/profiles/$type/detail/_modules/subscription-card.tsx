import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RuleSettingsRounded from '~icons/material-symbols/rule-settings-rounded'
import dayjs from 'dayjs'
import { filesize } from 'filesize'
import { useMemo } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { LinearProgress } from '@/components/ui/progress'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  getRemoteSource,
  useProfile,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'
import UpdateOptionEditor from './update-option-editor'

const clampPercentage = (value: number) => Math.min(100, Math.max(0, value))

export const SubscriptionCard = ({
  profile,
}: {
  profile: ProfileItem_Serialize
}) => {
  const { update } = useProfile()

  const remote = getRemoteSource(profile)
  const updatedAt = remote?.updated_at ?? null
  const updateIntervalMinutes = remote?.option.update_interval_minutes
  const expire = remote?.subscription?.expire

  const { progress, total, used } = useMemo(() => {
    let progress = 0
    let total = 0
    let used = 0

    const sub = getRemoteSource(profile)?.subscription
    if (sub) {
      total = sub.total ?? 0

      used = (sub.download ?? 0) + (sub.upload ?? 0)

      if (total > 0) {
        progress = clampPercentage((used / total) * 100)
      }
    }

    return { progress, total, used }
  }, [profile])

  const blockTask = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    async () => {
      try {
        // Pure refresh: re-download without changing stored options.
        await update.mutateAsync({
          uid: profile.uid,
          option: null,
        })
      } catch (e) {
        message(`Update failed: \n ${formatError(e)}`, {
          title: 'Error',
          kind: 'error',
        })
      }
    },
  )

  const handleRefreshClick = useLockFn(async () => {
    await blockTask.execute()
  })

  return (
    <Card className="col-span-2">
      <CardHeader>{m.profile_subscription_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between">
          <div className="text-sm font-bold">{progress.toFixed(2)}%</div>

          <div className="text-sm font-bold">
            {filesize(used, { standard: 'iec' })} /
            {filesize(total, { standard: 'iec' })}
          </div>
        </div>

        <LinearProgress value={progress} />

        <div className="flex items-center justify-between gap-2 text-sm font-bold">
          <Tooltip>
            <TooltipTrigger>
              {m.profile_subscription_updated_at({
                updated: updatedAt ? dayjs(updatedAt * 1000).fromNow() : '-',
              })}
            </TooltipTrigger>

            {updatedAt && updateIntervalMinutes ? (
              <TooltipContent side="bottom">
                {m.profile_subscription_next_update_at({
                  next: dayjs(
                    updatedAt * 1000 + updateIntervalMinutes * 60 * 1000,
                  ).format('YYYY-MM-DD HH:mm:ss'),
                })}
              </TooltipContent>
            ) : null}
          </Tooltip>

          {expire ? (
            <span>
              {m.profile_subscription_expires_in({
                expires: dayjs(expire * 1000).fromNow(),
              })}
            </span>
          ) : null}
        </div>
      </CardContent>

      <CardFooter className="gap-1">
        <Button
          className="flex items-center gap-2"
          onClick={handleRefreshClick}
          loading={blockTask.isPending}
        >
          <RefreshRounded />
          <span>{m.profile_subscription_update()}</span>
        </Button>

        <UpdateOptionEditor profile={profile} asChild>
          <Button className="flex items-center gap-2">
            <RuleSettingsRounded />
            <span>{m.profile_update_option_edit()}</span>
          </Button>
        </UpdateOptionEditor>
      </CardFooter>
    </Card>
  )
}
