import CloseRounded from '~icons/material-symbols/close-rounded'
import ViewColumnRounded from '~icons/material-symbols/view-column-rounded'
import { useDeferredValue, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { Button } from '@/components/ui/button'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { useDeleteClashConnections } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import ActiveViewer from './_modules/active-viewer'
import ClosedViewer from './_modules/closed-viewer'
import ConnectionsStatusTabs from './_modules/status-tabs'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { proxy, status = 'active' } = IndexRoute.useSearch()

  const navigate = IndexRoute.useNavigate()

  const [search, setSearch] = useState('')

  // Filtering, highlighting and re-sorting every connection is heavy; typing
  // stays responsive while the table catches up with the latest term.
  const deferredSearch = useDeferredValue(search)

  // Building the table from every connection and mounting its rows is the
  // bulk of opening the page. Router updates render synchronously, so the
  // table mounts in a deferred render instead: the page commits at once and
  // the rows render right after.
  const showTable = useDeferredValue(true, false)

  const [settingsOpen, setSettingsOpen] = useState(false)

  const deleteConnections = useDeleteClashConnections()

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null)
  })

  // Keyed by tab so switching starts the other table at the top.
  const scrollArea = (
    <ScrollArea
      key={status}
      className={cn(
        'min-h-0 flex-1',
        // Start the vertical scrollbar below the sticky h-9 table header;
        // Radix pins it to the top with an inline style.
        '[&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:top-9!',
        '[&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:h-auto',
      )}
      scrollbars="both"
      type="hover"
    >
      {showTable &&
        (status === 'closed' ? (
          <ClosedViewer
            search={deferredSearch}
            proxy={proxy}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
          />
        ) : (
          <ActiveViewer
            search={deferredSearch}
            proxy={proxy}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
          />
        ))}
    </ScrollArea>
  )

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      {status === 'closed' ? (
        scrollArea
      ) : (
        <RegisterContextMenu>
          <RegisterContextMenuTrigger asChild>
            {scrollArea}
          </RegisterContextMenuTrigger>

          <RegisterContextMenuContent>
            <ContextMenuItem onSelect={() => handleCloseAllConnections()}>
              <CloseRounded className="size-4" />
              <span>{m.connections_close_all_connections()}</span>
            </ContextMenuItem>
          </RegisterContextMenuContent>
        </RegisterContextMenu>
      )}

      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center gap-3 px-4"
        data-slot="connections-toolbar"
      >
        <ConnectionsStatusTabs
          value={status}
          onValueChange={(next) =>
            navigate({
              search: (previous) => ({ ...previous, status: next }),
            })
          }
        />

        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-0 flex-1 rounded-full px-4 text-sm outline-none',
          )}
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button onClick={() => setSettingsOpen(true)} icon>
              <ViewColumnRounded />
            </Button>
          </TooltipTrigger>

          <TooltipContent>{m.connections_column_settings()}</TooltipContent>
        </Tooltip>

        {status === 'active' && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button onClick={handleCloseAllConnections} icon>
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
