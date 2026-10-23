import CheckRounded from '~icons/material-symbols/check-rounded'
import ListRounded from '~icons/material-symbols/list-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import ScheduleRounded from '~icons/material-symbols/schedule-rounded'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import type { RuntimeInspection } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  CompactButton,
  RowAction,
  searchPillClassName,
} from '../../_modules/compact-controls'
import PageToolbar from '../../_modules/page-toolbar'

/** Which snapshot the page shows. */
export type SnapshotView = 'latest' | 'applied'

function statusText(snapshot: RuntimeInspection) {
  if (snapshot.effective_pending) return m.inspect_effective_pending()
  return snapshot.applied ? m.inspect_applied() : m.inspect_not_applied()
}

const snapshotItemClassName = 'flex-none px-3.5 text-[13px] whitespace-nowrap'

/**
 * The control row of the page: which snapshot to show, what it is, a search
 * over the fields the steps changed, and a refresh.
 */
export default function InspectToolbar({
  snapshot,
  view,
  onViewChange,
  fieldQuery,
  onFieldQueryChange,
  onCycleMatch,
  onRefresh,
  onShowSteps,
}: {
  /** Absent while loading and when there is no snapshot. */
  snapshot?: RuntimeInspection | null
  view: SnapshotView
  onViewChange: (view: SnapshotView) => void
  fieldQuery: string
  onFieldQueryChange: (query: string) => void
  /** Enter selects the next step with a matching field; Shift+Enter, the previous. */
  onCycleMatch: (delta: 1 | -1) => void
  onRefresh: () => void
  /** Opens the step list where it does not fit beside the details. */
  onShowSteps: () => void
}) {
  const settled = snapshot?.applied && !snapshot.effective_pending
  const StatusIcon = settled ? CheckRounded : ScheduleRounded

  return (
    <PageToolbar
      title={m.inspect_title()}
      backTo="/main/profiles"
      context={
        <>
          <SegmentedButton
            className="h-8 w-auto shrink-0 text-[13px]"
            aria-label={m.inspect_snapshot_label()}
            value={view}
            onValueChange={(value) => {
              if (value) onViewChange(value as SnapshotView)
            }}
          >
            <SegmentedButtonItem
              value="latest"
              hideIndicator
              className={snapshotItemClassName}
            >
              {m.inspect_snapshot_latest()}
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="applied"
              hideIndicator
              className={snapshotItemClassName}
            >
              {m.inspect_snapshot_applied()}
            </SegmentedButtonItem>
          </SegmentedButton>

          {snapshot && (
            <p
              data-slot="inspect-status"
              data-applied={snapshot.applied || undefined}
              className={cn(
                'bg-surface-variant text-on-surface-variant hidden h-7 min-w-0 items-center gap-1.5',
                'rounded-lg px-2.5 text-xs font-medium whitespace-nowrap @3xl:inline-flex',
              )}
            >
              <StatusIcon
                aria-hidden
                className={cn(
                  'size-4 shrink-0',
                  settled ? 'text-ok' : 'text-warn',
                )}
              />

              <span className="truncate">
                {statusText(snapshot)} · {snapshot.target_core} ·{' '}
                {m.inspect_revision()} {snapshot.revision}
              </span>
            </p>
          )}
        </>
      }
      actions={
        <>
          <CompactButton
            className="@3xl:hidden"
            onClick={onShowSteps}
            data-slot="inspect-show-steps"
          >
            <ListRounded aria-hidden />
            {m.inspect_show_steps()}
          </CompactButton>

          <SearchField
            className={cn('w-40 @3xl:w-[250px]', searchPillClassName)}
            value={fieldQuery}
            onValueChange={onFieldQueryChange}
            clearLabel={m.inspect_find_clear()}
            placeholder={m.inspect_find_placeholder()}
            aria-label={m.inspect_find_field()}
            disabled={!snapshot}
            onKeyDown={(event) => {
              if (event.key !== 'Enter') return
              event.preventDefault()
              onCycleMatch(event.shiftKey ? -1 : 1)
            }}
          />

          <RowAction
            label={m.inspect_refresh()}
            className="size-10"
            onClick={onRefresh}
          >
            <RefreshRounded className="size-5" aria-hidden />
          </RowAction>
        </>
      }
    />
  )
}
