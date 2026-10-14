import CloseRounded from '~icons/material-symbols/close-rounded'
import ViewColumnRounded from '~icons/material-symbols/view-column-rounded'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { SearchField } from '@nyanpasu/ui/search-field'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import {
  dataPageFloatingGroupVariants,
  dataPageSearchVariants,
  DataPageToolbar,
} from '../../_modules/data-page'
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
  tabs?: ReactNode
  selection: ConnectionsSelection
  onSelectionChange: (selection: ConnectionsSelection) => void
  search: string
  onSearchChange: (search: string) => void
  onOpenSettings: () => void
  /** Absent while no live connection is listed. */
  onCloseAll?: () => void
}) {
  const filtered = isFiltered(selection)

  // Removable filters have their own floating row beneath the controls.
  return (
    <DataPageToolbar className="@container" data-slot="connections-toolbar">
      {start}

      {filtered && (
        <ConnectionsFilters
          className={cn(
            dataPageFloatingGroupVariants(),
            'order-last basis-full',
          )}
          selection={selection}
          onSelectionChange={onSelectionChange}
        />
      )}

      <SearchField
        className={cn(
          // Too narrow to read a term, it wraps instead of squeezing.
          dataPageSearchVariants(),
          'min-w-[min(12rem,100%)] flex-1 basis-48',
        )}
        data-slot="connections-search"
        placeholder={m.connections_search_placeholder()}
        value={search}
        onValueChange={onSearchChange}
        clearLabel={m.common_clear()}
      />

      {tabs && (
        <div
          className={cn(
            dataPageFloatingGroupVariants(),
            'max-w-full overflow-x-auto',
          )}
          data-slot="data-page-controls"
        >
          {tabs}
        </div>
      )}

      <div
        className={cn(dataPageFloatingGroupVariants(), 'ml-auto shrink-0')}
        data-slot="connections-actions"
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              onClick={onOpenSettings}
              aria-label={m.connections_column_settings()}
              className="rounded-full"
              icon
            >
              <ViewColumnRounded />
            </Button>
          </TooltipTrigger>

          <TooltipContent>{m.connections_column_settings()}</TooltipContent>
        </Tooltip>

        {onCloseAll && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                onClick={onCloseAll}
                aria-label={m.connections_close_all_connections()}
                className="rounded-full"
                icon
              >
                <CloseRounded />
              </Button>
            </TooltipTrigger>

            <TooltipContent>
              {m.connections_close_all_connections()}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    </DataPageToolbar>
  )
}
