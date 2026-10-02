import type {
  Proxies_Serialize,
  ProxyGroupItem_Serialize,
  ProxyItem_Serialize,
} from '@nyanpasu/rpc/types'

export function getGroupSelectedDelay(
  group: ProxyGroupItem_Serialize,
  proxies: Proxies_Serialize,
): number | undefined {
  const visited = new Set<string>([group.name])
  let name = group.now

  while (name && !visited.has(name)) {
    visited.add(name)
    const node: ProxyItem_Serialize | undefined = proxies.nodes[name]
    if (!node) return undefined
    if (node.all) {
      // A node that is itself a group: keep following its selection.
      name = node.now
      continue
    }
    return node.history.at(-1)?.delay
  }

  return undefined
}
