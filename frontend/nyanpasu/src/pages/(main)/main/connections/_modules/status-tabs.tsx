import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { useMockConnectionsNow } from '@/hooks/use-mock-connections'
import { m } from '@/paraglide/messages'
import { useClashConnections, useTrafficSummary } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import {
  mockActiveConnections,
  mockClosedConnections,
} from './mock-connections'

export type ConnectionsStatus = 'active' | 'closed'

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
  value: ConnectionsStatus
  onValueChange: (value: ConnectionsStatus) => void
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
// not the page around them. Counts cover everything kept, whatever the
// search or proxy filter shows.
export default function ConnectionsStatusTabs(
  props: Omit<Parameters<typeof StatusTabs>[0], 'activeCount' | 'closedCount'>,
) {
  const { data: samples } = useClashConnections()

  const { data: summary } = useTrafficSummary()

  const mockNow = useMockConnectionsNow()

  return (
    <StatusTabs
      {...props}
      activeCount={
        mockNow === null
          ? samples?.at(-1)?.connectionCount
          : mockActiveConnections(mockNow).length
      }
      closedCount={
        mockNow === null
          ? summary?.closed_connections
          : mockClosedConnections(mockNow).length
      }
    />
  )
}
