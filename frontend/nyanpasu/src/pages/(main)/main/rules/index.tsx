import { memo, useCallback, useDeferredValue, useMemo } from 'react'
import { ScrollArea, useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import { keepReturn } from '@/components/router/cross-navigation'
import { ReturnButton } from '@/components/router/return-button'
import {
  useCrossNavigate,
  useEntryFocus,
} from '@/components/router/use-cross-navigate'
import { m } from '@/paraglide/messages'
import {
  useClashProxies,
  useClashRules,
  useCurrentProfileUid,
} from '@nyanpasu/query'
import { type Bytes, type ClashRule } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useFocusHighlight } from '../_modules/focus-highlight'
import { ruleLabel } from '../_modules/traffic-filters'
import { useSearchTerm } from '../_modules/use-search-term'
import { rankByValue } from './_modules/rank-by-value'
import {
  RULE_ROW_HEIGHT,
  RuleRow,
  RulesHeader,
  type RuleSort,
} from './_modules/rule-row'
import { useRuleStats, type RuleLiveStats } from './_modules/use-rule-stats'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/rules/')({
  component: RouteComponent,
})

const EMPTY_RULES: ClashRule[] = []

type RuleEntry = {
  /** 1-based position in the rule list. */
  index: number
  rule: ClashRule
  label: string
  /**
   * Only the first of the rules sharing a type and payload can match, so the
   * stats keyed by them are its alone.
   */
  first: boolean
}

const liveValue: Record<
  'connections' | 'speed',
  (live: RuleLiveStats) => number
> = {
  connections: (live) => live.connections,
  speed: (live) => live.downloadSpeed + live.uploadSpeed,
}

const totalValue = (total: Bytes) => total.download + total.upload

// Only the first of a label's rules has its stats and jumps, so a focus on
// the label is that rule's.
const focusKey = (entry: RuleEntry) => (entry.first ? entry.label : undefined)

