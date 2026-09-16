import FilterAltRounded from '~icons/material-symbols/filter-alt-rounded'
import { useMemo, useState } from 'react'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { ScrollArea } from '@/components/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { containsSearchTerm } from '@/utils'
import { useClashConnections, useClashRules } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import TopologyView from './_modules/topology-view'

export const Route = createFileRoute('/(main)/main/topology')({
  component: RouteComponent,
  validateSearch: z.object({ proxy: z.string().optional().nullable() }),
})

function RouteComponent() {
  const [search, setSearch] = useState('')
  const { proxy } = Route.useSearch()
  const navigate = Route.useNavigate()
  const { data, isLoading, error } = useClashConnections()
  const { data: rules } = useClashRules()
  const proxies = useMemo(
    () => [
      ...new Set(
        rules?.rules
          .map((rule) => rule.proxy)
          .filter((name): name is string => !!name) ?? [],
      ),
    ],
    [rules],
  )
  const connections = useMemo(
    () =>
      (data.at(-1)?.connections ?? [])
        .filter((connection) => !proxy || connection.chains.includes(proxy))
        .filter(
          (connection) => !search || containsSearchTerm(connection, search),
        ),
    [data, proxy, search],
  )
  const selectProxy = (name?: string) => navigate({ search: { proxy: name } })

  return (
    <div className="divide-outline-variant flex min-h-0 min-w-0 flex-1 flex-col divide-y overflow-hidden">
      <ScrollArea className="min-h-0 flex-1">
        <TopologyView
          connections={connections}
          filterKey={JSON.stringify([proxy, search])}
          isLoading={isLoading}
          error={error}
        />
      </ScrollArea>
      <div
        className="bg-mixed-background flex shrink-0 flex-wrap items-center gap-3 p-4"
        data-slot="topology-toolbar"
      >
        <input
          type="search"
          className="bg-surface-variant focus-visible:ring-primary dark:bg-surface-variant/30 h-10 min-w-0 flex-1 rounded-full px-4 text-sm outline-none focus-visible:ring-2"
          aria-label={m.connections_search_placeholder()}
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="stroked"
              className="flex max-w-full items-center gap-2"
            >
              <FilterAltRounded className="size-5 shrink-0" />
              <span className="max-w-48 truncate">
                {proxy || m.connections_all_connections()}
              </span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="max-h-72 overflow-y-auto">
            <DropdownMenuCheckboxItem
              checked={!proxy}
              onSelect={() => {
                selectProxy()
              }}
            >
              {m.connections_all_connections()}
            </DropdownMenuCheckboxItem>
            {proxies.map((name) => (
              <DropdownMenuCheckboxItem
                key={name}
                checked={proxy === name}
                onSelect={() => {
                  selectProxy(name)
                }}
              >
                {name}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  )
}
