import type {
  ClashProxiesQuery,
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
} from '@nyanpasu/interface'

const PROXY_TYPES = ['Shadowsocks', 'Vmess', 'Trojan', 'Hysteria2']

// Every group lists every node, like a subscription whose groups all select
// from the full node list. Names carry a flag emoji, as real ones often do.
export function createProxiesFixture({
  groups,
  nodes,
}: {
  groups: number
  nodes: number
}): ClashProxiesQuery {
  const nodeMap: ClashProxiesQuery['nodes'] = {}
  const names: string[] = []

  for (let i = 0; i < nodes; i++) {
    const type = PROXY_TYPES[i % PROXY_TYPES.length]
    const name = `🇯🇵 Node ${i} | ${type} relay-${i % 17}`
    names.push(name)
    nodeMap[name] = {
      name,
      type,
      udp: i % 2 === 0,
      xudp: i % 3 === 0,
      tfo: i % 5 === 0,
      history: Array.from({ length: 10 }, (_, k) => ({
        time: new Date(1_700_000_000_000 + k * 1000).toISOString(),
        delay: (i * 7 + k * 13) % 600,
      })),
      all: null,
      now: null,
      provider: null,
      alive: true,
      hidden: false,
    } satisfies ClashProxiesQueryProxyItem
  }

  const groupItems = Array.from(
    { length: groups },
    (_, g) =>
      ({
        name: `Group ${g}`,
        type: 'Selector',
        udp: true,
        history: [],
        all: names,
        now: names[(g * 37) % nodes],
        provider: null,
        alive: true,
        hidden: false,
      }) satisfies ClashProxiesQueryGroupItem,
  )

  for (const group of groupItems) {
    nodeMap[group.name] = group
  }

  return {
    global: groupItems[0],
    groups: groupItems,
    nodes: nodeMap,
  }
}
