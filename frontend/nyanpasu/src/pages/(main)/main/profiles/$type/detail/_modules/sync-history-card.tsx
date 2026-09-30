import dayjs from 'dayjs'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import {
  useProfileSyncLogs,
  useProfileSyncRuns,
  useProfileSyncStatus,
  type RunDto,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import LogLevelBadge from '../../../../logs/_modules/log-level-badge'

function stateLabel(run: RunDto) {
  switch (run.state.kind) {
    case 'admitted':
      return m.profile_sync_admitted()
    case 'running':
      return m.profile_sync_running()
    case 'cancelling':
      return m.profile_sync_cancelling()
    case 'finalizing':
      return m.profile_sync_finalizing()
    case 'finished': {
      const { outcome } = run.state.completion
      switch (outcome.kind) {
        case 'succeeded':
          return m.profile_sync_succeeded()
        case 'failed':
          return m.profile_sync_failed()
        case 'cancelled':
          return m.profile_sync_cancelled()
        case 'interrupted':
          return m.profile_sync_interrupted()
        case 'skipped':
          return m.profile_sync_skipped()
      }
    }
  }
}

function triggerLabel(run: RunDto) {
  if (run.trigger === 'manual') return m.profile_sync_manual()
  if (run.trigger === 'startup_catch_up') return m.profile_sync_catch_up()
  return m.profile_sync_scheduled()
}

function RunStatus({ run }: { run: RunDto }) {
  const outcome =
    run.state.kind === 'finished' ? run.state.completion.outcome.kind : null

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-medium',
        'bg-surface-variant/50 text-on-surface-variant',
        outcome === 'succeeded' &&
          'bg-primary-container text-on-primary-container',
        outcome === 'failed' && 'bg-error-container text-on-error-container',
        !outcome && 'bg-secondary-container text-on-secondary-container',
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'size-1.5 rounded-full bg-current',
          !outcome && 'animate-pulse motion-reduce:animate-none',
        )}
      />
      {stateLabel(run)}
    </span>
  )
}

function completionDetail(run: RunDto) {
  if (run.state.kind !== 'finished') return null
  const { outcome, journal, output } = run.state.completion
  if (outcome.kind === 'failed') return outcome.message
  if (journal.kind === 'degraded') return m.profile_sync_journal_degraded()
  if (output.kind === 'json_text') {
    const summary = JSON.parse(output.value) as { degradation_count?: number }
    if (summary.degradation_count) return m.profile_sync_effects_degraded()
  }
  return null
}

