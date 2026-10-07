import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'

export const NODE_SORTS = ['default', 'name', 'delay'] as const

export type NodeSort = (typeof NODE_SORTS)[number]

export type NodeView = { sort: NodeSort; hideUnavailable: boolean }

export const DEFAULT_NODE_VIEW: NodeView = {
  sort: 'default',
  hideUnavailable: false,
}

export const NODE_VIEW_KV_KEY = 'proxies-node-view'

const collator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
})

/** Coerces whatever the KV store holds into a NodeView. */
export function toNodeView(value: unknown): NodeView {
  const stored = (value ?? {}) as Partial<Record<keyof NodeView, unknown>>

  return {
    sort: NODE_SORTS.includes(stored.sort as NodeSort)
      ? (stored.sort as NodeSort)
      : DEFAULT_NODE_VIEW.sort,
    hideUnavailable:
      typeof stored.hideUnavailable === 'boolean'
        ? stored.hideUnavailable
        : DEFAULT_NODE_VIEW.hideUnavailable,
  }
}

const searchTerms = (query: string) =>
  query.toLowerCase().split(/\s+/).filter(Boolean)

const matchesTerms = (node: Proxy_Serialize, terms: string[]) => {
  const text = `${node.name} ${node.type}`.toLowerCase()

  return terms.every((term) => text.includes(term))
}

export function matchesNodeSearch(node: Proxy_Serialize, query: string) {
  return matchesTerms(node, searchTerms(query))
}

// Tested delays first (ascending), then untested, then failed.
const delayRank = (delay: number | undefined) =>
  delay === undefined
    ? Number.MAX_SAFE_INTEGER - 1
    : delay <= 0
      ? Number.MAX_SAFE_INTEGER
      : delay

/**
 * The members to show, in order. `delayOf` returns memberDelay for a name.
 */
export function visibleMembers({
  group,
  proxies,
  query,
  view,
  delayOf,
}: {
  group: ProxyGroup
  proxies: Proxies_Serialize
  query: string
  view: NodeView
  delayOf: (name: string) => number | undefined
}): string[] {
  const terms = searchTerms(query)
  const needsDelay = view.hideUnavailable || view.sort === 'delay'
  const members: { name: string; delay: number | undefined }[] = []

  for (const name of group.all) {
    const node = proxies.nodes[name]
    if (!node || !matchesTerms(node, terms)) continue

    const delay = needsDelay ? delayOf(name) : undefined
    if (
      view.hideUnavailable &&
      name !== group.now &&
      delay !== undefined &&
      delay <= 0
    ) {
      continue
    }

    members.push({ name, delay })
  }

  switch (view.sort) {
    case 'name':
      return members.map(({ name }) => name).sort(collator.compare)
    case 'delay':
      return members
        .map((member, index) => ({ ...member, index }))
        .sort(
          (a, b) =>
            delayRank(a.delay) - delayRank(b.delay) || a.index - b.index,
        )
        .map(({ name }) => name)
    default:
      return members.map(({ name }) => name)
  }
}
