import ErrorOutlineRounded from '~icons/material-symbols/error-outline-rounded'
import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import SyncRounded from '~icons/material-symbols/sync-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { useState } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import {
  MutationUnconfirmedError,
  useProfile,
  useProfileMutations,
  useProfileSyncRuns,
  useProfileSyncStatus,
} from '@nyanpasu/query'
import type { RunDto } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import {
  latestFinishedRuns,
  resolveProfileTarget,
} from './dashboard-daily-utils'
import { useDashboardContext, useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetHeader, WidgetTitle } from './widget-ui'

const RECENT_RUN_ROW_HEIGHT = 24

function SubscriptionSchedulePreview({ id }: { id: string }) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionSchedule}
      minW={3}
      minH={2}
    >
      <Card className="flex size-full flex-col">
        <WidgetHeader>
          <WidgetTitle icon={EventRepeatRounded}>
            {m.dashboard_widget_subscription_schedule_title()}
          </WidgetTitle>
        </WidgetHeader>
        <CardContent className="min-h-0 flex-1 justify-center gap-2 overflow-hidden">
          <div className="bg-surface-variant h-4 w-2/3 animate-pulse rounded-full" />
          <div className="bg-surface-variant h-4 w-1/2 animate-pulse rounded-full" />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function runStateLabel(run: RunDto): string {
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

  switch (run.state.completion.outcome.kind) {
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

function SubscriptionScheduleTarget({
  id,
  onCloseClick,
  disabled,
  config,
  profileUid,
  profileName,
}: WidgetComponentProps & {
  disabled: boolean
  config: Extract<WidgetConfig, { type: WidgetId.SubscriptionSchedule }>
  profileUid: string
  profileName: string
}) {
  const status = useProfileSyncStatus(profileUid)
  const runs = useProfileSyncRuns(profileUid, { refetchInterval: 60_000 })
  const { update } = useProfileMutations()
  const { ref: contentRef, height: contentHeight } =
    useWidgetHeight<HTMLDivElement>()
  const queryClient = useQueryClient()
  const reducedMotion = useReducedMotion()
  const { displayItems } = useDndGridContext()
  const compact = displayItems.find((item) => item.id === id)?.h === 2
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
    Boolean(status.data?.registration_error) ||
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
            : status.data?.registration_error
              ? m.dashboard_widget_subscription_schedule_registration_error({
                  error: status.data.registration_error,
                })
              : status.data?.journal_degraded
                ? m.dashboard_widget_subscription_schedule_journal_degraded()
                : status.isPending
                  ? m.dashboard_widget_subscription_schedule_loading()
                  : null
  const noticeIsError =
    checkFailed ||
    actionState === 'failed' ||
    unavailable ||
    Boolean(status.data?.registration_error)
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
        uid: profileUid,
        option: null,
      })
      setActionState(
        outcome.status === 'committed_degraded' ? 'degraded' : 'idle',
      )
      await queryClient.invalidateQueries({
        queryKey: ['profile-sync-runs', profileUid],
      })
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
    if (disabled || checkingStatus) return
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
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionSchedule}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card
        className="flex size-full flex-col"
        data-slot="subscription-schedule-card"
      >
        <WidgetHeader className="flex-row items-center justify-between gap-2">
          <WidgetTitle className="flex-1" icon={EventRepeatRounded}>
            {m.dashboard_widget_subscription_schedule_title()}
          </WidgetTitle>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="basic"
                className="size-7 shrink-0"
                icon
                aria-label={m.dashboard_widget_subscription_schedule_refresh()}
                disabled={
                  disabled ||
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
        </WidgetHeader>

        <CardContent className="min-h-0 flex-1 gap-0 overflow-hidden pt-3 text-xs">
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
                      'flex min-w-0 items-center gap-3 text-sm',
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
                    className="text-on-surface-variant flex shrink-0 items-center gap-3 text-sm"
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
                              {m.dashboard_widget_subscription_schedule_next_run(
                                {
                                  time: formatDate(
                                    status.data.next_run_at,
                                    'yyyy-MM-dd',
                                  ),
                                },
                              )}
                            </p>
                            <time
                              dateTime={status.data.next_run_at}
                              className={cn(
                                'block text-3xl tabular-nums',
                                compact && 'text-xl',
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
                aria-disabled={disabled}
                tabIndex={disabled ? -1 : 0}
                onClick={(event) => {
                  if (disabled) event.preventDefault()
                }}
                className={cn(
                  'shrink-0 truncate text-sm font-medium',
                  'block',
                  disabled && 'pointer-events-none opacity-50',
                )}
                to="/main/profiles/$type/detail/$uid"
                params={{ type: 'profile', uid: profileUid }}
              >
                {profileName}
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
                  <p className="text-on-surface-variant text-xs" role="status">
                    {m.dashboard_widget_subscription_schedule_loading()}
                  </p>
                ) : runs.isError && !runs.data ? (
                  <p className="text-error text-xs" role="alert">
                    {m.dashboard_widget_subscription_schedule_history_unavailable()}
                  </p>
                ) : completed.length > 0 ? (
                  completed.slice(0, recentRunLimit).map((run, index) => (
                    <p
                      key={run.id}
                      className="flex items-center justify-between gap-2 text-xs"
                      data-slot="subscription-schedule-run"
                    >
                      <span
                        className={cn(
                          'truncate',
                          run.state.kind === 'finished' &&
                            run.state.completion.outcome.kind === 'failed' &&
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
                        className="text-on-surface-variant shrink-0 tabular-nums"
                      >
                        {run.finished_at
                          ? formatDate(run.finished_at, 'MM-dd HH:mm')
                          : '—'}
                      </time>
                    </p>
                  ))
                ) : (
                  <p className="text-on-surface-variant text-xs">
                    {m.dashboard_widget_subscription_schedule_no_runs()}
                  </p>
                )}
              </div>
            )}
            {actionState === 'unconfirmed' && (
              <>
                <Button
                  variant="basic"
                  className="h-7 min-w-0 self-start px-2 text-xs"
                  disabled={disabled || checkingStatus}
                  loading={checkingStatus}
                  onClick={checkStatus}
                >
                  {m.dashboard_widget_operation_check()}
                </Button>
              </>
            )}
          </motion.div>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function SubscriptionScheduleLive({
  id,
  onCloseClick,
  disabled,
  config,
}: WidgetComponentProps & {
  disabled: boolean
  config: Extract<WidgetConfig, { type: WidgetId.SubscriptionSchedule }>
}) {
  const { query } = useProfile()
  const { setIsEditing } = useDashboardContext()
  const profiles = query.data?.items ?? []
  const resolution = query.data
    ? resolveProfileTarget(profiles, query.data.current, config.target)
    : null
  const profile = resolution?.kind === 'selected' ? resolution.profile : null

  if (profile) {
    return (
      <SubscriptionScheduleTarget
        id={id}
        onCloseClick={onCloseClick}
        disabled={disabled}
        config={config}
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
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionSchedule}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col">
        <WidgetHeader>
          <WidgetTitle icon={EventRepeatRounded}>
            {m.dashboard_widget_subscription_schedule_title()}
          </WidgetTitle>
        </WidgetHeader>
        <CardContent className="min-h-0 flex-1 items-center justify-center gap-2 overflow-hidden text-center text-sm">
          <p role={query.isError ? 'alert' : 'status'}>{message}</p>
          {(resolution?.kind === 'missing' ||
            resolution?.kind === 'unsupported' ||
            resolution?.kind === 'needs_selection') && (
            <Button
              variant="raised"
              className="h-8 min-w-0 px-3 text-xs"
              onClick={() => setIsEditing(true)}
            >
              {m.dashboard_widget_subscription_schedule_configure()}
            </Button>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

export function SubscriptionScheduleWidget(props: WidgetComponentProps) {
  const { sourceOnly, isOverlay, disabled } = useDndGridContext()
  const config = useWidgetConfig(props.id, WidgetId.SubscriptionSchedule)

  if (sourceOnly || isOverlay)
    return <SubscriptionSchedulePreview id={props.id} />

  return (
    <SubscriptionScheduleLive {...props} disabled={!disabled} config={config} />
  )
}