export function SyncHistoryCard({ uid }: { uid: string }) {
  const status = useProfileSyncStatus(uid)
  const history = useProfileSyncRuns(uid)
  const [selection, setSelection] = useState<string | null>(null)
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
  const selected = runs.find((run) => run.id === selection) ?? runs[0]
  const logs = useProfileSyncLogs(uid, selected?.id ?? null)
  const error = status.error ?? history.error ?? logs.error
  const detail = selected ? completionDetail(selected) : null

  return (
    <Card className="@container col-span-2 md:col-span-4">
      <CardHeader>{m.profile_sync_history_title()}</CardHeader>
      <CardContent>
        {status.data && (
          <div className="text-on-surface-variant flex flex-wrap items-center gap-x-4 gap-y-2 text-xs">
            <span
              className={cn(
                'inline-flex items-center gap-2 rounded-full px-3 py-1.5 font-medium',
                status.data.scheduled
                  ? 'bg-primary-container/50 text-on-primary-container'
                  : 'bg-surface-variant/40',
              )}
            >
              <span
                aria-hidden="true"
                className="size-1.5 rounded-full bg-current"
              />
              {status.data.scheduled
                ? m.profile_sync_schedule_enabled()
                : m.profile_sync_schedule_disabled()}
            </span>
            {status.data.next_run_at && (
              <span className="tabular-nums">
                {m.profile_sync_next_run({
                  time: dayjs(status.data.next_run_at).format(
                    'YYYY-MM-DD HH:mm:ss',
                  ),
                })}
              </span>
            )}
            <span className="@[40rem]:ml-auto">
              {m.profile_sync_retention({ count: status.data.history_limit })}
            </span>
            {status.data.journal_degraded && (
              <span
                role="status"
                className="bg-tertiary-container/40 text-on-tertiary-container w-full rounded-xl px-3 py-2"
              >
                {m.profile_sync_journal_degraded()}
              </span>
            )}
            {status.data.registration_error && (
              <span
                role="alert"
                className="bg-error-container/40 text-on-error-container w-full rounded-xl px-3 py-2 break-words"
              >
                {status.data.registration_error}
              </span>
            )}
          </div>
        )}
        {error && (
          <p
            role="alert"
            className="bg-error-container/40 text-on-error-container rounded-xl px-3 py-2 text-sm break-words"
          >
            {formatError(error)}
          </p>
        )}
        {history.isPending ? (
          <p
            role="status"
            className="text-on-surface-variant bg-surface-variant/20 rounded-2xl px-4 py-10 text-center text-sm"
          >
            {m.profile_sync_loading()}
          </p>
        ) : runs.length === 0 ? (
          <p className="text-on-surface-variant bg-surface-variant/20 rounded-2xl px-4 py-10 text-center text-sm">
            {m.profile_sync_empty()}
          </p>
        ) : (
          <div className="grid min-w-0 items-start gap-4 @[40rem]:grid-cols-[minmax(0,18rem)_minmax(0,1fr)]">
            <div className="flex max-h-80 min-w-0 flex-col gap-1 overflow-y-auto p-0.5">
              {runs.map((run) => (
                <Button
                  key={run.id}
                  type="button"
                  className="text-on-surface dark:text-on-surface hover:bg-surface-variant/40 dark:hover:bg-surface-variant/40 aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container focus-visible:ring-primary flex h-auto min-h-20 w-full shrink-0 items-center justify-between gap-3 rounded-2xl px-3 py-3 text-left focus-visible:ring-2 focus-visible:ring-inset"
                  aria-pressed={run.id === selected?.id}
                  onClick={() => setSelection(run.id)}
                >
                  <span className="flex min-w-0 flex-col gap-1.5">
                    <time
                      dateTime={run.admitted_at}
                      className="text-sm leading-snug tabular-nums"
                    >
                      {dayjs(run.admitted_at).format('YYYY-MM-DD')}
                      <span className="ml-2 inline-block">
                        {dayjs(run.admitted_at).format('HH:mm:ss')}
                      </span>
                    </time>
                    <span className="text-on-surface-variant text-xs font-normal">
                      {triggerLabel(run)}
                    </span>
                  </span>
                  <RunStatus run={run} />
                </Button>
              ))}
              {history.hasNextPage && (
                <Button
                  className="mt-1 shrink-0 self-center"
                  onClick={() => history.fetchNextPage()}
                  loading={history.isFetchingNextPage}
                >
                  {m.profile_sync_older()}
                </Button>
              )}
            </div>
            <div className="border-outline-variant/40 bg-surface-variant/20 min-w-0 overflow-hidden rounded-2xl border">
              {selected && (
                <>
                  <div className="border-outline-variant/40 flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
                    <RunStatus run={selected} />
                    <time
                      dateTime={selected.admitted_at}
                      className="text-on-surface-variant text-xs tabular-nums"
                    >
                      {dayjs(selected.admitted_at).format(
                        'YYYY-MM-DD HH:mm:ss',
                      )}
                    </time>
                  </div>
                  {detail && (
                    <p className="text-on-surface-variant px-4 pt-3 text-sm break-words">
                      {detail}
                    </p>
                  )}
                  {selected.dropped_log_count !== '0' && (
                    <p className="text-on-surface-variant px-4 pt-3 text-xs">
                      {m.profile_sync_dropped_logs({
                        count: selected.dropped_log_count,
                      })}
                    </p>
                  )}
                </>
              )}
              <div className="max-h-80 min-h-48 overflow-auto p-2">
                {logs.isPending ? (
                  <p
                    role="status"
                    className="text-on-surface-variant px-2 py-8 text-center text-sm"
                  >
                    {m.profile_sync_loading()}
                  </p>
                ) : logs.data?.pages.some((page) => page.items.length > 0) ? (
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
                            {dayjs(entry.time).format('HH:mm:ss')}
                          </time>
                          <LogLevelBadge>{entry.level}</LogLevelBadge>
                          <span className="min-w-0 font-mono text-xs leading-relaxed [overflow-wrap:anywhere] whitespace-pre-wrap">
                            {entry.fields.message ?? entry.fields.stage}
                          </span>
                        </li>
                      )),
                    )}
                  </ol>
                ) : (
                  <p className="text-on-surface-variant px-2 py-8 text-center text-sm">
                    {m.profile_sync_no_logs()}
                  </p>
                )}
              </div>
              {logs.hasNextPage && (
                <Button
                  className="mx-2 mb-2"
                  onClick={() => logs.fetchNextPage()}
                  loading={logs.isFetchingNextPage}
                >
                  {m.profile_sync_more_logs()}
                </Button>
              )}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
