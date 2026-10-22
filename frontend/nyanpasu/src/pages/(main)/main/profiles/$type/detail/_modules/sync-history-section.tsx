import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded'
import ExpandLessRounded from '~icons/material-symbols/expand-less-rounded'
import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import HistoryToggleOffRounded from '~icons/material-symbols/history-toggle-off-rounded'
import ScheduleRounded from '~icons/material-symbols/schedule-rounded'
import { AnimatePresence, useReducedMotion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { Accordion } from '@nyanpasu/ui/accordion'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
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
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'
import { DetailRow, DetailSection } from './detail-section'
import {
  Chip,
  mergeSyncRuns,
  Notice,
  RunEntry,
  RunLogs,
} from './sync-history-run'

const LATEST_RUN_COUNT = 3

/**
 * The sync history of a remote profile: the latest runs, with the older ones
 * unfolding below them on request. Every run opens in place to its details.
 */
export default function SyncHistorySection({ uid }: { uid: string }) {
  const status = useProfileSyncStatus(uid)
  const history = useProfileSyncRuns(uid)

  const reduceMotion = useReducedMotion()

  const [expanded, setExpanded] = useState(false)

  // The open run's id; an empty string is every run collapsed.
  const [openRun, setOpenRun] = useState('')

  const runs = mergeSyncRuns(
    history.data?.pages.flatMap((page) => page.items),
    status.data?.active,
  )

  const latestRuns = runs.slice(0, LATEST_RUN_COUNT)

  const olderRuns = runs.slice(LATEST_RUN_COUNT)

  const selected = runs.find((run) => run.id === openRun)

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

    if (previous === undefined || previous === activeKey) {
      return
    }

    queryClient.invalidateQueries({ queryKey: ['profile-sync-runs', uid] })
    queryClient.invalidateQueries({ queryKey: ['profile-sync-logs', uid] })
  }, [activeKey, queryClient, uid])

  const error = status.error ?? history.error ?? logs.error

  const hasSummary = status.data !== undefined || error != null

  const hasToggle = olderRuns.length > 0 || history.hasNextPage

  const toggleExpanded = () => {
    // A run folded away with the older ones must not keep its logs polling.
    if (expanded && olderRuns.some((run) => run.id === openRun)) {
      setOpenRun('')
    }

    setExpanded(!expanded)
  }

  // The runs are rows of the section's group, so only the group's outer
  // corners are round.
  const renderRun = (run: RunDto, index: number) => (
    <RunEntry
      key={run.id}
      run={run}
      className={cn(
        !hasSummary && index === 0 && 'rounded-t-3xl',
        !hasToggle && index === runs.length - 1 && 'rounded-b-3xl',
      )}
    >
      <RunLogs logs={logs} />
    </RunEntry>
  )

  return (
    <DetailSection
      title={m.profile_sync_history_title()}
      description={
        status.data &&
        m.profile_sync_retention({ count: status.data.history_limit })
      }
      className="@container"
    >
      {hasSummary && (
        <SettingsCard data-slot="profile-sync-summary">
          <SettingsCardContent className="gap-3 py-4">
            {status.data && (
              <div className="flex flex-wrap items-center gap-2">
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
          </SettingsCardContent>
        </SettingsCard>
      )}

      {runs.length === 0 ? (
        <SettingsCard data-slot="profile-detail-run-empty">
          <SettingsCardContent className="text-on-surface-variant py-4 text-sm">
            {history.isPending
              ? m.profile_sync_loading()
              : m.profile_sync_empty()}
          </SettingsCardContent>
        </SettingsCard>
      ) : (
        <Accordion value={openRun} onValueChange={setOpenRun} className="gap-1">
          {latestRuns.map(renderRun)}

          <AnimatePresence initial={false}>
            {expanded && olderRuns.length > 0 && (
              <AnimatedItem
                key="older-runs"
                data-slot="profile-sync-older-runs"
                className="flex flex-col gap-1"
                {...(reduceMotion && { transition: { duration: 0 } })}
              >
                {olderRuns.map((run, index) =>
                  renderRun(run, LATEST_RUN_COUNT + index),
                )}
              </AnimatedItem>
            )}
          </AnimatePresence>
        </Accordion>
      )}

      {hasToggle && (
        <DetailRow
          label={
            expanded
              ? m.profile_detail_history_hide()
              : m.profile_detail_history_view_all()
          }
          aria-expanded={expanded}
          trailing={
            expanded ? (
              <ExpandLessRounded
                className="text-on-surface-variant size-5 shrink-0"
                aria-hidden
              />
            ) : (
              <ExpandMoreRounded
                className="text-on-surface-variant size-5 shrink-0"
                aria-hidden
              />
            )
          }
          onClick={toggleExpanded}
        />
      )}

      {expanded && history.hasNextPage && (
        <DetailRow
          label={m.profile_sync_older()}
          disabled={history.isFetchingNextPage}
          onClick={() => history.fetchNextPage()}
        />
      )}
    </DetailSection>
  )
}
