import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { useMockConnectionsNow } from '@/hooks/use-mock-connections'
import { m } from '@/paraglide/messages'
import {
  useClashConnections,
  useTrafficActiveConnectionIds,
  useTrafficReport,
  useTrafficSummary,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { toTrafficFilters } from '../../_modules/traffic-filters'
import {
  isFiltered,
  mockActiveRows,
  mockClosedRows,
  type ConnectionsSelection,
} from './use-connection-rows'

export type ConnectionsScope = 'active' | 'closed'

function CountBadge({ count }: { count?: number }) {
  if (count === undefined) {
    return null
  }

  return (
    <span
      className={cn(
        'ml-1.5 inline-block rounded-full px-1.5 text-[11px] leading-4 tabular-nums',
        'bg-on-surface/8 text-on-surface-variant',
        'group-data-[state=on]:bg-on-secondary-container/12',
        'group-data-[state=on]:text-on-secondary-container',
      )}
    >
      {count.toLocaleString()}
    </span>
  )
}

export function StatusTabs({
  value,
  onValueChange,
  activeCount,
  closedCount,
}: {
  value: ConnectionsScope
  onValueChange: (value: ConnectionsScope) => void
  activeCount?: number
  closedCount?: number
}) {
  return (
    <SegmentedButton
      size="sm"
      className="w-auto shrink-0"
      value={value}
      onValueChange={(next) => {
        // Selecting the selected segment again clears a toggle group.
        if (next === 'active' || next === 'closed') {
          onValueChange(next)
        }
      }}
    >
      <SegmentedButtonItem
        value="active"
        className="flex-none whitespace-nowrap"
      >
        {m.connections_tab_active()}
        <CountBadge count={activeCount} />
      </SegmentedButtonItem>

      <SegmentedButtonItem
        value="closed"
        className="flex-none whitespace-nowrap"
      >
        {m.connections_tab_closed()}
        <CountBadge count={closedCount} />
      </SegmentedButtonItem>
    </SegmentedButton>
  )
}

// Reads the counts on its own, so each sample re-renders the tabs alone and
// not the page around them. Unfiltered, counts cover everything kept, whatever
// the search or proxy filter shows; filtered, they count the selection as the
// traffic report does.
export default function ConnectionsStatusTabs({
  selection,
  ...props
}: Omit<Parameters<typeof StatusTabs>[0], 'activeCount' | 'closedCount'> & {
  selection: ConnectionsSelection
}) {
  const { data: samples } = useClashConnections()

  const { data: summary } = useTrafficSummary()

  const mockNow = useMockConnectionsNow()

  const filtered = isFiltered(selection)

  const filters = toTrafficFilters(selection.filters)

  const { data: ids } = useTrafficActiveConnectionIds(filters, {
    enabled: filtered && mockNow === null,
  })

  const { data: report } = useTrafficReport(
    {
      query: { range: selection.range ?? 'all', scope: 'closed', filters },
      metric: 'connections',
      rankings: [],
      ranking_limit: 1,
      topology: null,
    },
    { enabled: filtered && mockNow === null },
  )

  const counts =
    mockNow !== null
      ? {
          active: mockActiveRows(mockNow, selection.filters).length,
          closed: mockClosedRows(mockNow, selection).length,
        }
      : filtered
        ? { active: ids?.length, closed: report?.total.connections }
        : {
            active: samples?.at(-1)?.connectionCount,
            closed: summary?.closed_connections,
          }

  return (
    <StatusTabs
      {...props}
      activeCount={counts.active}
      closedCount={counts.closed}
    />
  )
}
