import type {
  ClashProxiesQuery,
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
} from '@nyanpasu/query'

const PROXY_TYPES = ['Shadowsocks', 'Vmess', 'Trojan', 'Hysteria2']

const record = (name: string, type: string): ClashProxiesQueryProxyItem => ({
  name,
  type,
  udp: false,
  history: [],
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})

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
      ...record(name, type),
      udp: i % 2 === 0,
      xudp: i % 3 === 0,
      tfo: i % 5 === 0,
      alive: true,
      history: Array.from({ length: 10 }, (_, k) => ({
        time: new Date(1_700_000_000_000 + k * 1000).toISOString(),
        delay: (i * 7 + k * 13) % 600,
      })),
    }
  }

  const groupItems = Array.from(
    { length: groups },
    (_, g) =>
      ({
        name: `Group ${g}`,
        type: 'Selector',
        all: names,
        now: names[(g * 37) % nodes],
        fixed: null,
        testUrl: null,
        expectedStatus: null,
        hidden: false,
        icon: null,
        capabilities: { select: true, clearFixed: false },
      }) satisfies ClashProxiesQueryGroupItem,
  )

  for (const group of groupItems) {
    nodeMap[group.name] = {
      ...record(group.name, 'Selector'),
      all: group.all,
      now: group.now,
    }
  }

  return {
    global: groupItems[0],
    groups: groupItems,
    nodes: nodeMap,
  }
}
