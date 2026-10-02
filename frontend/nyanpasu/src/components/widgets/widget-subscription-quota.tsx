import { filesize } from 'filesize'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { LinearProgress } from '@nyanpasu/ui/progress'
import TextMarquee from '@nyanpasu/ui/text-marquee'
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
        <CardHeader className="shrink-0 text-base font-medium">
          {m.dashboard_widget_subscription_quota_title()}
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
  const { displayItems } = useDndGridContext()
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const expanded = (displayItems.find((item) => item.id === id)?.h ?? 2) >= 3
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
        <CardHeader className="min-w-0 shrink-0 gap-2 px-3 pt-2 pb-1 text-base font-medium">
          <TextMarquee>
            {m.dashboard_widget_subscription_quota_title()}
          </TextMarquee>
          {profile && (
            <Link
              aria-disabled={disabled}
              tabIndex={disabled ? -1 : 0}
              onClick={(event) => {
                if (disabled) event.preventDefault()
              }}
              className={`text-on-surface-variant min-w-0 truncate text-xs ${disabled ? 'pointer-events-none opacity-50' : ''}`}
              to="/main/profiles/$type/detail/$uid"
              params={{ type: 'profile', uid: profile.uid }}
            >
              {profile.name}
            </Link>
          )}
        </CardHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-y-auto px-3 py-1">
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
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-xl font-bold tabular-nums">
                      <span data-slot="subscription-quota-remaining">
                        {filesize(quota.remaining, { standard: 'iec' })}
                      </span>
                    </span>
                    <span className="text-on-surface-variant text-[10px] tabular-nums">
                      {m.dashboard_widget_subscription_quota_remaining_of({
                        total: filesize(quota.total, { standard: 'iec' }),
                      })}
                    </span>
                  </div>
                  {config.showProgress && (
                    <div data-slot="subscription-quota-progress">
                      <LinearProgress value={quota.usedPercent} />
                    </div>
                  )}
                  {quota.overage > 0 && (
                    <p className="text-error text-xs">
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
                <p className="text-on-surface-variant truncate text-[10px]">
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
              {expanded && quota && (
                <p className="text-on-surface-variant truncate text-xs">
                  {m.dashboard_widget_subscription_quota_breakdown({
                    upload: filesize(quota.upload, { standard: 'iec' }),
                    download: filesize(quota.download, { standard: 'iec' }),
                  })}
                </p>
              )}
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
        </CardContent>

        {profile && expanded && (
          <CardFooter className="shrink-0 justify-between gap-2 px-3">
            <Button
              variant="basic"
              className="h-8 min-w-0 px-2 text-xs"
              disabled={disabled}
              asChild
            >
              <Link
                aria-disabled={disabled}
                tabIndex={disabled ? -1 : 0}
                onClick={(event) => {
                  if (disabled) event.preventDefault()
                }}
                className={disabled ? 'pointer-events-none opacity-50' : ''}
                to="/main/profiles/$type/detail/$uid"
                params={{ type: 'profile', uid: profile.uid }}
              >
                {m.dashboard_widget_subscription_quota_details()}
              </Link>
            </Button>
            <Button
              variant="raised"
              className="h-8 min-w-0 px-3 text-xs"
              disabled={
                disabled ||
                update.isPending ||
                checkingStatus ||
                (actionState === 'unconfirmed' && !checkedAfterUnconfirmed)
              }
              loading={update.isPending}
              onClick={refresh}
            >
              {m.dashboard_widget_subscription_quota_refresh()}
            </Button>
          </CardFooter>
        )}
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
