import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
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
import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import {
  latestFinishedRuns,
  resolveProfileTarget,
} from './dashboard-daily-utils'
import { useDashboardContext, useWidgetConfig } from './provider'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'

function SubscriptionSchedulePreview({ id }: { id: string }) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.SubscriptionSchedule}
      minW={3}
      minH={2}
    >
      <Card className="flex size-full flex-col">
        <CardHeader className="shrink-0 text-base font-medium">
          {m.dashboard_widget_subscription_schedule_title()}
        </CardHeader>
        <CardContent className="min-h-0 flex-1 justify-center gap-2">
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
  const { displayItems } = useDndGridContext()
  const queryClient = useQueryClient()
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const activeRun = status.data?.active[0]
  const expanded = (displayItems.find((item) => item.id === id)?.h ?? 2) >= 3
  const completed = latestFinishedRuns(
    runs.data?.pages.flatMap((page) => page.items) ?? [],
    expanded ? 3 : 1,
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

  const unavailable = status.isError && !status.data

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
        <CardHeader className="shrink-0 flex-row items-center justify-between gap-2 px-3 pt-2 pb-1">
          <div className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">
              {m.dashboard_widget_subscription_schedule_title()}
            </span>
            <Link
              aria-disabled={disabled}
              tabIndex={disabled ? -1 : 0}
              onClick={(event) => {
                if (disabled) event.preventDefault()
              }}
              className={`text-on-surface-variant block truncate text-[10px] ${disabled ? 'pointer-events-none opacity-50' : ''}`}
              to="/main/profiles/$type/detail/$uid"
              params={{ type: 'profile', uid: profileUid }}
            >
              {profileName}
            </Link>
          </div>
          <Button
            variant="raised"
            className="h-8 min-w-0 shrink-0 px-2 text-xs"
            disabled={
              disabled ||
              update.isPending ||
              checkingStatus ||
              (actionState === 'unconfirmed' && !checkedAfterUnconfirmed)
            }
            loading={update.isPending}
            onClick={refresh}
          >
            {m.dashboard_widget_subscription_schedule_refresh()}
          </Button>
        </CardHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-y-auto px-3 py-1 text-xs">
          {status.isPending && (
            <p role="status">
              {m.dashboard_widget_subscription_schedule_loading()}
            </p>
          )}
          {unavailable && (
            <p role="alert" className="text-error">
              {m.dashboard_widget_subscription_schedule_load_failed()}
            </p>
          )}
          {status.data && (
            <>
              {activeRun ? (
                <p role="status" className="font-medium">
                  {m.dashboard_widget_subscription_schedule_active({
                    state: runStateLabel(activeRun),
                  })}
                </p>
              ) : status.data.scheduled && status.data.next_run_at ? (
                <p>
                  {m.dashboard_widget_subscription_schedule_next_run({
                    time: formatDate(status.data.next_run_at),
                  })}
                </p>
              ) : (
                <p>{m.dashboard_widget_subscription_schedule_manual_only()}</p>
              )}
              {status.data.registration_error && (
                <p className="text-error line-clamp-2" role="alert">
                  {m.dashboard_widget_subscription_schedule_registration_error({
                    error: status.data.registration_error,
                  })}
                </p>
              )}
              {status.data.journal_degraded && (
                <p className="text-on-surface-variant" role="status">
                  {m.dashboard_widget_subscription_schedule_journal_degraded()}
                </p>
              )}
            </>
          )}
          {config.showRecentRuns && expanded && (
            <div className="space-y-1 pt-1">
              <p className="text-on-surface-variant text-xs">
                {m.dashboard_widget_subscription_schedule_recent()}
              </p>
              {runs.isPending ? (
                <p className="text-on-surface-variant text-xs" role="status">
                  {m.dashboard_widget_subscription_schedule_loading()}
                </p>
              ) : runs.isError && !runs.data ? (
                <p className="text-error text-xs" role="alert">
                  {m.dashboard_widget_subscription_schedule_history_unavailable()}
                </p>
              ) : completed.length > 0 ? (
                completed.map((run) => (
                  <p
                    key={run.id}
                    className="flex items-center justify-between gap-2 text-xs"
                    data-slot="subscription-schedule-run"
                  >
                    <span className="truncate">{runStateLabel(run)}</span>
                    <time className="text-on-surface-variant shrink-0 tabular-nums">
                      {run.finished_at ? formatDate(run.finished_at) : '—'}
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
          {config.showRecentRuns && !expanded && completed[0] && (
            <p className="text-on-surface-variant truncate text-[10px]">
              {m.dashboard_widget_subscription_schedule_recent()}:{' '}
              {runStateLabel(completed[0])}
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
                ? m.dashboard_widget_subscription_schedule_committed_degraded()
                : actionState === 'unconfirmed'
                  ? m.dashboard_widget_subscription_schedule_unconfirmed()
                  : m.dashboard_widget_subscription_schedule_refresh_failed()}
            </p>
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
              {checkFailed && (
                <p className="text-error text-xs" role="alert">
                  {m.dashboard_widget_operation_check_failed()}
                </p>
              )}
            </>
          )}
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
        <CardHeader className="shrink-0 text-base font-medium">
          {m.dashboard_widget_subscription_schedule_title()}
        </CardHeader>
        <CardContent className="min-h-0 flex-1 items-center justify-center gap-2 text-center text-sm">
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
