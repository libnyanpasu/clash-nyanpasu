import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { filesize } from 'filesize'
import { useReducedMotion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import TextMarquee from '@nyanpasu/ui/text-marquee'
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
import { SubscriptionQuotaWave } from './subscription-quota-wave'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'
import {
  WidgetHeader,
  WidgetMeta,
  WidgetMetric,
  WidgetTitle,
} from './widget-ui'

function SubscriptionQuotaPreview({ id }: { id: string }) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionQuota}
      minW={3}
      minH={2}
    >
      <Card className="relative isolate flex size-full flex-col">
        <SubscriptionQuotaWave percent={68} waveStyle="double" animateWave />
        <WidgetHeader>
          <WidgetTitle icon={DataUsageRounded}>
            {m.dashboard_widget_subscription_quota_title()}
          </WidgetTitle>
        </WidgetHeader>
        <CardContent className="relative min-h-0 flex-1 justify-start gap-0 p-4 pt-2">
          <div className="flex min-h-0 flex-1 flex-col gap-1">
            <div className="flex min-h-0 flex-1 items-center">
              <WidgetMetric data-slot="subscription-quota-percent">
                68%
              </WidgetMetric>
            </div>
            <WidgetMeta data-slot="subscription-quota-summary">
              6.8 GiB{' '}
              {m.dashboard_widget_subscription_quota_remaining_of({
                total: '10 GiB',
              })}
            </WidgetMeta>
          </div>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function QuotaSummary({
  remaining,
  total,
  expiryText,
  notice,
  isError,
}: {
  remaining: string | null
  total: string | null
  expiryText: string | null
  notice: string | null
  isError: boolean
}) {
  const quotaText =
    remaining && total
      ? `${remaining} ${m.dashboard_widget_subscription_quota_remaining_of({ total })}`
      : null
  const texts = useMemo(
    () => [notice, quotaText, expiryText].filter((text) => text !== null),
    [notice, quotaText, expiryText],
  )
  const [index, setIndex] = useState(0)
  const ref = useRef<HTMLDivElement>(null)
  const reducedMotion = useReducedMotion()
  const text = texts[reducedMotion ? 0 : index % texts.length] ?? null
  const isNotice = notice !== null && text === notice

  useEffect(() => {
    setIndex(0)
  }, [texts])

  useEffect(() => {
    if (texts.length < 2 || reducedMotion) return

    const root = ref.current
    if (!root) return
    let interval: number
    const schedule = () => {
      window.clearInterval(interval)
      const marquee = Array.from(
        root.querySelectorAll('[data-slot="text-marquee"]'),
      ).at(-1)
      const content = marquee?.querySelector<HTMLElement>(
        '[data-slot="text-marquee-content-item"], [data-slot="text-marquee-content"]',
      )
      // Leave enough time for one complete scroll, including its initial pause.
      const duration = Math.max(
        8000,
        (((content?.scrollWidth ?? 0) + 32) / 30) * 1000 + 2000,
      )
      interval = window.setInterval(() => {
        if (!document.hidden)
          setIndex((current) => (current + 1) % texts.length)
      }, duration)
    }
    const observer = new ResizeObserver(schedule)
    observer.observe(root)
    schedule()

    return () => {
      observer.disconnect()
      window.clearInterval(interval)
    }
  }, [texts, reducedMotion, text])

  const content =
    text === quotaText ? (
      <>
        <span data-slot="subscription-quota-remaining">{remaining}</span>{' '}
        {total && m.dashboard_widget_subscription_quota_remaining_of({ total })}
      </>
    ) : (
      text
    )

  return (
    <WidgetMeta
      ref={ref}
      className="min-w-0 shrink-0 tabular-nums"
      data-slot="subscription-quota-summary"
      role={isNotice ? (isError ? 'alert' : 'status') : undefined}
      aria-label={isNotice ? notice : undefined}
      title={reducedMotion ? texts.join(' · ') : undefined}
    >
      {reducedMotion ? (
        <div className="truncate">{content}</div>
      ) : (
        <ActionSwap
          contentKey={text}
          className="min-w-0 grid-cols-[minmax(0,1fr)] [&>div]:min-w-0"
        >
          <TextMarquee className="w-full" speed={30}>
            {content}
          </TextMarquee>
        </ActionSwap>
      )}
    </WidgetMeta>
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
  const needsCheck = actionState === 'unconfirmed' && !checkedAfterUnconfirmed
  const summaryNotice = () => {
    if (targetMessage) return targetMessage
    if (checkFailed) return m.dashboard_widget_operation_check_failed()
    if (actionState === 'failed')
      return m.dashboard_widget_subscription_quota_refresh_failed()
    if (actionState === 'unconfirmed')
      return m.dashboard_widget_subscription_quota_unconfirmed()
    if (actionState === 'degraded')
      return m.dashboard_widget_subscription_quota_committed_degraded()
    if (query.isError && query.data)
      return m.dashboard_widget_subscription_quota_stale_error()
    if (quota && quota.overage > 0)
      return m.dashboard_widget_subscription_quota_overage({
        value: filesize(quota.overage, { standard: 'iec' }),
      })
    if (isWarning) return m.dashboard_widget_subscription_quota_warning()
    if (profile && !quota)
      return m.dashboard_widget_subscription_quota_unknown()
    return null
  }
  const noticeIsError =
    query.isError || checkFailed || actionState === 'failed' || isWarning
  const refreshLabel = needsCheck
    ? m.dashboard_widget_operation_check()
    : m.dashboard_widget_subscription_quota_refresh()

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionQuota}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card
        className="relative isolate flex size-full flex-col"
        data-slot="subscription-quota-card"
      >
        {quota && config.showProgress && !targetMessage && (
          <SubscriptionQuotaWave
            percent={quota.remainingPercent}
            waveStyle={config.waveStyle}
            animateWave={config.animateWave}
          />
        )}
        {profile && !targetMessage && (
          <Link
            className="hover:bg-on-surface/5 active:bg-on-surface/10 focus-visible:outline-primary absolute inset-0 z-10 rounded-3xl transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2"
            data-slot="subscription-quota-details"
            aria-label={`${profile.name}: ${m.dashboard_widget_subscription_quota_details()}`}
            aria-disabled={disabled}
            tabIndex={disabled ? -1 : 0}
            onClick={(event) => {
              if (disabled) event.preventDefault()
            }}
            to="/main/profiles/$type/detail/$uid"
            params={{ type: 'profile', uid: profile.uid }}
          />
        )}
        <WidgetHeader className="pointer-events-none relative flex-row items-center justify-between gap-2">
          <WidgetTitle
            className="flex-1"
            icon={DataUsageRounded}
            data-slot="subscription-quota-title"
          >
            {profile?.name ?? m.dashboard_widget_subscription_quota_configure()}
          </WidgetTitle>
          {profile && (
            <div className="pointer-events-auto relative z-20 flex shrink-0 items-center gap-1">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="raised"
                    className="size-7"
                    icon
                    aria-label={refreshLabel}
                    disabled={disabled || update.isPending || checkingStatus}
                    loading={update.isPending || checkingStatus}
                    onClick={needsCheck ? checkStatus : refresh}
                  >
                    <RefreshRounded className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{refreshLabel}</TooltipContent>
              </Tooltip>
            </div>
          )}
        </WidgetHeader>

        <CardContent className="pointer-events-none relative min-h-0 flex-1 justify-start gap-0 p-4 pt-2">
          <div
            className="flex min-h-0 flex-1 flex-col gap-1"
            data-slot="subscription-quota-content"
          >
            {targetMessage ? (
              <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-center text-sm">
                {(resolution?.kind === 'needs_selection' ||
                  resolution?.kind === 'missing' ||
                  resolution?.kind === 'unsupported') && (
                  <Button
                    variant="raised"
                    className="pointer-events-auto relative z-20 h-8 min-w-0 px-3 text-xs"
                    onClick={() => setIsEditing(true)}
                  >
                    {m.dashboard_widget_subscription_quota_configure()}
                  </Button>
                )}
              </div>
            ) : profile ? (
              <>
                {quota ? (
                  <div
                    className="flex min-h-0 flex-1 items-center"
                    data-slot="subscription-quota-metric"
                  >
                    <WidgetMetric
                      className="tabular-nums"
                      data-slot="subscription-quota-percent"
                    >
                      {Math.round(quota.remainingPercent)}%
                    </WidgetMetric>
                  </div>
                ) : (
                  <WidgetMetric className="flex min-h-0 flex-1 items-center">
                    &mdash;
                  </WidgetMetric>
                )}
              </>
            ) : (
              <div className="flex-1" />
            )}
            <QuotaSummary
              notice={summaryNotice()}
              isError={noticeIsError}
              remaining={
                quota ? filesize(quota.remaining, { standard: 'iec' }) : null
              }
              total={quota ? filesize(quota.total, { standard: 'iec' }) : null}
              expiryText={
                profile && config.showExpiry
                  ? expiry
                    ? m.dashboard_widget_subscription_quota_expires({
                        relative: formatRelativeTime(
                          expiry,
                          Date.now(),
                          getLocale(),
                        ),
                        date: formatDate(expiry),
                      })
                    : m.dashboard_widget_subscription_quota_expiry_unknown()
                  : null
              }
            />
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
