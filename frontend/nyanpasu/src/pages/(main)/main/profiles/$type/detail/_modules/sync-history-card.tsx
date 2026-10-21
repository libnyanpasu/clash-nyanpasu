import CancelRounded from '~icons/material-symbols/cancel-rounded'
import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded'
import HistoryRounded from '~icons/material-symbols/history-rounded'
import HistoryToggleOffRounded from '~icons/material-symbols/history-toggle-off-rounded'
import HourglassTopRounded from '~icons/material-symbols/hourglass-top-rounded'
import PersonRounded from '~icons/material-symbols/person-rounded'
import ReportRounded from '~icons/material-symbols/report-rounded'
import SaveRounded from '~icons/material-symbols/save-rounded'
import ScheduleRounded from '~icons/material-symbols/schedule-rounded'
import SkipNextRounded from '~icons/material-symbols/skip-next-rounded'
import StopCircleRounded from '~icons/material-symbols/stop-circle-rounded'
import SyncRounded from '~icons/material-symbols/sync-rounded'
import UpdateRounded from '~icons/material-symbols/update-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import {
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from 'react'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@nyanpasu/ui/accordion'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui/card'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { ErrorMessage } from '@/components/error-message'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import {
  useProfileSyncLogs,
  useProfileSyncRuns,
  useProfileSyncStatus,
} from '@nyanpasu/query'
import { type RunDto } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useQueryClient } from '@tanstack/react-query'
import LogLevelBadge from '../../../../logs/_modules/log-level-badge'

type IconComponent = ComponentType<{ className?: string }>

type Tone = 'success' | 'error' | 'progress' | 'warning' | 'neutral'

// Tinted from the foreground role so the same tone reads on a plain surface
// and on the selected (secondary-container) list item.
const TONE_CLASSES = {
  success: 'bg-primary/15 text-primary',
  error: 'bg-error/15 text-error',
  progress: 'bg-secondary/15 text-secondary',
  warning: 'bg-tertiary/15 text-tertiary',
  neutral: 'bg-on-surface-variant/12 text-on-surface-variant',
} as const satisfies Record<Tone, string>

type RunStatus = {
  label: string
  icon: IconComponent
  tone: Tone
  /** The run is still in flight, so its icon animates. */
  busy: boolean
}

function runStatus(run: RunDto): RunStatus {
  switch (run.state.kind) {
    case 'admitted':
      return {
        label: m.profile_sync_admitted(),
        icon: HourglassTopRounded,
        tone: 'progress',
        busy: true,
      }
    case 'running':
      return {
        label: m.profile_sync_running(),
        icon: SyncRounded,
        tone: 'progress',
        busy: true,
      }
    case 'cancelling':
      return {
        label: m.profile_sync_cancelling(),
        icon: StopCircleRounded,
        tone: 'progress',
        busy: true,
      }
    case 'finalizing':
      return {
        label: m.profile_sync_finalizing(),
        icon: SaveRounded,
        tone: 'progress',
        busy: true,
      }
    case 'finished': {
      const { outcome } = run.state.completion
      switch (outcome.kind) {
        case 'succeeded':
          return {
            label: m.profile_sync_succeeded(),
            icon: CheckCircleRounded,
            tone: 'success',
            busy: false,
          }
        case 'failed':
          return {
            label: m.profile_sync_failed(),
            icon: ErrorRounded,
            tone: 'error',
            busy: false,
          }
        case 'cancelled':
          return {
            label: m.profile_sync_cancelled(),
            icon: CancelRounded,
            tone: 'neutral',
            busy: false,
          }
        case 'interrupted':
          return {
            label: m.profile_sync_interrupted(),
            icon: ReportRounded,
            tone: 'warning',
            busy: false,
          }
        case 'skipped':
          return {
            label: m.profile_sync_skipped(),
            icon: SkipNextRounded,
            tone: 'neutral',
            busy: false,
          }
      }
    }
  }
}

function runTrigger(run: RunDto): { label: string; icon: IconComponent } {
  if (run.trigger === 'manual') {
    return { label: m.profile_sync_manual(), icon: PersonRounded }
  }
  if (run.trigger === 'startup_catch_up') {
    return { label: m.profile_sync_catch_up(), icon: UpdateRounded }
  }
  return { label: m.profile_sync_scheduled(), icon: ScheduleRounded }
}

type CompletionDetail = { message: string; tone: 'error' | 'warning' }

function completionDetail(run: RunDto): CompletionDetail | null {
  if (run.state.kind !== 'finished') return null
  const { outcome, journal, output } = run.state.completion
  if (outcome.kind === 'failed') {
    return { message: outcome.message, tone: 'error' }
  }
  if (journal.kind === 'degraded') {
    return { message: m.profile_sync_journal_degraded(), tone: 'warning' }
  }
  if (output.kind === 'json_text') {
    const summary = JSON.parse(output.value) as { degradation_count?: number }
    if (summary.degradation_count) {
      return { message: m.profile_sync_effects_degraded(), tone: 'warning' }
    }
  }
  return null
}