// Memoized so a keystroke's urgent render skips the list; it re-renders with
// the deferred search term, or on its own stream and prop updates.
const Viewer = memo(function Viewer({
  search,
  sort,
}: {
  search: string
  sort: RuleSort
}) {
  const { data } = useClashRules()

  const rules = data?.rules ?? EMPTY_RULES

  const { proxy } = IndexRoute.useSearch()

  const { viewportRef } = useScrollAreaViewport()

  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const groupIcons = useMemo(
    () => new Map(proxies?.groups.map((group) => [group.name, group.icon])),
    [proxies],
  )

  // Every rule is queried, whatever the filter, so sorting sees them all.
  const { live, totals } = useRuleStats(rules)

  const entries = useMemo(() => {
    const seen = new Set<string>()

    const all = rules.map<RuleEntry>((rule, index) => {
      const label = ruleLabel(rule.type, rule.payload)
      const first = !seen.has(label)

      seen.add(label)

      return { index: index + 1, rule, label, first }
    })

    const proxyFiltered = proxy
      ? all.filter(({ rule }) => rule.proxy === proxy)
      : all

    if (!search.trim()) {
      return proxyFiltered
    }

    const searchLower = search.toLowerCase()

    return proxyFiltered.filter(({ rule }) => {
      return (
        rule.type?.toLowerCase().includes(searchLower) ||
        rule.payload?.toLowerCase().includes(searchLower) ||
        rule.proxy?.toLowerCase().includes(searchLower)
      )
    })
  }, [rules, proxy, search])

  // Live stats change with every sample and totals with every poll, so each
  // sort depends only on the stats it ranks by.
  const byLive = useMemo(() => {
    if (sort !== 'connections' && sort !== 'speed') {
      return undefined
    }

    const value = liveValue[sort]

    // Most rules have no live stats, so only the few that do are sorted.
    return rankByValue(entries, (entry) => {
      const stats = entry.first ? live.get(entry.label) : undefined
      return stats ? value(stats) : 0
    })
  }, [entries, live, sort])

  const byTotal = useMemo(() => {
    if (sort !== 'total') {
      return undefined
    }

    return rankByValue(entries, (entry) => {
      const total = entry.first ? totals?.get(entry.label) : undefined
      return total ? totalValue(total) : 0
    })
  }, [entries, totals, sort])

  const items = byLive ?? byTotal ?? entries

  const rowVirtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => RULE_ROW_HEIGHT,
    getItemKey: (index) => items[index].index,
    overscan: 10,
  })

  const crossNavigate = useCrossNavigate()

  const profile = useCurrentProfileUid()

  const handleViewConnections = useCallback(
    (label: string) =>
      crossNavigate({
        from: 'rules',
        originFocus: label,
        to: {
          to: '/main/connections',
          search: { scope: 'active', filters: [{ d: 'rule', v: label }] },
        },
      }),
    [crossNavigate],
  )

  // The same profile filter as the rule totals, so the page's total is the
  // cell's.
  const handleViewUsage = useCallback(
    (label: string) =>
      crossNavigate({
        from: 'rules',
        originFocus: label,
        to: {
          to: '/main/topology',
          search: {
            range: 'all',
            scope: 'all',
            filters: [
              { d: 'profile', v: profile ?? '' },
              { d: 'rule', v: label },
            ],
          },
        },
      }),
    [crossNavigate, profile],
  )

  const focus = useEntryFocus()

  const focusedLabel = useFocusHighlight(focus, items, focusKey, rowVirtualizer)

  return (
    <div
      className="relative mx-auto max-w-7xl"
      data-slot="rules-virtual-container"
      style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
    >
      {rowVirtualizer.getVirtualItems().map((virtualRow) => {
        const item = items[virtualRow.index]

        return (
          <div
            key={virtualRow.key}
            className="absolute inset-x-0 top-0 select-text"
            style={{ transform: `translateY(${virtualRow.start}px)` }}
          >
            <RuleRow
              index={item.index}
              rule={item.rule}
              label={item.label}
              live={item.first ? live.get(item.label) : undefined}
              total={item.first ? totals?.get(item.label) : undefined}
              icon={groupIcons.get(item.rule.proxy)}
              search={search}
              focused={item.first && item.label === focusedLabel}
              onViewConnections={item.first ? handleViewConnections : undefined}
              onViewUsage={item.first ? handleViewUsage : undefined}
            />
          </div>
        )
      })}
    </div>
  )
})

function RouteComponent() {
  const { q, sort = 'index' } = IndexRoute.useSearch()

  const navigate = IndexRoute.useNavigate()

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

  // Filtering and highlighting every rule is heavy; typing stays responsive
  // while the list catches up with the latest term.
  const deferredSearch = useDeferredValue(search)

  const handleSortChange = (next: RuleSort) =>
    navigate({
      search: (previous) => ({
        ...previous,
        sort: next === 'index' ? undefined : next,
      }),
      replace: true,
      state: keepReturn,
    })

  // Building the rule list and mounting its rows is the bulk of opening the
  // page. Router updates render synchronously, so the list mounts in a
  // deferred render instead: the page commits at once and the rows render
  // right after, into the scroll area's viewport already attached.
  const showRules = useDeferredValue(true, false)

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <div className="bg-mixed-background shrink-0">
        <div className="mx-auto max-w-7xl">
          <RulesHeader sort={sort} onSortChange={handleSortChange} />
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1" type="hover">
        {showRules && <Viewer search={deferredSearch} sort={sort} />}
      </ScrollArea>

      <div
        className="bg-mixed-background @container flex h-16 shrink-0 items-center gap-3 px-4"
        data-slot="rules-search"
      >
        <ReturnButton />

        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-0 flex-1 rounded-full px-4 pr-10 text-sm outline-none',
          )}
          data-slot="rules-search-input-field"
          placeholder={m.rules_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
    </div>
  )
}
