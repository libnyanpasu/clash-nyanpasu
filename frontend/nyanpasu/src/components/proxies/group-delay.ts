import type {
  Proxies_Serialize,
  ProxyGroupItem_Serialize,
  ProxyItem_Serialize,
} from '@nyanpasu/interface'

export function getGroupSelectedDelay(
  group: ProxyGroupItem_Serialize,
  proxies: Proxies_Serialize,
): number | undefined {
  const groups = [proxies.global, ...proxies.groups]
  const visited = new Set<string>([group.name])
  let name = group.now
  let members = group.all

  while (name && !visited.has(name)) {
    visited.add(name)
    const nestedGroup = groups.find((item) => item.name === name)
    if (nestedGroup) {
      name = nestedGroup.now
      members = nestedGroup.all
      continue
    }

    const proxy: ProxyItem_Serialize | undefined =
      members.find((item) => item.name === name) ?? proxies.records[name]
    if (!proxy) return undefined
    if (proxy.all) {
      name = proxy.now
      // Hidden groups are only present in records; prefer live member histories.
      members = groups.flatMap((item) => item.all)
      continue
    }
    return proxy.history.at(-1)?.delay
  }

  return undefined
}