function StatusIcon({
  status,
  className,
}: {
  status: RunStatus
  className?: string
}) {
  const Icon = status.icon

  return (
    <span
      aria-hidden="true"
      data-slot="profile-sync-status-icon"
      className={cn(
        'grid size-10 shrink-0 place-items-center rounded-full',
        TONE_CLASSES[status.tone],
        className,
      )}
    >
      <Icon
        className={cn(
          'size-5',
          status.busy &&
            (status.icon === SyncRounded ? 'animate-spin' : 'animate-pulse'),
          'motion-reduce:animate-none',
        )}
      />
    </span>
  )
}

function Chip({
  icon: Icon,
  tonal,
  className,
  children,
}: {
  icon: IconComponent
  tonal?: boolean
  className?: string
  children: ReactNode
}) {
  return (
    <span
      data-slot="profile-sync-chip"
      className={cn(
        'inline-flex h-8 max-w-full items-center gap-2 rounded-lg border px-3 text-sm font-medium',
        tonal
          ? 'bg-secondary-container text-on-secondary-container border-transparent'
          : 'border-outline-variant text-on-surface-variant',
        className,
      )}
    >
      <Icon className="size-4.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0 truncate">{children}</span>
    </span>
  )
}

function Notice({
  tone,
  role,
  children,
}: {
  tone: 'error' | 'warning'
  role: 'alert' | 'status'
  children: ReactNode
}) {
  const Icon = tone === 'error' ? ErrorRounded : WarningRounded

  return (
    <div
      role={role}
      data-slot="profile-sync-notice"
      className={cn(
        'flex items-start gap-3 rounded-2xl px-4 py-3 text-sm',
        tone === 'error'
          ? 'bg-error-container text-on-error-container'
          : 'bg-tertiary-container text-on-tertiary-container',
      )}
    >
      <Icon className="mt-0.5 size-5 shrink-0" aria-hidden="true" />
      <div className="min-w-0 wrap-break-word">{children}</div>
    </div>
  )
}

function Placeholder({
  busy,
  className,
  children,
}: {
  busy?: boolean
  className?: string
  children: ReactNode
}) {
  return (
    <div
      role={busy ? 'status' : undefined}
      data-slot="profile-sync-placeholder"
      className={cn(
        'text-on-surface-variant flex flex-col items-center justify-center gap-3 px-4 py-10 text-center text-sm',
        className,
      )}
    >
      {busy && <CircularProgress className="size-8" indeterminate />}
      <p>{children}</p>
    </div>
  )
}

function RunEntry({ run, children }: { run: RunDto; children: ReactNode }) {
  const status = runStatus(run)
  const trigger = runTrigger(run)
  const detail = completionDetail(run)

  return (
    <AccordionItem value={run.id} data-slot="profile-sync-run">
      <AccordionTrigger>
        <StatusIcon status={status} />

        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate text-sm font-medium">{status.label}</span>

          <time
            dateTime={run.admitted_at}
            className="text-on-surface-variant text-xs font-normal tabular-nums"
          >
            {formatDate(run.admitted_at)}
          </time>
        </span>

        <Chip icon={trigger.icon} className="hidden @md:inline-flex">
          {trigger.label}
        </Chip>

        <span
          title={trigger.label}
          data-slot="profile-sync-run-trigger"
          className="text-on-surface-variant shrink-0 @md:hidden"
        >
          <trigger.icon className="size-5" />
          <span className="sr-only">{trigger.label}</span>
        </span>
      </AccordionTrigger>

      <AccordionContent>
        {detail && (
          <Notice
            tone={detail.tone}
            role={detail.tone === 'error' ? 'alert' : 'status'}
          >
            {detail.message}
          </Notice>
        )}

        {run.dropped_log_count !== '0' && (
          <p className="text-on-surface-variant text-xs">
            {m.profile_sync_dropped_logs({ count: run.dropped_log_count })}
          </p>
        )}

        {children}
      </AccordionContent>
    </AccordionItem>
  )
}

