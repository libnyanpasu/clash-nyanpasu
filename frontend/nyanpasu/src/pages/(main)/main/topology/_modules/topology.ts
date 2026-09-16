import type { ClashConnectionItem } from '@nyanpasu/interface'

export type TopologyMetric = 'connections' | 'bytes'
export type TopologyNode = {
  id: string
  label: string | null
  layer: number
  count: number
  bytes: number
  connectionIds: Set<string>
  other: boolean
}
export type TopologyLink = {
  source: string
  target: string
  count: number
  bytes: number
  connectionIds: Set<string>
}

// The intermediate column preserves all reported groups, in traversal order.
// It describes the core's logical chain, not physical network hops.
function dimensions(connection: ClashConnectionItem): (string | null)[] {
  const metadata = connection.metadata
  return [
    metadata?.process || metadata?.sourceIP || null,
    connection.rule
      ? [connection.rule, connection.rulePayload].filter(Boolean).join(': ')
      : null,
    connection.chains.length > 1
      ? connection.chains.slice(1).reverse().join(' → ')
      : null,
    connection.chains[0] || null,
  ]
}

export function buildTopology(
  connections: ClashConnectionItem[],
  metric: TopologyMetric,
) {
  const limit = 7
  const columns = Array.from(
    { length: 4 },
    () => new Map<string, TopologyNode>(),
  )
  const paths = new Map<string, string[]>()
  const value = (item: { count: number; bytes: number }) =>
    metric === 'bytes' ? item.bytes : item.count

  for (const connection of connections) {
    const bytes =
      Math.max(0, connection.upload) + Math.max(0, connection.download)
    paths.set(
      connection.id,
      dimensions(connection).map((label, layer) => {
        const id = JSON.stringify([layer, label])
        let node = columns[layer].get(id)
        if (!node) {
          node = {
            id,
            label,
            layer,
            count: 0,
            bytes: 0,
            connectionIds: new Set(),
            other: false,
          }
          columns[layer].set(id, node)
        }
        node.count++
        node.bytes += bytes
        node.connectionIds.add(connection.id)
        return id
      }),
    )
  }

  const remap = new Map<string, string>()
  const layers = columns.map((column, layer) => {
    const ranked = [...column.values()].sort(
      (a, b) => value(b) - value(a) || a.id.localeCompare(b.id),
    )
    if (ranked.length <= limit) return ranked
    const visible = ranked.slice(0, limit - 1)
    const other: TopologyNode = {
      id: `other:${layer}`,
      label: null,
      layer,
      count: 0,
      bytes: 0,
      connectionIds: new Set(),
      other: true,
    }
    for (const node of ranked.slice(limit - 1)) {
      remap.set(node.id, other.id)
      other.count += node.count
      other.bytes += node.bytes
      for (const id of node.connectionIds) other.connectionIds.add(id)
    }
    return [...visible, other]
  })

  const links = new Map<string, TopologyLink>()
  for (const connection of connections) {
    const path = paths.get(connection.id)!.map((id) => remap.get(id) ?? id)
    for (let layer = 0; layer < path.length - 1; layer++) {
      const source = path[layer]
      const target = path[layer + 1]
      const id = JSON.stringify([source, target])
      let link = links.get(id)
      if (!link) {
        link = { source, target, count: 0, bytes: 0, connectionIds: new Set() }
        links.set(id, link)
      }
      link.count++
      link.bytes +=
        Math.max(0, connection.upload) + Math.max(0, connection.download)
      link.connectionIds.add(connection.id)
    }
  }
  return { layers, links: [...links.values()] }
}
