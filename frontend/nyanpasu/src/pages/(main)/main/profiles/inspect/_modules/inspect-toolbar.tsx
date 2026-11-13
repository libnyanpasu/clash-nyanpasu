import AccountTreeRounded from '~icons/material-symbols/account-tree-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import ListRounded from '~icons/material-symbols/list-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import ScheduleRounded from '~icons/material-symbols/schedule-rounded'
import type { ReactNode } from 'react'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import type { RuntimeInspection } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import type { LinkProps } from '@tanstack/react-router'
import { RowAction, searchPillClassName } from '../../_modules/compact-controls'
import PageToolbar from '../../_modules/page-toolbar'

/** Which snapshot the page shows. */
export type SnapshotView = 'latest' | 'applied'

/** How the page lays the steps out. */
export type StepLayout = 'list' | 'graph'

function statusText(snapshot: RuntimeInspection) {
  if (snapshot.effective_pending) return m.inspect_effective_pending()
  return snapshot.applied ? m.inspect_applied() : m.inspect_not_applied()
}

const snapshotItemClassName = 'flex-none px-3.5 text-[13px] whitespace-nowrap'
const layoutItemClassName = 'flex-none px-3'

/**
 * The control row of the page: which snapshot to show, what it is, a search
 * over the fields the steps changed, and a refresh.
 */
export default function InspectToolbar({
  snapshot,
  title,
  titleHidden,
  backTo,
  snapshotControls,
  status,
  layout,
  onLayoutChange,
  fieldQuery,
  onFieldQueryChange,
  onCycleMatch,
  onRefresh,
  onShowSteps,
}: {
  /** Absent while loading and when there is no snapshot. */
  snapshot?: RuntimeInspection | null
  title?: string
  titleHidden?: boolean
  backTo?: LinkProps['to']
  snapshotControls?: ReactNode
  status?: ReactNode
  layout: StepLayout
  onLayoutChange: (layout: StepLayout) => void
  fieldQuery: string
  onFieldQueryChange: (query: string) => void
  /** Enter selects the next step with a matching field; Shift+Enter, the previous. */
  onCycleMatch: (delta: 1 | -1) => void
  onRefresh?: () => void
  /** Opens the step list where it does not fit beside the details. */
  onShowSteps: () => void
}) {
  return (
    <PageToolbar
      title={title ?? m.inspect_title()}
      titleHidden={titleHidden}
      backTo={backTo}
      // Narrow: the title row carries the steps and refresh, and the snapshot
      // switch and the search share the row below. Wide: one row.
      contextClassName="contents"
      actionsClassName="contents"
      context={
        <>
          {snapshotControls}

          <SegmentedButton
            className="order-5 h-8 w-auto shrink-0 text-[13px] @3xl:order-2"
            aria-label={m.inspect_view_label()}
            value={layout}
            onValueChange={(value) => {
              if (value) onLayoutChange(value as StepLayout)
            }}
          >
            <SegmentedButtonItem
              value="list"
              hideIndicator
              aria-label={m.inspect_view_list()}
              className={layoutItemClassName}
            >
              <ListRounded className="size-4" aria-hidden />
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="graph"
              hideIndicator
              aria-label={m.inspect_view_graph()}
              className={layoutItemClassName}
            >
              <AccountTreeRounded className="size-4" aria-hidden />
            </SegmentedButtonItem>
          </SegmentedButton>

          {status}
        </>
      }
      actions={
        <>
          <span aria-hidden className="order-4 h-0 basis-full @3xl:hidden" />

          <RowAction
            label={m.inspect_show_steps()}
            className="order-2 ml-auto size-10 @3xl:hidden"
            onClick={onShowSteps}
            data-slot="inspect-show-steps"
          >
            <ListRounded className="size-5" aria-hidden />
          </RowAction>

          <SearchField
            className={cn(
              'order-6 min-w-0 flex-1 @3xl:order-4 @3xl:ml-auto @3xl:w-[250px] @3xl:flex-none',
              searchPillClassName,
            )}
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

          {onRefresh && (
            <RowAction
              label={m.inspect_refresh()}
              className="order-3 size-10 @3xl:order-5"
              onClick={onRefresh}
            >
              <RefreshRounded className="size-5" aria-hidden />
            </RowAction>
          )}
        </>
      }
    />
  )
}

export function SnapshotSwitch({
  view,
  onViewChange,
}: {
  view: SnapshotView
  onViewChange: (view: SnapshotView) => void
}) {
  return (
    <SegmentedButton
      className="order-5 h-8 w-auto shrink-0 text-[13px] @3xl:order-2"
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
  )
}

export function SnapshotStatus({ snapshot }: { snapshot: RuntimeInspection }) {
  const settled = snapshot.applied && !snapshot.effective_pending
  const StatusIcon = settled ? CheckRounded : ScheduleRounded

  return (
    <p
      data-slot="inspect-status"
      data-applied={snapshot.applied || undefined}
      className={cn(
        'bg-surface-variant text-on-surface-variant hidden h-7 min-w-0 items-center gap-1.5',
        'rounded-lg px-2.5 text-xs font-medium whitespace-nowrap @3xl:order-3 @3xl:inline-flex',
        // Shrinks before the controls wrap onto a second row.
        '@3xl:max-w-max @3xl:flex-1 @3xl:basis-0',
      )}
    >
      <StatusIcon
        aria-hidden
        className={cn('size-4 shrink-0', settled ? 'text-ok' : 'text-warn')}
      />

      <span className="truncate">
        {statusText(snapshot)} · {snapshot.target_core} · {m.inspect_revision()}{' '}
        {snapshot.revision}
      </span>
    </p>
  )
}