export function SyncHistoryCard({ uid }: { uid: string }) {
  const status = useProfileSyncStatus(uid)
  const history = useProfileSyncRuns(uid)
  // undefined keeps the latest run open; an empty string is every run collapsed.
  const [expansion, setExpansion] = useState<string | undefined>(undefined)
  // Active inspection contains the current state; durable admission rows may
  // still say admitted until their owner finalizes them.
  const rows = new Map<string, RunDto>()
  for (const page of history.data?.pages ?? []) {
    for (const run of page.items) rows.set(run.id, run)
  }
  for (const run of status.data?.active ?? []) rows.set(run.id, run)
  const runs = [...rows.values()].sort((a, b) =>
    BigInt(a.admission_sequence) > BigInt(b.admission_sequence) ? -1 : 1,
  )
  const expanded = expansion ?? runs[0]?.id ?? ''
  const selected = runs.find((run) => run.id === expanded)
  const activeIds = status.data?.active.map((run) => run.id) ?? []
  const logs = useProfileSyncLogs(
    uid,
    selected?.id ?? null,
    selected !== undefined && activeIds.includes(selected.id),
  )

  // The history and logs are not polled. A run entering or leaving the active
  // set is what changes them, so refresh both then (the logs once more to pick
  // up the lines written after the last poll).
  const queryClient = useQueryClient()
  const activeKey = status.data ? activeIds.join() : undefined
  const previousActiveKey = useRef(activeKey)
  useEffect(() => {
    const previous = previousActiveKey.current
    previousActiveKey.current = activeKey
    if (previous === undefined || previous === activeKey) return
    queryClient.invalidateQueries({ queryKey: ['profile-sync-runs', uid] })
    queryClient.invalidateQueries({ queryKey: ['profile-sync-logs', uid] })
  }, [activeKey, queryClient, uid])
  const error = status.error ?? history.error ?? logs.error

  return (
    <Card
      data-slot="profile-sync-history"
      className="@container col-span-2 md:col-span-4"
    >
      <CardHeader className="flex-col items-start gap-1">
        <div className="flex items-center gap-3">
          <HistoryRounded className="text-primary size-6" aria-hidden="true" />
          <h2 className="text-lg font-medium">
            {m.profile_sync_history_title()}
          </h2>
        </div>

        {status.data && (
          <p className="text-on-surface-variant text-sm">
            {m.profile_sync_retention({ count: status.data.history_limit })}
          </p>
        )}
      </CardHeader>

      <CardContent>
        {status.data && (
          <div
            data-slot="profile-sync-summary"
            className="flex flex-wrap items-center gap-2"
          >
            <Chip
              tonal={status.data.scheduled}
              icon={
                status.data.scheduled
                  ? EventRepeatRounded
                  : HistoryToggleOffRounded
              }
            >
              {status.data.scheduled
                ? m.profile_sync_schedule_enabled()
                : m.profile_sync_schedule_disabled()}
            </Chip>

            {status.data.next_run_at && (
              <Chip icon={ScheduleRounded} className="tabular-nums">
                {m.profile_sync_next_run({
                  time: formatDate(status.data.next_run_at),
                })}
              </Chip>
            )}
          </div>
        )}

        {status.data?.journal_degraded && (
          <Notice tone="warning" role="status">
            {m.profile_sync_journal_degraded()}
          </Notice>
        )}

        {status.data?.registration_error && (
          <Notice tone="error" role="alert">
            {status.data.registration_error}
          </Notice>
        )}

        {error && (
          <Notice tone="error" role="alert">
            <ErrorMessage error={error} />
          </Notice>
        )}

        {history.isPending ? (
          <Placeholder busy className="bg-surface-variant/30 rounded-3xl">
            {m.profile_sync_loading()}
          </Placeholder>
        ) : runs.length === 0 ? (
          <EmptyState tone="normal" message={m.profile_sync_empty()} />
        ) : (
          <>
            <Accordion value={expanded} onValueChange={setExpansion}>
              {runs.map((run) => (
                <RunEntry key={run.id} run={run}>
                  <div
                    data-slot="profile-sync-logs"
                    className="bg-surface flex min-w-0 flex-col overflow-hidden rounded-2xl"
                  >
                    <div className="max-h-80 min-h-32 overflow-auto p-2">
                      {logs.isPending ? (
                        <Placeholder busy>
                          {m.profile_sync_loading()}
                        </Placeholder>
                      ) : logs.data?.pages.some(
                          (page) => page.items.length > 0,
                        ) ? (
                        <ol className="divide-outline-variant/30 divide-y">
                          {logs.data.pages.flatMap((page) =>
                            page.items.map((entry) => (
                              <li
                                key={entry.sequence}
                                className="grid grid-cols-[auto_auto_minmax(0,1fr)] items-start gap-x-3 px-2 py-2.5"
                              >
                                <time
                                  dateTime={entry.time}
                                  className="text-on-surface-variant pt-1 font-mono text-[11px] leading-none tabular-nums"
                                >
                                  {formatDate(entry.time, 'HH:mm:ss')}
                                </time>

                                <LogLevelBadge>{entry.level}</LogLevelBadge>

                                <span className="min-w-0 font-mono text-xs leading-relaxed wrap-anywhere whitespace-pre-wrap">
                                  {entry.fields.message ?? entry.fields.stage}
                                </span>
                              </li>
                            )),
                          )}
                        </ol>
                      ) : (
                        <EmptyState
                          size="compact"
                          tone="normal"
                          icon={HistoryRounded}
                          message={m.profile_sync_no_logs()}
                        />
                      )}
                    </div>

                    {logs.hasNextPage && (
                      <Button
                        variant="stroked"
                        className="mx-2 mb-2 self-center"
                        onClick={() => logs.fetchNextPage()}
                        loading={logs.isFetchingNextPage}
                      >
                        {m.profile_sync_more_logs()}
                      </Button>
                    )}
                  </div>
                </RunEntry>
              ))}
            </Accordion>

            {history.hasNextPage && (
              <Button
                variant="stroked"
                className="self-center"
                onClick={() => history.fetchNextPage()}
                loading={history.isFetchingNextPage}
              >
                {m.profile_sync_older()}
              </Button>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}
