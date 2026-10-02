import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { filesize } from 'filesize'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { LinearProgress } from '@nyanpasu/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatDate, formatRelativeTime } from '@/utils/date'
import {
  MutationUnconfirmedError,
  useProfile,
  useProfileMutations,
} from '@nyanpasu/query'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import {
  calculateSubscriptionQuota,
  isExpiryWarning,
  isQuotaWarning,
  resolveProfileTarget,
  subscriptionExpiryTimestamp,
} from './dashboard-daily-utils'
import { useDashboardContext, useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'

function SubscriptionQuotaPreview({ id }: { id: string }) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionQuota}
      minW={3}
      minH={2}
    >
      <Card className="flex size-full flex-col">
        <CardHeader className="shrink-0 gap-2 px-3 pt-2 pb-1 text-sm font-medium">
          <DataUsageRounded className="text-on-surface-variant size-5 shrink-0" />
          <span
            className="min-w-0 flex-1 truncate"
            title={m.dashboard_widget_subscription_quota_title()}
          >
            {m.dashboard_widget_subscription_quota_title()}
          </span>
        </CardHeader>
        <CardContent className="min-h-0 flex-1 justify-center">
          <div className="bg-surface-variant h-5 w-2/3 animate-pulse rounded-full" />
          <div className="bg-surface-variant h-2 w-full animate-pulse rounded-full" />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function SubscriptionQuotaLive({
  id,
  onCloseClick,
  disabled,
  config,
}: WidgetComponentProps & {
  disabled: boolean
  config: Extract<WidgetConfig, { type: WidgetId.SubscriptionQuota }>
}) {
  const { query } = useProfile()
  const { update } = useProfileMutations()
  const { setIsEditing } = useDashboardContext()
  const { ref: contentRef, height: contentHeight } =
    useWidgetHeight<HTMLDivElement>()
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const profiles = query.data?.items ?? []
  const resolution = query.data
    ? resolveProfileTarget(profiles, query.data.current, config.target)
    : null
  const profile = resolution?.kind === 'selected' ? resolution.profile : null
  const subscription =
    profile?.type === 'config' && profile.config.type === 'file'
      ? profile.config.source.type === 'remote'
        ? profile.config.source.subscription
        : undefined
      : undefined
  const quota = calculateSubscriptionQuota(subscription)
  const expiry = subscriptionExpiryTimestamp(subscription?.expire)
  const updatedAt =
    profile?.type === 'config' && profile.config.type === 'file'
      ? profile.config.source.type === 'remote'
        ? profile.config.source.updated_at
        : undefined
      : undefined
  const isWarning =
    isQuotaWarning(quota, config.quotaWarningPercent) ||
    isExpiryWarning(expiry, config.expiryWarningDays, Date.now())

  const refresh = async () => {
    if (
      !profile ||
      disabled ||
      update.isPending ||
      checkingStatus ||
      (actionState === 'unconfirmed' && !checkedAfterUnconfirmed)
    )
      return
    setActionState('idle')
    setCheckedAfterUnconfirmed(false)
    setCheckFailed(false)
    try {
      const outcome = await update.mutateAsync({
        uid: profile.uid,
        option: null,
      })
      setActionState(
        outcome.status === 'committed_degraded' ? 'degraded' : 'idle',
      )
    } catch (error) {
      setActionState(
        error instanceof MutationUnconfirmedError ? 'unconfirmed' : 'failed',
      )
      if (error instanceof MutationUnconfirmedError) {
        setCheckingStatus(true)
        try {
          await query.refetch()
        } catch {
          setCheckFailed(false)
        } finally {
          setCheckingStatus(false)
        }
      }
    }
  }

  const checkStatus = async () => {
    if (disabled || checkingStatus) return
    setCheckingStatus(true)
    setCheckFailed(false)
    try {
      const result = await query.refetch()
      if (result.isError || result.data == null) {
        setCheckFailed(true)
        setCheckedAfterUnconfirmed(false)
      } else {
        setCheckedAfterUnconfirmed(true)
      }
    } catch {
      setCheckFailed(true)
      setCheckedAfterUnconfirmed(false)
    } finally {
      setCheckingStatus(false)
    }
  }

  const statusText = () => {
    if (query.isPending) return m.dashboard_widget_subscription_quota_loading()
    if (query.isError && !query.data)
      return m.dashboard_widget_subscription_quota_load_failed()
    if (resolution?.kind === 'needs_selection')
      return m.dashboard_widget_subscription_quota_choose_target()
    if (resolution?.kind === 'missing')
      return m.dashboard_widget_subscription_quota_target_missing()
    if (resolution?.kind === 'unsupported')
      return m.dashboard_widget_subscription_quota_target_unsupported()
    return null
  }

  const targetMessage = statusText()
  const hasPriorityState =
    isWarning ||
    (query.isError && query.data != null) ||
    actionState !== 'idle' ||
    checkFailed
  const showTotal = !hasPriorityState && (contentHeight ?? 0) >= 88
  const showBreakdown = !hasPriorityState && (contentHeight ?? 0) >= 144
  const showUpdatedAt = !hasPriorityState && (contentHeight ?? 0) >= 112

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionQuota}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card
        className="flex size-full flex-col"
        data-slot="subscription-quota-card"
      >
        <CardHeader className="shrink-0 flex-row items-center justify-between gap-2 px-3 pt-2 pb-1 text-sm font-medium">
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <DataUsageRounded className="text-on-surface-variant size-5 shrink-0" />
            <span
              className="min-w-0 flex-1 truncate"
              title={m.dashboard_widget_subscription_quota_title()}
            >
              {m.dashboard_widget_subscription_quota_title()}
            </span>
          </div>
          {profile && (
            <div className="flex shrink-0 items-center gap-1">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="basic"
                    className="size-7"
                    icon
                    aria-label={m.dashboard_widget_subscription_quota_details()}
                    asChild
                  >
                    <Link
                      aria-disabled={disabled}
                      tabIndex={disabled ? -1 : 0}
                      onClick={(event) => {
                        if (disabled) event.preventDefault()
                      }}
                      to="/main/profiles/$type/detail/$uid"
                      params={{ type: 'profile', uid: profile.uid }}
                    >
                      <OpenInNewRounded className="size-4" />
                    </Link>
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {m.dashboard_widget_subscription_quota_details()}
                </TooltipContent>
              </Tooltip>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="raised"
                    className="size-7"
                    icon
                    aria-label={m.dashboard_widget_subscription_quota_refresh()}
                    disabled={
                      disabled ||
                      update.isPending ||
                      checkingStatus ||
                      (actionState === 'unconfirmed' &&
                        !checkedAfterUnconfirmed)
                    }
                    loading={update.isPending}
                    onClick={refresh}
                  >
                    <RefreshRounded className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {m.dashboard_widget_subscription_quota_refresh()}
                </TooltipContent>
              </Tooltip>
            </div>
          )}
        </CardHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-0 px-3 py-1">
          <div
            ref={contentRef}
            className="flex min-h-0 flex-1 flex-col gap-1"
            data-slot="subscription-quota-content"
          >
            {targetMessage ? (
              <div className="flex flex-col items-center gap-2 text-center text-sm">
                <p role={query.isError ? 'alert' : 'status'}>{targetMessage}</p>
                {(resolution?.kind === 'needs_selection' ||
                  resolution?.kind === 'missing' ||
                  resolution?.kind === 'unsupported') && (
                  <Button
                    variant="raised"
                    className="h-8 min-w-0 px-3 text-xs"
                    onClick={() => setIsEditing(true)}
                  >
                    {m.dashboard_widget_subscription_quota_configure()}
                  </Button>
                )}
              </div>
            ) : profile ? (
              <>
                {!hasPriorityState && (
                  <Link
                    aria-disabled={disabled}
                    tabIndex={disabled ? -1 : 0}
                    onClick={(event) => {
                      if (disabled) event.preventDefault()
                    }}
                    className={`text-on-surface-variant truncate text-xs ${disabled ? 'pointer-events-none opacity-50' : ''}`}
                    to="/main/profiles/$type/detail/$uid"
                    params={{ type: 'profile', uid: profile.uid }}
                  >
                    {profile.name}
                  </Link>
                )}
                {isWarning && (
                  <p
                    className="text-error text-xs"
                    role="status"
                    data-slot="subscription-quota-warning"
                  >
                    {m.dashboard_widget_subscription_quota_warning()}
                  </p>
                )}
                {quota ? (
                  <>
                    <div className="flex flex-col gap-0">
                      <span className="text-xl font-bold whitespace-nowrap tabular-nums">
                        <span data-slot="subscription-quota-remaining">
                          {filesize(quota.remaining, { standard: 'iec' })}
                        </span>
                      </span>
                      {showTotal && (
                        <span className="text-on-surface-variant text-[10px] tabular-nums">
                          {m.dashboard_widget_subscription_quota_remaining_of({
                            total: filesize(quota.total, { standard: 'iec' }),
                          })}
                        </span>
                      )}
                    </div>
                    {config.showProgress && (
                      <div data-slot="subscription-quota-progress">
                        <LinearProgress value={quota.usedPercent} />
                      </div>
                    )}
                    {quota.overage > 0 && (
                      <p className="text-error text-xs break-words">
                        {m.dashboard_widget_subscription_quota_overage({
                          value: filesize(quota.overage, { standard: 'iec' }),
                        })}
                      </p>
                    )}
                  </>
                ) : (
                  <p className="text-on-surface-variant text-sm" role="status">
                    {m.dashboard_widget_subscription_quota_unknown()}
                  </p>
                )}
                {config.showExpiry && (
                  <p className="text-on-surface-variant text-[10px] break-words">
                    {expiry
                      ? m.dashboard_widget_subscription_quota_expires({
                          relative: formatRelativeTime(
                            expiry,
                            Date.now(),
                            getLocale(),
                          ),
                          date: formatDate(expiry),
                        })
                      : m.dashboard_widget_subscription_quota_expiry_unknown()}
                  </p>
                )}
                {showBreakdown && quota && (
                  <p className="text-on-surface-variant truncate text-xs">
                    {m.dashboard_widget_subscription_quota_breakdown({
                      upload: filesize(quota.upload, { standard: 'iec' }),
                      download: filesize(quota.download, { standard: 'iec' }),
                    })}
                  </p>
                )}
                {showUpdatedAt && (
                  <p className="text-on-surface-variant truncate text-[10px]">
                    {updatedAt
                      ? m.dashboard_widget_subscription_quota_updated({
                          time: formatRelativeTime(
                            updatedAt * 1000,
                            Date.now(),
                            getLocale(),
                          ),
                        })
                      : m.dashboard_widget_subscription_quota_updated_unknown()}
                  </p>
                )}
                {query.isError && query.data && (
                  <p className="text-error text-xs" role="alert">
                    {m.dashboard_widget_subscription_quota_stale_error()}
                  </p>
                )}
                {actionState !== 'idle' && (
                  <p
                    className={
                      actionState === 'failed'
                        ? 'text-error text-xs'
                        : 'text-on-surface-variant text-xs'
                    }
                    role={actionState === 'failed' ? 'alert' : 'status'}
                  >
                    {actionState === 'degraded'
                      ? m.dashboard_widget_subscription_quota_committed_degraded()
                      : actionState === 'unconfirmed'
                        ? m.dashboard_widget_subscription_quota_unconfirmed()
                        : m.dashboard_widget_subscription_quota_refresh_failed()}
                  </p>
                )}
                {actionState === 'unconfirmed' && (
                  <>
                    <Button
                      variant="basic"
                      className="h-7 min-w-0 px-2 text-xs"
                      disabled={disabled || checkingStatus}
                      loading={checkingStatus}
                      onClick={checkStatus}
                    >
                      {m.dashboard_widget_operation_check()}
                    </Button>
                    {checkFailed && (
                      <p className="text-error text-xs" role="alert">
                        {m.dashboard_widget_operation_check_failed()}
                      </p>
                    )}
                  </>
                )}
              </>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

export function SubscriptionQuotaWidget(props: WidgetComponentProps) {
  const { sourceOnly, isOverlay, disabled } = useDndGridContext()
  const config = useWidgetConfig(props.id, WidgetId.SubscriptionQuota)

  if (sourceOnly || isOverlay) return <SubscriptionQuotaPreview id={props.id} />

  return (
    <SubscriptionQuotaLive {...props} disabled={!disabled} config={config} />
  )
}
