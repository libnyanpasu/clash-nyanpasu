import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
} from '@nyanpasu/rpc/types'

/** The delay source of a group that tests with the app's default URL. */
export const DEFAULT_DELAY_KEY = ''

/** Anything that names the delay source its members are shown with. */
export type DelayKeySource = { delayKey: string }

/**
 * A snapshot with its groups' delay keys. `delayKeys` is complete where
 * `groups` is not: the proxies query drops hidden groups from `groups`, yet a
 * hidden group can still sit between a member and its leaf.
 */
export type DelayKeyedProxies = Proxies_Serialize & {
  delayKeys?: Readonly<Record<string, string>>
}

/** Every group's delay key by name. */
export function collectDelayKeys(
  proxies: Proxies_Serialize,
): Record<string, string> {
  return Object.fromEntries(
    [...(proxies.global ? [proxies.global] : []), ...proxies.groups].map(
      (group) => [group.name, group.delayKey],
    ),
  )
}

const delayKeysBySnapshot = new WeakMap<
  Proxies_Serialize,
  Readonly<Record<string, string>>
>()

/** The delay key of the group named `group`, else the default's. */
export function groupDelayKey(
  proxies: DelayKeyedProxies | undefined,
  group: string | undefined,
): string {
  if (!proxies || group === undefined) {
    return DEFAULT_DELAY_KEY
  }

  let keys = proxies.delayKeys

  if (!keys) {
    keys = delayKeysBySnapshot.get(proxies)

    if (!keys) {
      keys = collectDelayKeys(proxies)
      delayKeysBySnapshot.set(proxies, keys)
    }
  }

  return Object.hasOwn(keys, group) ? keys[group] : DEFAULT_DELAY_KEY
}

/**
 * A node's samples for one delay source. The backend sends them per source in
 * `extra`, keyed by {@link DelayKeySource.delayKey} and never by a test URL,
 * and already picks the newest of the raw and normalized records of a URL.
 * Clash-rs (`extra: {}`) and Meow (no `extra`) only have `history`.
 */
export function nodeDelayHistory(
  node: Proxy_Serialize,
  delayKey: string,
): DelayHistory[] {
  return node.extra?.[delayKey]?.history ?? node.history
}

/** The latest sample's delay; `0` is a failed test, `undefined` untested. */
export function latestDelay(
  node: Proxy_Serialize | undefined,
  delayKey: string,
): number | undefined {
  return node ? nodeDelayHistory(node, delayKey).at(-1)?.delay : undefined
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
 * the way the group that directly holds that leaf tests. The history keeps
 * the identity of the snapshot's own arrays, so a memoized card can rely on it.
 */
export function memberState(
  member: string,
  group: DelayKeySource,
  proxies: DelayKeyedProxies,
): MemberState {
  const node = proxies.nodes[member]
  if (!node?.all) {
    const history = node ? nodeDelayHistory(node, group.delayKey) : NO_HISTORY
    return { delay: history.at(-1)?.delay, history }
  }

  const chain = resolveChain(member, proxies)
  const history = chain.leaf
    ? nodeDelayHistory(chain.leaf, groupDelayKey(proxies, chain.parent))
    : NO_HISTORY
  return { delay: history.at(-1)?.delay, leaf: chain.leaf?.name, history }
}

/** A member's delay as its group sees it; see {@link memberState}. */
export function memberDelay(
  member: string,
  group: DelayKeySource,
  proxies: DelayKeyedProxies,
): number | undefined {
  return memberState(member, group, proxies).delay
}
