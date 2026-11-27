import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { useReducedMotion } from 'motion/react'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatDate, formatRelativeTime } from '@/utils/date'
import { formatBytes } from '@/utils/format-bytes'
import {
  MutationUnconfirmedError,
  useProfileListing,
  useProfileMutations,
} from '@nyanpasu/query'
import { Link } from '@tanstack/react-router'
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetContext,
  useWidgetInteractive,
  Widget,
  WidgetContent,
  WidgetControls,
  WidgetHeader,
  WidgetMeta,
  WidgetMetric,
  WidgetTitle,
} from './core'
import {
  calculateSubscriptionQuota,
  isExpiryWarning,
  isQuotaWarning,
  resolveProfileTarget,
  subscriptionExpiryTimestamp,
} from './dashboard-daily-utils'
import { SubscriptionQuotaWave } from './subscription-quota-wave'
import {
  DEFAULT_WIDGET_CONFIGS,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetId,
} from './widget-config'
import {
  ConfigChoice,
  ConfigStepper,
  ConfigToggle,
} from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'
import { SubscriptionTargetField } from './widget-reference-fields'

function SubscriptionQuotaView({
  chart,
  overlay,
  header,
  children,
}: {
  chart?: ReactNode
  overlay?: ReactNode
  header: ReactNode
  children: ReactNode
}) {
  return (
    <Widget
      scaling="proportional"
      data-widget-kind="subscription-quota"
      data-slot="subscription-quota-card"
    >
      {chart}

      {overlay}

      <WidgetHeader className="pointer-events-none relative flex-row items-center justify-between gap-2">
        {header}
      </WidgetHeader>

      <WidgetContent
        className="pointer-events-none relative justify-start gap-0"
        data-slot="subscription-quota-content"
      >
        {children}
      </WidgetContent>
    </Widget>
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

function SubscriptionQuotaPreview() {
  const config = useWidgetConfig(WidgetId.SubscriptionQuota)

  return (
    <SubscriptionQuotaView
      chart={
        config.showProgress ? (
          <SubscriptionQuotaWave
            percent={68}
            waveStyle="filled"
            animateWave={config.animateWave}
          />
        ) : undefined
      }
      header={<WidgetTitle />}
    >
      <div className="flex min-h-0 flex-1 flex-col gap-1">
        <div className="flex min-h-0 flex-1 items-center">
          <WidgetMetric data-slot="subscription-quota-percent">
            {config.showProgress ? '68%' : '—'}
          </WidgetMetric>
        </div>
        <WidgetMeta data-slot="subscription-quota-summary">
          {config.showProgress
            ? `${formatBytes(6.8 * 1024 ** 3)} ${m.dashboard_widget_subscription_quota_remaining_of(
                {
                  total: formatBytes(10 * 1024 ** 3),
                },
              )}`
            : m.dashboard_widget_subscription_quota_unknown()}
        </WidgetMeta>
      </div>
    </SubscriptionQuotaView>
  )
}

function SubscriptionQuotaLive() {
  const query = useProfileListing()
  const { update } = useProfileMutations()
  const { enterEditMode } = useWidgetContext()
  const interactive = useWidgetInteractive()
  const config = useWidgetConfig(WidgetId.SubscriptionQuota)
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
      !interactive ||
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
    if (!interactive || checkingStatus) return
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
        value: formatBytes(quota.overage),
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

  const header = (
    <>
      <WidgetTitle className="flex-1" data-slot="subscription-quota-title">
        {profile?.name ?? m.dashboard_widget_subscription_quota_configure()}
      </WidgetTitle>
      {profile && (
        <WidgetControls>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="raised"
                className="size-7"
                icon
                aria-label={refreshLabel}
                disabled={!interactive || update.isPending || checkingStatus}
                loading={update.isPending || checkingStatus}
                onClick={needsCheck ? checkStatus : refresh}
              >
                <RefreshRounded className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{refreshLabel}</TooltipContent>
          </Tooltip>
        </WidgetControls>
      )}
    </>
  )

  return (
    <SubscriptionQuotaView
      chart={
        quota && config.showProgress && !targetMessage ? (
          <SubscriptionQuotaWave
            percent={quota.remainingPercent}
            waveStyle={config.waveStyle}
            animateWave={config.animateWave}
          />
        ) : undefined
      }
      overlay={
        profile &&
        !targetMessage && (
          <Link
            className="hover:bg-on-surface/5 active:bg-on-surface/10 focus-visible:outline-primary rounded-extra-large absolute inset-0 z-10 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2"
            data-slot="subscription-quota-details"
            aria-label={`${profile.name}: ${m.dashboard_widget_subscription_quota_details()}`}
            aria-disabled={!interactive}
            tabIndex={interactive ? 0 : -1}
            onClick={(event) => {
              if (!interactive) event.preventDefault()
            }}
            to="/main/profiles/$type/detail/$uid"
            params={{ type: 'profile', uid: profile.uid }}
          />
        )
      }
      header={header}
    >
      <div className="flex min-h-0 flex-1 flex-col gap-1">
        {targetMessage ? (
          <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-center text-[length:var(--w-text-body)]">
            {(resolution?.kind === 'needs_selection' ||
              resolution?.kind === 'missing' ||
              resolution?.kind === 'unsupported') && (
              <Button
                variant="raised"
                className="pointer-events-auto relative z-20 h-8 min-w-0 px-3 text-xs"
                onClick={enterEditMode}
              >
                {m.dashboard_widget_subscription_quota_configure()}
              </Button>
            )}
          </div>
        ) : profile ? (
          quota ? (
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
          )
        ) : (
          <div className="flex-1" />
        )}

        <QuotaSummary
          notice={summaryNotice()}
          isError={noticeIsError}
          remaining={quota ? formatBytes(quota.remaining) : null}
          total={quota ? formatBytes(quota.total) : null}
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
    </SubscriptionQuotaView>
  )
}

function SubscriptionQuotaConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.SubscriptionQuota,
  )

  return (
    <>
      <ConfigGroup>
        <SubscriptionTargetField
          target={config.target}
          disabled={disabled}
          onChange={(target) => updateConfig({ target })}
        />
      </ConfigGroup>

      <ConfigGroup>
        <ConfigToggle
          label={m.dashboard_widget_config_expiry()}
          checked={config.showExpiry}
          disabled={disabled}
          onChange={(showExpiry) => updateConfig({ showExpiry })}
        />
        <ConfigToggle
          label={m.dashboard_widget_config_progress()}
          checked={config.showProgress}
          disabled={disabled}
          onChange={(showProgress) => updateConfig({ showProgress })}
        />
        <ConfigChoice
          label={m.dashboard_widget_subscription_quota_config_wave_style()}
          value={config.waveStyle}
          options={[
            {
              value: 'filled',
              label: m.dashboard_widget_subscription_quota_config_wave_filled(),
            },
            {
              value: 'outline',
              label:
                m.dashboard_widget_subscription_quota_config_wave_outline(),
            },
          ]}
          disabled={disabled || !config.showProgress}
          onChange={(waveStyle) => updateConfig({ waveStyle })}
        />
        <ConfigToggle
          label={m.dashboard_widget_subscription_quota_config_wave_animation()}
          checked={config.animateWave}
          disabled={disabled || !config.showProgress}
          onChange={(animateWave) => updateConfig({ animateWave })}
        />
      </ConfigGroup>

      <ConfigGroup>
        <ConfigStepper
          label={m.dashboard_widget_config_expiry_threshold()}
          value={config.expiryWarningDays}
          range={WIDGET_CONFIG_NUMBER_RANGES.expiryWarningDays}
          disabled={disabled}
          onChange={(expiryWarningDays) => updateConfig({ expiryWarningDays })}
        />
        <ConfigStepper
          label={m.dashboard_widget_config_quota_threshold()}
          value={config.quotaWarningPercent}
          range={WIDGET_CONFIG_NUMBER_RANGES.quotaWarningPercent}
          disabled={disabled}
          onChange={(quotaWarningPercent) =>
            updateConfig({ quotaWarningPercent })
          }
        />
      </ConfigGroup>
    </>
  )
}

export const subscriptionQuotaWidget = defineWidget({
  id: WidgetId.SubscriptionQuota,
  category: 'subscription',
  title: m.dashboard_widget_subscription_quota_title,
  description: m.dashboard_widget_subscription_quota_description,
  icon: DataUsageRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 3 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
  Component: SubscriptionQuotaLive,
  Preview: SubscriptionQuotaPreview,
  ConfigForm: SubscriptionQuotaConfigForm,
})
