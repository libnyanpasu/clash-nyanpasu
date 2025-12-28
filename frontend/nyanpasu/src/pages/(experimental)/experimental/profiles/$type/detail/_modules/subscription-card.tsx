import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RuleSettingsRounded from '~icons/material-symbols/rule-settings-rounded'
import dayjs from 'dayjs'
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
import parseTraffic from '@/utils/parse-traffic'
import {
  RemoteProfile,
  RemoteProfileOptionsBuilder,
  useProfile,
} from '@nyanpasu/interface'
import UpdateOptionEditor from './update-option-editor'

export const SubscriptionCard = ({ profile }: { profile: RemoteProfile }) => {
  const { update } = useProfile()

  const { progress, total, used } = useMemo(() => {
    let progress = 0
    let total = 0
    let used = 0

    if (
      profile !== undefined &&
      'extra' in profile &&
      profile.extra !== undefined
    ) {
      const { download, upload, total: t } = profile.extra

      total = t

      used = download + upload

      progress = (used / (total || 1)) * 100
    }

    return { progress, total, used }
  }, [profile])

  const blockTask = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    async () => {
      // TODO: define backend serde(option) to move null
      const selfOption = 'option' in profile ? profile.option : undefined

      const options: RemoteProfileOptionsBuilder = {
        with_proxy: false,
        self_proxy: false,
        update_interval: 0,
        user_agent: null,
        ...selfOption,
      }

      // if (proxy) {
      //   if (selfOption?.self_proxy) {
      //     options.with_proxy = false
      //     options.self_proxy = true
      //   } else {
      //     options.with_proxy = true
      //     options.self_proxy = false
      //   }
      // }

      try {
        await update.mutateAsync({
          uid: profile.uid,
          option: options,
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
    <Card className="col-span-2 break-inside-avoid">
      <CardHeader>{m.profile_subscription_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between">
          <div className="text-sm font-bold">{progress.toFixed(2)}%</div>

          <div className="text-sm font-bold">
            {parseTraffic(used)} / {parseTraffic(total)}
          </div>
        </div>

        <LinearProgress value={progress} />

        <div className="flex items-center justify-between gap-2 text-sm font-bold">
          <Tooltip>
            <TooltipTrigger>
              {m.profile_subscription_updated_at({
                updated: dayjs(profile.updated * 1000).fromNow(),
              })}
            </TooltipTrigger>

            {profile.option?.update_interval && (
              <TooltipContent side="bottom">
                {m.profile_subscription_next_update_at({
                  next: dayjs(
                    profile.updated * 1000 +
                      profile.option.update_interval * 1000 * 60,
                  ).format('YYYY-MM-DD HH:mm:ss'),
                })}
              </TooltipContent>
            )}
          </Tooltip>

          {profile.extra?.expire && (
            <span>
              {m.profile_subscription_expires_in({
                expires: dayjs(profile.extra?.expire * 1000).fromNow(),
              })}
            </span>
          )}
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
