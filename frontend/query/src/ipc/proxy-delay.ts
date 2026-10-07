import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
} from '@nyanpasu/rpc/types'

/** Anything that names the URL its members are tested with. */
export type TestUrlSource = { testUrl?: string | null }

/** The URL a group's members are tested with: its own, else the default. */
export function groupTestUrl(
  group: TestUrlSource | undefined,
  defaultUrl: string,
): string {
  return group?.testUrl || defaultUrl
}

/** `url` as the backend sends it (`Url::as_str`), which Mihomo keys `extra` by. */
function normalizeUrl(url: string): string {
  // `URL.canParse` is missing from WebKit before 17 (macOS 12/13 webviews).
  try {
    return new URL(url).href
  } catch {
    return url
  }
}

function lastSampleTime(entry: { history: DelayHistory[] }): number {
  const time = Date.parse(entry.history.at(-1)?.time ?? '')
  return Number.isNaN(time) ? -Infinity : time
}

/**
 * A node's samples for one URL. Mihomo keeps them per URL in `extra`, keyed by
 * the URL it was asked to test: the backend's normalized form, or the group's
 * raw `testUrl` for its own health checks. Both keys are read and the newer
 * samples win. Clash-rs (`extra: {}`) and Meow (no `extra`) only have `history`.
 */
export function nodeDelayHistory(
  node: Proxy_Serialize,
  url: string,
): DelayHistory[] {
  const raw = node.extra?.[url]
  const normalized = node.extra?.[normalizeUrl(url)]
  const entry =
    raw && normalized && lastSampleTime(normalized) > lastSampleTime(raw)
      ? normalized
      : (raw ?? normalized)
  return entry?.history ?? node.history
}

/** The latest sample's delay; `0` is a failed test, `undefined` untested. */
export function latestDelay(
  node: Proxy_Serialize | undefined,
  url: string,
): number | undefined {
  return node ? nodeDelayHistory(node, url).at(-1)?.delay : undefined
}

export type Chain = {
  /** Every name after `start`, down to and including the leaf. */
  path: string[]
  /** Absent when the chain is unselected, cycles, or names a missing node. */
  leaf?: Proxy_Serialize
  /** The group that directly holds the last name in `path`. */
  parent: string
}

/** Follows `start`'s selection through nested groups to a leaf node. */
export function resolveChain(start: string, proxies: Proxies_Serialize): Chain {
  const visited = new Set([start])
  const path: string[] = []
  let parent = start
  let name = proxies.nodes[start]?.now

  while (name && !visited.has(name)) {
    const node: Proxy_Serialize | undefined = proxies.nodes[name]
    path.push(name)
    if (!node) {
      return { path, parent }
    }
    if (!node.all) {
      return { path, leaf: node, parent }
    }
    visited.add(name)
    parent = name
    name = node.now
  }

  return { path, parent }
}

export type MemberState = {
  /** The delay the group sees; a nested group reports its leaf's. */
  delay: number | undefined
  /** The node a nested group member currently resolves to. */
  leaf?: string
  /** The samples `delay` is the latest of. */
  history: DelayHistory[]
}

const NO_HISTORY: DelayHistory[] = []

/**
 * A member as its group sees it: a nested group reports its leaf, tested
 * against the URL of the group that directly holds that leaf. The history keeps
 * the identity of the snapshot's own arrays, so a memoized card can rely on it.
 */
export function memberState(
  member: string,
  group: TestUrlSource,
  proxies: Proxies_Serialize,
  defaultUrl: string,
): MemberState {
  const node = proxies.nodes[member]
  if (!node?.all) {
    const history = node
      ? nodeDelayHistory(node, groupTestUrl(group, defaultUrl))
      : NO_HISTORY
    return { delay: history.at(-1)?.delay, history }
  }

  const chain = resolveChain(member, proxies)
  const history = chain.leaf
    ? nodeDelayHistory(
        chain.leaf,
        groupTestUrl(proxies.nodes[chain.parent], defaultUrl),
      )
    : NO_HISTORY
  return { delay: history.at(-1)?.delay, leaf: chain.leaf?.name, history }
}

/** A member's delay as its group sees it; see {@link memberState}. */
export function memberDelay(
  member: string,
  group: TestUrlSource,
  proxies: Proxies_Serialize,
  defaultUrl: string,
): number | undefined {
  return memberState(member, group, proxies, defaultUrl).delay
}
