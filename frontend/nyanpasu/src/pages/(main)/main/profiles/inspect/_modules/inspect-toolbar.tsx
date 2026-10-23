import ListRounded from '~icons/material-symbols/list-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { Button } from '@nyanpasu/ui/button'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import type { RuntimeInspection } from '@nyanpasu/rpc/types'
import PageToolbar from '../../_modules/page-toolbar'

/** Which snapshot the page shows. */
export type SnapshotView = 'latest' | 'applied'

function statusText(snapshot: RuntimeInspection) {
  if (snapshot.effective_pending) return m.inspect_effective_pending()
  return snapshot.applied ? m.inspect_applied() : m.inspect_not_applied()
}

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
  return (
    <PageToolbar
      title={m.inspect_title()}
      backTo="/main/profiles"
      context={
        snapshot && (
          <p
            data-slot="inspect-status"
            data-applied={snapshot.applied || undefined}
            className="text-on-surface-variant min-w-0 truncate text-sm"
          >
            <span className="bg-secondary-container text-on-secondary-container rounded-md px-1.5 py-0.5 text-xs font-medium">
              {statusText(snapshot)}
            </span>{' '}
            · {snapshot.target_core} · {m.inspect_revision()}{' '}
            {snapshot.revision}
          </p>
        )
      }
      actions={
        <>
          <Button
            type="button"
            className="@3xl:hidden"
            onClick={onShowSteps}
            data-slot="inspect-show-steps"
          >
            <ListRounded className="size-5" aria-hidden />
            {m.inspect_show_steps()}
          </Button>

          <SearchField
            className="w-48"
            value={fieldQuery}
            onValueChange={onFieldQueryChange}
            clearLabel={m.inspect_find_clear()}
            placeholder={m.inspect_find_field()}
            aria-label={m.inspect_find_field()}
            disabled={!snapshot}
            onKeyDown={(event) => {
              if (event.key !== 'Enter') return
              event.preventDefault()
              onCycleMatch(event.shiftKey ? -1 : 1)
            }}
          />

          <SegmentedButton
            className="w-72"
            size="sm"
            aria-label={m.inspect_snapshot_label()}
            value={view}
            onValueChange={(value) => {
              if (value) onViewChange(value as SnapshotView)
            }}
          >
            <SegmentedButtonItem value="latest">
              {m.inspect_snapshot_latest()}
            </SegmentedButtonItem>
            <SegmentedButtonItem value="applied">
              {m.inspect_snapshot_applied()}
            </SegmentedButtonItem>
          </SegmentedButton>

          <Button
            type="button"
            icon
            className="size-9"
            aria-label={m.inspect_refresh()}
            onClick={onRefresh}
          >
            <RefreshRounded className="size-5" aria-hidden />
          </Button>
        </>
      }
    />
  )
}
