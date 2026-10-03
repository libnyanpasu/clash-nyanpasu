import CloseRounded from '~icons/material-symbols/close-rounded'
import { useCallback, useDeferredValue, useMemo, useState } from 'react'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { keepReturn } from '@/components/router/cross-navigation'
import { ReturnButton } from '@/components/router/return-button'
import {
  useCrossNavigate,
  useEntryFocus,
} from '@/components/router/use-cross-navigate'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import { useDeleteClashConnections } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import type { SearchFilter } from '../_modules/traffic-filters'
import { useSearchTerm } from '../_modules/use-search-term'
import ActiveViewer from './_modules/active-viewer'
import AllViewer from './_modules/all-viewer'
import ClosedViewer from './_modules/closed-viewer'
import ConnectionsToolbar from './_modules/connections-toolbar'
import ConnectionsStatusTabs from './_modules/status-tabs'
import type { ConnectionDetail } from './_modules/table-row'
import {
  useTabFocus,
  type ConnectionsSelection,
} from './_modules/use-connection-rows'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
})

const NO_FILTERS: SearchFilter[] = []

function RouteComponent() {
  const {
    proxy,
    scope = 'active',
    range,
    filters = NO_FILTERS,
    q,
  } = IndexRoute.useSearch()

  const navigate = IndexRoute.useNavigate()

  // One object while the URL keeps it, so the memoized viewers skip renders.
  const selection = useMemo<ConnectionsSelection>(
    () => ({ range, filters }),
    [range, filters],
  )

  const handleSelectionChange = (next: ConnectionsSelection) =>
    navigate({
      search: (previous) => ({
        ...previous,
        range: next.range,
        filters: next.filters.length > 0 ? next.filters : undefined,
      }),
      replace: true,
      state: keepReturn,
    })

  const writeQuery = useCallback(
    (next: string | undefined) =>
      navigate({
        search: (previous) => ({ ...previous, q: next }),
        replace: true,
        state: keepReturn,
      }),
    [navigate],
  )

  const [search, setSearch] = useSearchTerm(q, writeQuery)

  // Filtering, highlighting and re-sorting every connection is heavy; typing
  // stays responsive while the table catches up with the latest term.
  const deferredSearch = useDeferredValue(search)

  // Building the table from every connection and mounting its rows is the
  // bulk of opening the page. Router updates render synchronously, so the
  // table mounts in a deferred render instead: the page commits at once and
  // the rows render right after.
  const showTable = useDeferredValue(true, false)

  const [settingsOpen, setSettingsOpen] = useState(false)

  const crossNavigate = useCrossNavigate()

  // No proxy or search on the rules page, so nothing hides the rule.
  const handleLocateRule = useCallback(
    (detail: ConnectionDetail) =>
      crossNavigate({
        from: 'connections',
        originFocus: detail.rowId,
        targetFocus: detail.ruleLabel,
        to: { to: '/main/rules', search: {} },
      }),
    [crossNavigate],
  )

  const handleViewRuleUsage = useCallback(
    (detail: ConnectionDetail) =>
      crossNavigate({
        from: 'connections',
        originFocus: detail.rowId,
        to: {
          to: '/main/topology',
          search: { filters: [{ d: 'rule', v: detail.ruleLabel }] },
        },
      }),
    [crossNavigate],
  )

  const entryFocus = useEntryFocus()

  // The row a jump left from, highlighted when returning to it.
  const focusRowId = useTabFocus(scope, entryFocus)

  const deleteConnections = useDeleteClashConnections()

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null)
  })

  // Keyed by tab so switching starts the other table at the top.
  const scrollArea = (
    <ScrollArea
      key={scope}
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
        (scope === 'all' ? (
          <AllViewer
            search={deferredSearch}
            proxy={proxy}
            selection={selection}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={handleLocateRule}
            onViewRuleUsage={handleViewRuleUsage}
            focusRowId={focusRowId}
          />
        ) : scope === 'closed' ? (
          <ClosedViewer
            search={deferredSearch}
            proxy={proxy}
            selection={selection}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={handleLocateRule}
            onViewRuleUsage={handleViewRuleUsage}
            focusRowId={focusRowId}
          />
        ) : (
          <ActiveViewer
            search={deferredSearch}
            proxy={proxy}
            filters={filters}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={handleLocateRule}
            onViewRuleUsage={handleViewRuleUsage}
            focusRowId={focusRowId}
          />
        ))}
    </ScrollArea>
  )

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      {scope === 'closed' ? (
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

      <ConnectionsToolbar
        start={<ReturnButton />}
        tabs={
          <ConnectionsStatusTabs
            value={scope}
            onValueChange={(next) =>
              navigate({
                search: (previous) => ({ ...previous, scope: next }),
                state: keepReturn,
              })
            }
            selection={selection}
          />
        }
        selection={selection}
        onSelectionChange={handleSelectionChange}
        search={search}
        onSearchChange={setSearch}
        onOpenSettings={() => setSettingsOpen(true)}
        onCloseAll={scope === 'closed' ? undefined : handleCloseAllConnections}
      />
    </div>
  )
}
