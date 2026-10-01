import { useDeferredValue, useMemo, useState } from 'react'
import { ScrollArea, useScrollAreaViewport } from '@/components/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  useClashProxies,
  useClashRules,
  type Bytes,
  type ClashRule,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import {
  RULE_ROW_HEIGHT,
  RuleRow,
  RulesHeader,
  type RuleSort,
} from './_modules/rule-row'
import {
  ruleLabel,
  useRuleStats,
  type RuleLiveStats,
} from './_modules/use-rule-stats'
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

type RuleItem = RuleEntry & {
  live?: RuleLiveStats
  total?: Bytes
}

const sortValue: Record<
  Exclude<RuleSort, 'index'>,
  (item: RuleItem) => number
> = {
  connections: (item) => item.live?.connections ?? 0,
  speed: (item) =>
    item.live ? item.live.downloadSpeed + item.live.uploadSpeed : 0,
  total: (item) => (item.total ? item.total.download + item.total.upload : 0),
}

const Viewer = ({ search, sort }: { search: string; sort: RuleSort }) => {
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

  const items = useMemo(() => {
    const withStats = entries.map<RuleItem>((entry) =>
      entry.first
        ? {
            ...entry,
            live: live.get(entry.label),
            total: totals?.get(entry.label),
          }
        : entry,
    )

    if (sort === 'index') {
      return withStats
    }

    const value = sortValue[sort]

    return withStats.sort((a, b) => value(b) - value(a) || a.index - b.index)
  }, [entries, live, totals, sort])

  const rowVirtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => RULE_ROW_HEIGHT,
    getItemKey: (index) => items[index].index,
    overscan: 10,
  })

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
              live={item.live}
              total={item.total}
              icon={groupIcons.get(item.rule.proxy)}
              search={search}
            />
          </div>
        )
      })}
    </div>
  )
}

function RouteComponent() {
  const [search, setSearch] = useState('')

  const [sort, setSort] = useState<RuleSort>('index')

  // Building the rule list and mounting its rows is the bulk of opening the
  // page. Router updates render synchronously, so the list mounts in a
  // deferred render instead: the page commits at once and the rows render
  // right after, into the scroll area's viewport already attached.
  const showRules = useDeferredValue(true, false)

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <div className="bg-mixed-background shrink-0">
        <div className="mx-auto max-w-7xl">
          <RulesHeader sort={sort} onSortChange={setSort} />
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1" type="hover">
        {showRules && <Viewer search={search} sort={sort} />}
      </ScrollArea>

      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center px-4"
        data-slot="rules-search"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 w-full rounded-full px-4 pr-10 text-sm outline-none',
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
