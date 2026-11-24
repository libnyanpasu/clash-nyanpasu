import ErrorOutlineRounded from '~icons/material-symbols/error-outline-rounded'
import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import SyncRounded from '~icons/material-symbols/sync-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { useState, type ReactNode } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import {
  MutationUnconfirmedError,
  useProfileListing,
  useProfileMutations,
  useProfileSyncRunsSummary,
  useProfileSyncStatusSummary,
} from '@nyanpasu/query'
import type { SyncRunSummary } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useQueryClient } from '@tanstack/react-query'
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
  WidgetTitle,
} from './core'
import {
  latestFinishedRuns,
  resolveProfileTarget,
} from './dashboard-daily-utils'
import { useWidgetHeight } from './use-widget-height'
import { DEFAULT_WIDGET_CONFIGS, WidgetId } from './widget-config'
import { ConfigToggle } from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'
import { SubscriptionTargetField } from './widget-reference-fields'

const RECENT_RUN_ROW_HEIGHT = 24

function SubscriptionSchedulePreview() {
  return (
    <Widget scaling="list" data-widget-kind="subscription-schedule">
      <WidgetHeader>
        <WidgetTitle />
      </WidgetHeader>
      <WidgetContent
        className="gap-2 pt-3 text-[length:var(--w-text-meta)]"
        data-slot="subscription-schedule-preview"
      >
        <div className="bg-surface-variant/30 flex min-h-0 flex-1 flex-col justify-center gap-1 overflow-hidden rounded-2xl px-4 py-3">
          <p className="shrink-0 truncate text-[length:var(--w-text-body)] font-medium">
            Japan Premium
          </p>
          <div className="text-on-surface-variant flex min-w-0 items-center gap-2">
            <SyncRounded className="size-4 shrink-0" aria-hidden="true" />
            <span>{m.dashboard_widget_subscription_schedule_succeeded()}</span>
          </div>
          <p className="text-on-surface-variant tabular-nums">
            {m.dashboard_widget_subscription_schedule_next_run({
              time: '21:30',
            })}
          </p>
        </div>
      </WidgetContent>
    </Widget>
  )
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function runStateLabel(run: SyncRunSummary): string {
  if (run.state.kind !== 'finished') {
    switch (run.state.kind) {
      case 'admitted':
        return m.dashboard_widget_subscription_schedule_admitted()
      case 'running':
        return m.dashboard_widget_subscription_schedule_running()
      case 'cancelling':
        return m.dashboard_widget_subscription_schedule_cancelling()
      case 'finalizing':
        return m.dashboard_widget_subscription_schedule_finalizing()
    }
  }

  switch (run.state.outcome) {
    case 'succeeded':
      return m.dashboard_widget_subscription_schedule_succeeded()
    case 'failed':
      return m.dashboard_widget_subscription_schedule_failed()
    case 'cancelled':
      return m.dashboard_widget_subscription_schedule_cancelled()
    case 'interrupted':
      return m.dashboard_widget_subscription_schedule_interrupted()
    case 'skipped':
      return m.dashboard_widget_subscription_schedule_skipped()
  }
}

function SubscriptionScheduleView({
  header,
  children,
}: {
  header?: ReactNode
  children: ReactNode
}) {
  return (
    <Widget
      scaling="list"
      data-widget-kind="subscription-schedule"
      data-slot="subscription-schedule-card"
    >
      <WidgetHeader className="flex-row items-center justify-between gap-2">
        <WidgetTitle className="flex-1" />

        {header}
      </WidgetHeader>

      <WidgetContent
        layout="flush"
        className="gap-0 pt-3 text-[length:var(--w-text-meta)]"
      >
        {children}
      </WidgetContent>
    </Widget>
  )
}

function SubscriptionScheduleTarget({
  profileUid,
  profileName,
}: {
  profileUid: string
  profileName: string
}) {
  const config = useWidgetConfig(WidgetId.SubscriptionSchedule)
  const interactive = useWidgetInteractive()
  const { span } = useWidgetContext()
  const status = useProfileSyncStatusSummary(profileUid)
  const runs = useProfileSyncRunsSummary(profileUid, {
    refetchInterval: 60_000,
  })
  const { update } = useProfileMutations()
  const { ref: contentRef, height: contentHeight } =
    useWidgetHeight<HTMLDivElement>()
  const queryClient = useQueryClient()
  const reducedMotion = useReducedMotion()
  const compact = span.h <= 2
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const activeRun = status.data?.active[0]
  const completed = latestFinishedRuns(
    runs.data?.pages.flatMap((page) => page.items) ?? [],
    3,
  )
  const unavailable = status.isError && !status.data
  const hasCriticalState =
    unavailable ||
    Boolean(status.data?.has_registration_error) ||
    status.data?.journal_degraded === true ||
    activeRun != null ||
    actionState !== 'idle' ||
    checkFailed
  const notice = checkFailed
    ? m.dashboard_widget_operation_check_failed()
    : actionState === 'failed'
      ? m.dashboard_widget_subscription_schedule_refresh_failed()
      : actionState === 'unconfirmed'
        ? m.dashboard_widget_subscription_schedule_unconfirmed()
        : actionState === 'degraded'
          ? m.dashboard_widget_subscription_schedule_committed_degraded()
          : unavailable
            ? m.dashboard_widget_subscription_schedule_load_failed()
            : status.data?.has_registration_error
              ? m.dashboard_widget_subscription_schedule_registration_failed()
              : status.data?.journal_degraded
                ? m.dashboard_widget_subscription_schedule_journal_degraded()
                : status.isPending
                  ? m.dashboard_widget_subscription_schedule_loading()
                  : null
  const noticeIsError =
    checkFailed ||
    actionState === 'failed' ||
    unavailable ||
    Boolean(status.data?.has_registration_error)
  const recentRunLimit =
    contentHeight == null || hasCriticalState
      ? 0
      : Math.max(
          0,
          Math.min(
            3,
            Math.floor(
              (contentHeight - (compact ? 72 : 120)) / RECENT_RUN_ROW_HEIGHT,
            ),
          ),
        )

  const refresh = async () => {
    if (
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
        uid: profileUid,
        option: null,
      })
      setActionState(
        outcome.status === 'committed_degraded' ? 'degraded' : 'idle',
      )
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ['profile-sync-runs-summary', profileUid],
        }),
        queryClient.invalidateQueries({
          queryKey: ['profile-sync-runs', profileUid],
        }),
      ])
    } catch (error) {
      setActionState(
        error instanceof MutationUnconfirmedError ? 'unconfirmed' : 'failed',
      )
      if (error instanceof MutationUnconfirmedError) {
        setCheckingStatus(true)
        try {
          await Promise.all([status.refetch(), runs.refetch()])
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
      const [statusResult, runsResult] = await Promise.all([
        status.refetch(),
        runs.refetch(),
      ])
      if (
        statusResult.isError ||
        statusResult.data == null ||
        runsResult.isError ||
        runsResult.data == null
      ) {
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

  return (
    <SubscriptionScheduleView
      header={
        <WidgetControls>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="basic"
                className="size-7 shrink-0"
                icon
                aria-label={m.dashboard_widget_subscription_schedule_refresh()}
                disabled={
                  !interactive ||
                  update.isPending ||
                  checkingStatus ||
                  (actionState === 'unconfirmed' && !checkedAfterUnconfirmed)
                }
                loading={update.isPending}
                onClick={refresh}
              >
                <RefreshRounded className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {m.dashboard_widget_subscription_schedule_refresh()}
            </TooltipContent>
          </Tooltip>
        </WidgetControls>
      }
    >
      <motion.div
        ref={contentRef}
        className={cn(
          'bg-surface-variant/30 flex min-h-0 flex-1 flex-col gap-1 overflow-hidden rounded-2xl px-4 py-3',
          compact && 'gap-0.5 px-3 py-1',
          (activeRun || notice) && 'justify-center',
        )}
        data-slot="subscription-schedule-content"
      >
        <motion.div
          layout="position"
          transition={{
            duration: reducedMotion ? 0 : 0.22,
            ease: 'easeOut',
          }}
          className="min-w-0 shrink-0"
        >
          <ActionSwap
            contentKey={
              notice ??
              activeRun?.state.kind ??
              status.data?.next_run_at ??
              'manual'
            }
            className="min-h-12 min-w-0 shrink-0 grid-cols-[minmax(0,1fr)] items-center [&>div]:min-w-0"
            data-slot="subscription-schedule-summary"
          >
            {notice ? (
              <div
                className={cn(
                  'flex min-w-0 items-center gap-3 text-[length:var(--w-text-body)]',
                  noticeIsError ? 'text-error' : 'text-on-surface-variant',
                )}
                role={noticeIsError ? 'alert' : 'status'}
                aria-label={notice}
              >
                <span className="bg-surface-variant/40 flex size-8 shrink-0 items-center justify-center rounded-xl">
                  {noticeIsError ? (
                    <ErrorOutlineRounded
                      className="size-5"
                      aria-hidden="true"
                    />
                  ) : (
                    <SyncRounded className="size-5" aria-hidden="true" />
                  )}
                </span>
                <TextMarquee className="w-full" speed={30}>
                  {notice}
                </TextMarquee>
              </div>
            ) : activeRun ? (
              <div
                className="text-on-surface-variant flex shrink-0 items-center gap-3 text-[length:var(--w-text-body)]"
                role="status"
                data-slot="subscription-schedule-active"
              >
                <span className="bg-surface-variant/40 flex size-8 shrink-0 items-center justify-center rounded-xl">
                  <motion.span
                    className="flex"
                    animate={{
                      rotate:
                        activeRun.state.kind === 'running' && !reducedMotion
                          ? 360
                          : 0,
                    }}
                    transition={
                      activeRun.state.kind === 'running' && !reducedMotion
                        ? {
                            duration: 1.8,
                            ease: 'linear',
                            repeat: Infinity,
                          }
                        : { duration: 0 }
                    }
                  >
                    <SyncRounded className="size-5" aria-hidden="true" />
                  </motion.span>
                </span>
                <span>{runStateLabel(activeRun)}</span>
              </div>
            ) : (
              status.data && (
                <>
                  {!activeRun &&
                    (status.data.scheduled && status.data.next_run_at ? (
                      <div
                        className="shrink-0"
                        data-slot="subscription-schedule-next-run"
                      >
                        <p className="text-on-surface-variant">
                          {m.dashboard_widget_subscription_schedule_next_run({
                            time: formatDate(
                              status.data.next_run_at,
                              'yyyy-MM-dd',
                            ),
                          })}
                        </p>
                        <time
                          dateTime={status.data.next_run_at}
                          className={cn(
                            'block text-[length:var(--w-text-metric)] tabular-nums',
                            compact && 'text-[length:var(--w-text-title)]',
                          )}
                        >
                          {formatDate(status.data.next_run_at, 'HH:mm')}
                        </time>
                      </div>
                    ) : (
                      <p>
                        {m.dashboard_widget_subscription_schedule_manual_only()}
                      </p>
                    ))}
                </>
              )
            )}
          </ActionSwap>
        </motion.div>
        <motion.div
          layout="position"
          className="order-first min-w-0 shrink-0"
          transition={{
            duration: reducedMotion ? 0 : 0.22,
            ease: 'easeOut',
          }}
        >
          <Link
            aria-disabled={!interactive}
            tabIndex={interactive ? 0 : -1}
            onClick={(event) => {
              if (!interactive) event.preventDefault()
            }}
            className={cn(
              'shrink-0 truncate text-[length:var(--w-text-body)] font-medium',
              'block',
              !interactive && 'pointer-events-none opacity-50',
            )}
            to="/main/profiles/$type/detail/$uid"
            params={{ type: 'profile', uid: profileUid }}
          >
            <TextMarquee className="w-full">{profileName}</TextMarquee>
          </Link>
        </motion.div>
        {config.showRecentRuns && recentRunLimit > 0 && (
          <div
            className={cn(
              'border-outline-variant mt-2 shrink-0 space-y-1 border-t pt-2',
              compact && 'mt-0.5 pt-0.5',
            )}
            data-slot="subscription-schedule-recent"
          >
            {runs.isPending ? (
              <p
                className="text-on-surface-variant text-[length:var(--w-text-meta)]"
                role="status"
              >
                {m.dashboard_widget_subscription_schedule_loading()}
              </p>
            ) : runs.isError && !runs.data ? (
              <p
                className="text-error text-[length:var(--w-text-meta)]"
                role="alert"
              >
                {m.dashboard_widget_subscription_schedule_history_unavailable()}
              </p>
            ) : completed.length > 0 ? (
              completed.slice(0, recentRunLimit).map((run, index) => (
                <p
                  key={run.id}
                  className="flex items-center justify-between gap-2 text-[length:var(--w-text-meta)]"
                  data-slot="subscription-schedule-run"
                >
                  <span
                    className={cn(
                      'truncate',
                      run.state.kind === 'finished' &&
                        run.state.outcome === 'failed' &&
                        'text-error',
                    )}
                  >
                    {index === 0 && (
                      <span className="text-on-surface-variant mr-1">
                        {m.dashboard_widget_subscription_schedule_recent()}
                      </span>
                    )}
                    {runStateLabel(run)}
                  </span>
                  <time
                    dateTime={run.finished_at ?? undefined}
                    className="text-on-surface-variant shrink-0 whitespace-nowrap tabular-nums"
                  >
                    {run.finished_at
                      ? formatDate(run.finished_at, 'MM-dd HH:mm')
                      : '—'}
                  </time>
                </p>
              ))
            ) : (
              <p className="text-on-surface-variant text-[length:var(--w-text-meta)]">
                {m.dashboard_widget_subscription_schedule_no_runs()}
              </p>
            )}
          </div>
        )}
        {actionState === 'unconfirmed' && (
          <Button
            variant="basic"
            className="h-7 min-w-0 self-start px-2 text-xs"
            disabled={!interactive || checkingStatus}
            loading={checkingStatus}
            onClick={checkStatus}
          >
            {m.dashboard_widget_operation_check()}
          </Button>
        )}
      </motion.div>
    </SubscriptionScheduleView>
  )
}

function SubscriptionScheduleLive() {
  const query = useProfileListing()
  const { enterEditMode } = useWidgetContext()
  const config = useWidgetConfig(WidgetId.SubscriptionSchedule)
  const profiles = query.data?.items ?? []
  const resolution = query.data
    ? resolveProfileTarget(profiles, query.data.current, config.target)
    : null
  const profile = resolution?.kind === 'selected' ? resolution.profile : null

  if (profile) {
    return (
      <SubscriptionScheduleTarget
        profileUid={profile.uid}
        profileName={profile.name}
      />
    )
  }

  const message = query.isPending
    ? m.dashboard_widget_subscription_schedule_loading()
    : query.isError && !query.data
      ? m.dashboard_widget_subscription_schedule_load_failed()
      : resolution?.kind === 'missing'
        ? m.dashboard_widget_subscription_schedule_target_missing()
        : resolution?.kind === 'unsupported'
          ? m.dashboard_widget_subscription_schedule_target_unsupported()
          : m.dashboard_widget_subscription_schedule_choose_target()

  return (
    <SubscriptionScheduleView>
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-center text-[length:var(--w-text-body)]">
        <p role={query.isError ? 'alert' : 'status'}>{message}</p>
        {(resolution?.kind === 'missing' ||
          resolution?.kind === 'unsupported' ||
          resolution?.kind === 'needs_selection') && (
          <Button
            variant="raised"
            className="h-8 min-w-0 px-3 text-xs"
            onClick={enterEditMode}
          >
            {m.dashboard_widget_subscription_schedule_configure()}
          </Button>
        )}
      </div>
    </SubscriptionScheduleView>
  )
}

function SubscriptionScheduleConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.SubscriptionSchedule,
  )

  return (
    <ConfigGroup>
      <SubscriptionTargetField
        target={config.target}
        disabled={disabled}
        onChange={(target) => updateConfig({ target })}
      />
      <ConfigToggle
        label={m.dashboard_widget_config_recent_runs()}
        checked={config.showRecentRuns}
        disabled={disabled}
        onChange={(showRecentRuns) => updateConfig({ showRecentRuns })}
      />
    </ConfigGroup>
  )
}

export const subscriptionScheduleWidget = defineWidget({
  id: WidgetId.SubscriptionSchedule,
  category: 'subscription',
  title: m.dashboard_widget_subscription_schedule_title,
  icon: EventRepeatRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 3 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionSchedule],
  Component: SubscriptionScheduleLive,
  Preview: SubscriptionSchedulePreview,
  ConfigForm: SubscriptionScheduleConfigForm,
})
