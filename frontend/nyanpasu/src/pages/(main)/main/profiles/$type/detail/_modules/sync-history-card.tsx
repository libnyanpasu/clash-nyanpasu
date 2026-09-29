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

  return (
    <Card className="col-span-2 md:col-span-4">
      <CardHeader>{m.profile_sync_history_title()}</CardHeader>
      <CardContent>
        {status.data && (
          <div className="flex flex-wrap gap-4 text-sm">
            <span>
              {status.data.scheduled
                ? m.profile_sync_schedule_enabled()
                : m.profile_sync_schedule_disabled()}
            </span>
            {status.data.next_run_at && (
              <span>
                {m.profile_sync_next_run({
                  time: dayjs(status.data.next_run_at).format(
                    'YYYY-MM-DD HH:mm:ss',
                  ),
                })}
              </span>
            )}
            <span>
              {m.profile_sync_retention({ count: status.data.history_limit })}
            </span>
            {status.data.journal_degraded && (
              <span role="status">{m.profile_sync_journal_degraded()}</span>
            )}
            {status.data.registration_error && (
              <span role="alert">{status.data.registration_error}</span>
            )}
          </div>
        )}
        {error && <p role="alert">{formatError(error)}</p>}
        {history.isPending ? (
          <p>{m.profile_sync_loading()}</p>
        ) : runs.length === 0 ? (
          <p className="text-sm">{m.profile_sync_empty()}</p>
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            <div className="flex flex-col gap-2">
              {runs.map((run) => (
                <Button
                  key={run.id}
                  variant={run.id === selected?.id ? 'raised' : 'stroked'}
                  className="h-auto justify-between gap-2 py-2 text-left"
                  aria-pressed={run.id === selected?.id}
                  onClick={() => setSelection(run.id)}
                >
                  <span>
                    {dayjs(run.admitted_at).format('YYYY-MM-DD HH:mm:ss')}
                    <span className="block text-xs">{triggerLabel(run)}</span>
                  </span>
                  <span>{stateLabel(run)}</span>
                </Button>
              ))}
              {history.hasNextPage && (
                <Button
                  onClick={() => history.fetchNextPage()}
                  loading={history.isFetchingNextPage}
                >
                  {m.profile_sync_older()}
                </Button>
              )}
            </div>
            <div className="min-w-0">
              {selected && (
                <>
                  <p className="mb-2 text-sm font-medium">
                    {stateLabel(selected)}
                  </p>
                  {completionDetail(selected) && (
                    <p className="mb-2 text-sm">{completionDetail(selected)}</p>
                  )}
                  {selected.dropped_log_count !== '0' && (
                    <p className="mb-2 text-sm">
                      {m.profile_sync_dropped_logs({
                        count: selected.dropped_log_count,
                      })}
                    </p>
                  )}
                </>
              )}
              <div className="bg-surface-container max-h-80 overflow-auto rounded-xl p-3 font-mono text-xs">
                {logs.isPending ? (
                  <p>{m.profile_sync_loading()}</p>
                ) : logs.data?.pages.some((page) => page.items.length > 0) ? (
                  logs.data.pages.flatMap((page) =>
                    page.items.map((entry) => (
                      <div key={entry.sequence} className="mb-1 break-words">
                        {dayjs(entry.time).format('HH:mm:ss')} [{entry.level}]{' '}
                        {entry.fields.message ?? entry.fields.stage}
                      </div>
                    )),
                  )
                ) : (
                  <p>{m.profile_sync_no_logs()}</p>
                )}
              </div>
              {logs.hasNextPage && (
                <Button
                  className="mt-2"
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
