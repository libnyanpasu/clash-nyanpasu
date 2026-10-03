import CloseRounded from '~icons/material-symbols/close-rounded'
import ViewColumnRounded from '~icons/material-symbols/view-column-rounded'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import ConnectionsFilters from './connections-filters'
import { isFiltered, type ConnectionsSelection } from './use-connection-rows'

export default function ConnectionsToolbar({
  start,
  tabs,
  selection,
  onSelectionChange,
  search,
  onSearchChange,
  onOpenSettings,
  onCloseAll,
}: {
  /** Rendered before every control, such as the way back after a jump. */
  start?: ReactNode
  tabs: ReactNode
  selection: ConnectionsSelection
  onSelectionChange: (selection: ConnectionsSelection) => void
  search: string
  onSearchChange: (search: string) => void
  onOpenSettings: () => void
  /** Absent while no live connection is listed. */
  onCloseAll?: () => void
}) {
  const filtered = isFiltered(selection)

  // Chips share the row while it is wide enough for every control, the
  // way back and the counts included; narrower, they take a row of
  // their own under the controls, as on the traffic page.
  return (
    <div
      className="bg-mixed-background @container shrink-0"
      data-slot="connections-toolbar"
    >
      <div className="flex min-h-16 flex-wrap items-center gap-3 px-4 py-3 @4xl:flex-nowrap @4xl:py-0">
        {start}

        {tabs}

        {filtered && (
          <ConnectionsFilters
            className="order-last basis-full @4xl:order-none @4xl:flex-1 @4xl:basis-0"
            selection={selection}
            onSelectionChange={onSelectionChange}
          />
        )}

        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            // Too narrow to read a term, it wraps instead of squeezing.
            'h-10 min-w-48 flex-1 rounded-full px-4 text-sm outline-none',
            filtered && '@4xl:w-56 @4xl:flex-none @6xl:w-72',
          )}
          data-slot="connections-search"
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
        />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button onClick={onOpenSettings} icon>
              <ViewColumnRounded />
            </Button>
          </TooltipTrigger>

          <TooltipContent>{m.connections_column_settings()}</TooltipContent>
        </Tooltip>

        {onCloseAll && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button onClick={onCloseAll} icon>
                <CloseRounded />
              </Button>
            </TooltipTrigger>

            <TooltipContent>
              {m.connections_close_all_connections()}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    </div>
  )
}
