import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { filesize } from 'filesize'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
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
    <Button
      variant="raised"
      className="flex shrink-0 items-center justify-center gap-2 whitespace-nowrap"
      disabled={disabled}
      loading={task.isPending}
      onClick={refresh}
    >
      <RefreshRounded className="size-4" />
      {m.profile_subscription_update()}
    </Button>
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
    <div data-slot="profile-card-subscription" className="flex flex-col gap-2">
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

      <div className="text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs">
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
  showPercentLabel = false,
}: {
  featured: boolean
  label: string
  value?: string
  hint: string
  percent?: number
  percentLabel: string
  children?: ReactNode
  showPercentLabel?: boolean
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0 flex-1">
        <p className="text-on-surface-variant mb-1 text-sm">{label}</p>
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
        <p className="text-on-surface-variant mt-1 text-sm">{hint}</p>
        {children}
      </div>
      {percent !== undefined && (
        <div className="flex shrink-0 flex-col items-center gap-1">
          <div
            data-slot="profile-card-quota"
            className={cn(
              'bg-surface/40 relative shrink-0 rounded-full',
              featured ? 'size-24 sm:size-28' : 'size-20',
            )}
            role="meter"
            aria-label={percentLabel}
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <CircularProgress
              className="text-primary size-full motion-reduce:[&_circle]:transition-none"
              value={percent}
              aria-hidden="true"
            />
            <span
              className={cn(
                'absolute inset-0 grid place-items-center font-medium tabular-nums',
                featured ? 'text-2xl' : 'text-lg',
              )}
            >
              {Math.round(percent)}%
            </span>
          </div>
          {showPercentLabel && (
            <span className="text-on-surface-variant text-xs">
              {percentLabel}
            </span>
          )}
        </div>
      )}
    </div>
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
  const percent =
    usage && total && total > 0
      ? (usage.bytes.download / total) * 100
      : undefined

  return (
    <div data-slot="profile-card-subscription" className="flex flex-col gap-2">
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
              : total === 0 && usage.connections === 0
                ? m.profile_card_traffic_empty()
                : m.topology_connection_count({ count: usage.connections })
        }
        percent={featured && !report.isPlaceholderData ? percent : undefined}
        percentLabel={m.profile_card_download_share()}
        showPercentLabel
      />
      {usage && !report.isPlaceholderData && (
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
    </div>
  )
}
