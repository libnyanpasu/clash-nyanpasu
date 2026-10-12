import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { filesize } from 'filesize'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { LinearProgress } from '@nyanpasu/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { useBlockTask } from '@/components/providers/block-task-provider'
import {
  calculateSubscriptionQuota,
  subscriptionExpiryTimestamp,
} from '@/components/widgets/dashboard-daily-utils'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatError } from '@/utils'
import { formatDate, formatRelativeTime } from '@/utils/date'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import {
  getRemoteSource,
  isConfigItem,
  useProfileMutations,
  useTrafficReport,
} from '@nyanpasu/query'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import ProfileCardAction from './profile-card-action'
import ProfileCardContentSummary from './profile-card-content-summary'

export function ProfileCardUpdate({
  profile,
  disabled,
}: {
  profile: ProfileItem_Serialize
  disabled: boolean
}) {
  const { update } = useProfileMutations()
  const task = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    async () => {
      try {
        await update.mutateAsync({ uid: profile.uid, option: null })
      } catch (error) {
        message(formatError(error), {
          title: m.profile_card_update_failed(),
          kind: 'error',
          error,
        })
      }
    },
  )
  const refresh = useLockFn(() => task.execute())

  return (
    <ProfileCardAction label={m.profile_subscription_update()}>
      <Button
        icon
        className="size-9 shrink-0 sm:size-11"
        disabled={disabled}
        loading={task.isPending}
        aria-label={m.profile_subscription_update()}
        onClick={refresh}
      >
        <RefreshRounded className="size-5" aria-hidden />
      </Button>
    </ProfileCardAction>
  )
}

export default function ProfileCardSubscription({
  profile,
  featured,
}: {
  profile: ProfileItem_Serialize
  featured: boolean
}) {
  const remote = getRemoteSource(profile)
  const quota = isConfigItem(profile)
    ? calculateSubscriptionQuota(remote?.subscription)
    : null
  const expiry = subscriptionExpiryTimestamp(remote?.subscription?.expire)
  const remainingLabel = m.profile_card_remaining()

  return (
    <div
      data-slot="profile-card-subscription"
      className="flex min-h-0 flex-1 flex-col justify-evenly gap-2"
    >
      {isConfigItem(profile) && (
        <ProfileCardMetric
          featured={featured}
          label={remainingLabel}
          value={
            quota ? filesize(quota.remaining, { standard: 'iec' }) : undefined
          }
          hint={
            quota
              ? m.profile_card_total({
                  total: filesize(quota.total, { standard: 'iec' }),
                })
              : m.profile_card_quota_unknown()
          }
          showHint={featured || !quota}
          percent={quota?.remainingPercent}
          percentLabel={remainingLabel}
        >
          {quota && quota.overage > 0 && (
            <p className="text-error mt-1 text-sm">
              {m.profile_card_overage({
                value: filesize(quota.overage, { standard: 'iec' }),
              })}
            </p>
          )}
        </ProfileCardMetric>
      )}

      <div
        className={cn(
          'text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs',
          !featured && 'hidden',
        )}
      >
        {remote?.updated_at ? (
          <span>
            {m.profile_subscription_updated_at({
              updated: formatRelativeTime(
                remote.updated_at * 1000,
                Date.now(),
                getLocale(),
              ),
            })}
          </span>
        ) : null}
        {expiry && (
          <span>
            {m.profile_subscription_expires_in({
              expires: formatDate(expiry, 'yyyy-MM-dd'),
            })}
          </span>
        )}
      </div>
    </div>
  )
}

function ProfileCardMetric({
  featured,
  label,
  value,
  hint,
  percent,
  percentLabel,
  children,
  showHint = true,
}: {
  featured: boolean
  label: string
  value?: string
  hint: string
  percent?: number
  percentLabel: string
  children?: ReactNode
  showHint?: boolean
}) {
  return (
    <>
      <p className="text-on-surface-variant text-sm">{label}</p>
      {value !== undefined && (
        <p
          data-slot="profile-card-remaining"
          className={cn(
            'font-medium tabular-nums',
            featured ? 'text-3xl sm:text-4xl' : 'text-2xl',
          )}
        >
          {value}
        </p>
      )}
      {showHint && hint && (
        <p className="text-on-surface-variant text-sm">{hint}</p>
      )}
      {children}
      {percent !== undefined && (
        <Tooltip>
          <TooltipTrigger asChild>
            <div data-slot="profile-card-quota" className="w-full">
              <LinearProgress
                className="h-1.5 motion-reduce:[&>div]:transition-none"
                value={percent}
                role="meter"
                aria-label={percentLabel}
              />
            </div>
          </TooltipTrigger>
          <TooltipContent>
            {percentLabel} {Math.round(percent)}%{hint ? ` · ${hint}` : ''}
          </TooltipContent>
        </Tooltip>
      )}
    </>
  )
}

export function ProfileCardTraffic({
  profile,
  featured,
}: {
  profile: ProfileItem_Serialize
  featured: boolean
}) {
  const report = useTrafficReport(
    {
      metric: 'bytes',
      query: {
        range: 'last24_hours',
        scope: 'all',
        filters: [{ dimension: 'profile', value: profile.uid }],
      },
      rankings: [],
      ranking_limit: 1,
      topology: null,
    },
    { refetchInterval: 10_000 },
  )
  const usage = report.data?.total
  const total = usage ? usage.bytes.upload + usage.bytes.download : undefined

  return (
    <div
      data-slot="profile-card-subscription"
      className="flex min-h-0 flex-1 flex-col justify-evenly gap-2"
    >
      <ProfileCardMetric
        featured={featured}
        label={m.profile_card_recorded_traffic()}
        value={
          total !== undefined && !report.isPlaceholderData
            ? filesize(total, { standard: 'iec' })
            : undefined
        }
        hint={
          report.isError
            ? m.profile_card_traffic_error()
            : !usage || report.isPlaceholderData
              ? m.profile_card_traffic_loading()
              : ''
        }
        percentLabel={m.profile_card_download_share()}
      />
      {featured &&
        usage &&
        (usage.bytes.upload > 0 || usage.bytes.download > 0) &&
        !report.isPlaceholderData && (
          <div className="text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs">
            <span>
              {m.dashboard_widget_traffic_report_download({
                value: filesize(usage.bytes.download, { standard: 'iec' }),
              })}
            </span>
            <span>
              {m.dashboard_widget_traffic_report_upload({
                value: filesize(usage.bytes.upload, { standard: 'iec' }),
              })}
            </span>
          </div>
        )}
      {isConfigItem(profile) && profile.config.type === 'file' && (
        <ProfileCardContentSummary uid={profile.uid} />
      )}
    </div>
  )
}
